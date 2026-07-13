from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path
from typing import Any

from validate_requirements import validate_requirements


ROOT = Path(__file__).resolve().parents[2]
SPEC_PATH = ROOT / "offline" / "runtime-spec.json"
REQUIREMENTS = ROOT / "offline" / "requirements" / "runtime.txt"
BOOTSTRAP = ROOT / "offline" / "bootstrap"
RUNTIME_PROJECT = ROOT / "runtime" / "python"


def run(command: list[str], *, env: dict[str, str] | None = None, capture: bool = False) -> str:
    print("+", " ".join(command))
    result = subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        check=True,
        text=True,
        capture_output=capture,
    )
    return result.stdout.strip() if capture else ""


def load_spec() -> dict[str, Any]:
    return json.loads(SPEC_PATH.read_text(encoding="utf-8"))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(url: str, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    print(f"download {url}")
    request = urllib.request.Request(url, headers={"User-Agent": "DRPA-offline-builder/1"})
    with urllib.request.urlopen(request, timeout=120) as response, target.open("wb") as output:
        shutil.copyfileobj(response, output, length=1024 * 1024)


def safe_extract_zip(archive: Path, destination: Path) -> None:
    destination = destination.resolve()
    with zipfile.ZipFile(archive) as source:
        for item in source.infolist():
            target = (destination / item.filename).resolve()
            if target != destination and destination not in target.parents:
                raise RuntimeError(f"archive path escapes destination: {item.filename}")
            source.extract(item, destination)
            mode = (item.external_attr >> 16) & 0o777
            if mode and target.exists():
                target.chmod(mode)


def bundled_python(stage: Path) -> Path:
    candidates = (
        list((stage / "python").glob("*/python.exe"))
        + list((stage / "python").glob("*/bin/python3.11"))
        + list((stage / "python").glob("*/bin/python3"))
    )
    if not candidates:
        raise RuntimeError("uv did not install a bundled Python executable")
    return candidates[0]


def environment_python(environment: Path) -> Path:
    return environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def browser_executable(stage: Path, chrome_platform: str) -> Path:
    paths = {
        "win64": stage / "browser" / "chrome-win64" / "chrome.exe",
        "linux64": stage / "browser" / "chrome-linux64" / "chrome",
        "mac-arm64": stage / "browser" / "chrome-mac-arm64" / "Google Chrome for Testing.app" / "Contents" / "MacOS" / "Google Chrome for Testing",
        "mac-x64": stage / "browser" / "chrome-mac-x64" / "Google Chrome for Testing.app" / "Contents" / "MacOS" / "Google Chrome for Testing",
    }
    return paths[chrome_platform]


def create_inventory(stage: Path, spec: dict[str, Any], platform_id: str, chrome_platform: str) -> None:
    files = []
    for path in sorted(stage.rglob("*")):
        if path.is_file() and path.name not in {"manifest.json", "SHA256SUMS"}:
            files.append(
                {
                    "path": path.relative_to(stage).as_posix(),
                    "bytes": path.stat().st_size,
                    "sha256": sha256(path),
                }
            )
    manifest = {
        "schema": 1,
        "bundleVersion": spec["bundleVersion"],
        "platform": platform_id,
        "pythonVersion": spec["pythonVersion"],
        "pythonExecutable": bundled_python(stage).relative_to(stage).as_posix(),
        "browserExecutable": browser_executable(stage, chrome_platform).relative_to(stage).as_posix(),
        "uvVersion": spec["uvVersion"],
        "chromeForTestingVersion": spec["chromeForTestingVersion"],
        "files": files,
    }
    (stage / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    checksums = []
    for path in sorted(stage.rglob("*")):
        if path.is_file() and path.name != "SHA256SUMS":
            checksums.append(f"{sha256(path)}  {path.relative_to(stage).as_posix()}")
    (stage / "SHA256SUMS").write_text("\n".join(checksums) + "\n", encoding="utf-8")


def archive_bundle(stage: Path, output: Path, archive_kind: str) -> Path:
    output.mkdir(parents=True, exist_ok=True)
    if archive_kind == "zip":
        target = output / f"{stage.name}.zip"
        with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            for path in sorted(stage.rglob("*")):
                archive.write(path, Path(stage.name) / path.relative_to(stage))
    else:
        target = output / f"{stage.name}.tar.gz"
        with tarfile.open(target, "w:gz", compresslevel=6) as archive:
            archive.add(stage, arcname=stage.name, recursive=True)
    (output / f"{target.name}.sha256").write_text(f"{sha256(target)}  {target.name}\n", encoding="utf-8")
    return target


def smoke_test(stage: Path, chrome_platform: str) -> None:
    smoke = stage.parent / "smoke-environment"
    cache = stage.parent / "empty-uv-cache"
    offline_env = os.environ.copy()
    offline_env.update(
        {
            "PIP_NO_INDEX": "1",
            "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            "UV_OFFLINE": "1",
            "UV_NO_MANAGED_PYTHON": "1",
            "UV_PYTHON_DOWNLOADS": "never",
            "UV_CACHE_DIR": str(cache),
        }
    )
    # Exercise the same entry point used by the desktop Host. Calling uv directly
    # previously allowed a broken Host interpreter lookup to pass the release job.
    run(
        [
            str(bundled_python(stage)),
            str(stage / "bootstrap_runtime.py"),
            "--environment",
            str(smoke),
        ],
        env=offline_env,
    )
    python = environment_python(smoke)
    browser = browser_executable(stage, chrome_platform)
    if not browser.exists():
        raise RuntimeError(f"Chrome for Testing is missing: {browser}")
    marker = stage.parent / "offline-smoke.html"
    marker.write_text("<title>DRPA_OFFLINE_OK</title><main>sealed runtime</main>", encoding="utf-8")
    smoke_script = (
        "from DrissionPage import ChromiumOptions, ChromiumPage;"
        f"o=ChromiumOptions().set_browser_path({str(browser)!r}).auto_port().headless(True);"
        "p=ChromiumPage(o);"
        f"p.get({marker.resolve().as_uri()!r});"
        "assert p.title=='DRPA_OFFLINE_OK', p.title;"
        "p.quit();"
        "import drpa_runner,pandas,openpyxl,xlwt,requests;"
        "print('DRPA offline smoke test passed')"
    )
    run([str(python), "-c", smoke_script], env={**offline_env, "DRPA_BROWSER_PATH": str(browser)})
    kernel_input = "\n".join(
        [
            json.dumps({"type": "execute", "request_id": "one", "code": "value = 40\\nprint('ready')"}),
            json.dumps({"type": "execute", "request_id": "two", "code": "value + 2"}),
            "",
        ]
    )
    kernel = subprocess.run(
        [str(python), "-m", "drpa_runner.kernel"],
        cwd=ROOT,
        env=offline_env,
        input=kernel_input,
        text=True,
        capture_output=True,
        timeout=90,
        check=False,
    )
    if kernel.returncode != 0:
        raise RuntimeError(f"Jupyter Kernel protocol smoke failed: {kernel.stderr}")
    responses = [json.loads(line) for line in kernel.stdout.splitlines() if line.strip()]
    if len(responses) != 2 or responses[0]["stdout"] != "ready\n" or responses[1]["result"] != "42":
        raise RuntimeError(f"Jupyter Kernel returned unexpected responses: {responses}")


def build(platform_id: str, work_dir: Path) -> Path:
    spec = load_spec()
    requirement_errors = validate_requirements(REQUIREMENTS)
    if requirement_errors:
        raise RuntimeError("invalid sealed requirements:\n" + "\n".join(requirement_errors))
    target = spec["platforms"][platform_id]
    if platform.system() != target["system"] or platform.machine() not in target["machine"]:
        raise RuntimeError(
            f"runner mismatch for {platform_id}: got {platform.system()} {platform.machine()}"
        )
    if sys.version_info[:3] != tuple(int(part) for part in spec["pythonVersion"].split(".")):
        raise RuntimeError(f"builder must run on Python {spec['pythonVersion']}")

    bundle_name = f"drpa-runtime-{spec['bundleVersion']}-py{spec['pythonVersion']}-{platform_id}"
    stage = work_dir / bundle_name
    if stage.exists():
        shutil.rmtree(stage)
    for directory in (stage / "python", stage / "tools", stage / "wheelhouse", stage / "locks", stage / "browser"):
        directory.mkdir(parents=True, exist_ok=True)

    uv_source = Path(shutil.which("uv") or "")
    if not uv_source.is_file():
        raise RuntimeError("uv executable is not installed")
    uv_target = stage / "tools" / ("uv.exe" if os.name == "nt" else "uv")
    shutil.copy2(uv_source, uv_target)
    uv_target.chmod(uv_target.stat().st_mode | stat.S_IXUSR)

    managed_env = os.environ.copy()
    managed_env["UV_PYTHON_INSTALL_DIR"] = str(stage / "python")
    run([str(uv_source), "python", "install", spec["pythonVersion"]], env=managed_env)

    run([sys.executable, "-m", "pip", "download", "--only-binary=:all:", "--dest", str(stage / "wheelhouse"), "--requirement", str(REQUIREMENTS)])
    run([str(uv_source), "build", str(RUNTIME_PROJECT), "--out-dir", str(stage / "wheelhouse")])
    shutil.copy2(REQUIREMENTS, stage / "locks" / "runtime.txt")
    for bootstrap in BOOTSTRAP.iterdir():
        if bootstrap.is_file():
            copied = stage / bootstrap.name
            shutil.copy2(bootstrap, copied)
            if copied.suffix == ".sh":
                copied.chmod(copied.stat().st_mode | stat.S_IXUSR)

    chrome_version = spec["chromeForTestingVersion"]
    chrome_platform = target["chromePlatform"]
    chrome_archive = work_dir / f"chrome-{chrome_platform}.zip"
    download(
        f"https://storage.googleapis.com/chrome-for-testing-public/{chrome_version}/{chrome_platform}/chrome-{chrome_platform}.zip",
        chrome_archive,
    )
    safe_extract_zip(chrome_archive, stage / "browser")
    create_inventory(stage, spec, platform_id, chrome_platform)
    smoke_test(stage, chrome_platform)
    return archive_bundle(stage, work_dir / "out", target["archive"])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    args = parser.parse_args()
    args.work_dir.mkdir(parents=True, exist_ok=True)
    artifact = build(args.platform, args.work_dir.resolve())
    print(f"created {artifact} ({artifact.stat().st_size / 1024 / 1024:.1f} MiB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
