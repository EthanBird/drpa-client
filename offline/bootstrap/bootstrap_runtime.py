from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


class BootstrapError(RuntimeError):
    pass


def bundle_root() -> Path:
    return Path(__file__).resolve().parent


def find_bundled_python(root: Path) -> Path:
    candidates = (
        list((root / "python").glob("*/python.exe"))
        + list((root / "python").glob("*/bin/python3.11"))
        + list((root / "python").glob("*/bin/python3"))
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise BootstrapError("bundled CPython executable is missing")


def environment_python(environment: Path) -> Path:
    return environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def find_runtime_adapter_wheel(wheelhouse: Path) -> Path:
    candidates = list(wheelhouse.glob("drpa_runtime_python-*.whl"))
    if not candidates:
        raise BootstrapError("drpa runtime adapter wheel is missing")

    def version_key(path: Path) -> tuple[tuple[int, ...], str]:
        version = path.name.removeprefix("drpa_runtime_python-").split("-", 1)[0]
        return tuple(int(part) for part in re.findall(r"\d+", version)), version

    return max(candidates, key=version_key)


def environment_python_version(python: Path) -> str | None:
    try:
        result = subprocess.run(
            [str(python), "-I", "-c", "import platform; print(platform.python_version())"],
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stdout.strip() if result.returncode == 0 else None


def runtime_adapter_is_healthy(python: Path) -> bool:
    try:
        result = subprocess.run(
            [
                str(python),
                "-I",
                "-c",
                "from drpa_runner.context import RuntimeContext; "
                "from drpa_runner import document_worker, python_flow; "
                "import docx, openpyxl, pptx, pypdf, reportlab, rpa, tagui; "
                "assert hasattr(RuntimeContext, 'open_output_directory'); "
                "assert document_worker.PROTOCOL_VERSION == 1; "
                "assert python_flow.SCHEMA_VERSION == 1",
            ],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def install_runtime_adapter(
    uv: Path,
    python: Path,
    wheel: Path,
    root: Path,
    offline_env: dict[str, str],
) -> None:
    subprocess.run(
        [
            str(uv),
            "pip",
            "install",
            "--python",
            str(python),
            "--offline",
            "--no-index",
            "--no-deps",
            "--reinstall",
            str(wheel),
        ],
        cwd=root,
        env=offline_env,
        check=True,
    )


def verify_environment(uv: Path, python: Path, root: Path, offline_env: dict[str, str]) -> None:
    subprocess.run(
        [str(uv), "pip", "check", "--python", str(python)],
        cwd=root,
        env=offline_env,
        check=True,
    )
    if not runtime_adapter_is_healthy(python):
        raise BootstrapError("drpa runtime adapter API verification failed")


def prepare(root: Path | None = None, environment_override: Path | None = None) -> Path:
    root = (root or bundle_root()).resolve()
    manifest = root / "manifest.json"
    requirements = root / "locks" / "runtime.txt"
    wheelhouse = root / "wheelhouse"
    uv = root / "tools" / ("uv.exe" if os.name == "nt" else "uv")
    for required in (manifest, requirements, wheelhouse, uv):
        if not required.exists():
            raise BootstrapError(f"offline bundle is incomplete: {required.relative_to(root)}")

    try:
        manifest_data = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise BootstrapError(f"offline runtime manifest is invalid: {error}") from error
    adapter_wheel = find_runtime_adapter_wheel(wheelhouse)

    environment = (environment_override or (root / "environment")).resolve()
    environment.parent.mkdir(parents=True, exist_ok=True)
    marker = environment / ".drpa-runtime.json"
    expected = {
        "schema": 2,
        "bundleVersion": manifest_data.get("bundleVersion", "unknown"),
        "pythonVersion": manifest_data.get("pythonVersion", ""),
        "requirementsSha256": digest_file(requirements),
        "runtimeAdapter": {
            "file": adapter_wheel.name,
            "sha256": digest_file(adapter_wheel),
        },
    }
    current: dict[str, object] = {}
    if marker.exists():
        try:
            loaded = json.loads(marker.read_text(encoding="utf-8"))
            current = loaded if isinstance(loaded, dict) else {}
        except (OSError, json.JSONDecodeError):
            current = {}

    generated_python = environment_python(environment)
    offline_env = os.environ.copy()
    offline_env.update(
        {
            "PIP_NO_INDEX": "1",
            "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            "UV_OFFLINE": "1",
            "UV_NO_MANAGED_PYTHON": "1",
            "UV_PYTHON_DOWNLOADS": "never",
            "UV_CACHE_DIR": str(environment.parent / ".drpa-uv-cache"),
        }
    )

    marker_has_python_version = (
        current.get("schema") == 2
        and current.get("pythonVersion") == expected["pythonVersion"]
    )
    environment_compatible = (
        generated_python.is_file()
        and current.get("requirementsSha256") == expected["requirementsSha256"]
        and (
            marker_has_python_version
            or environment_python_version(generated_python) == expected["pythonVersion"]
        )
    )
    if environment_compatible:
        adapter_current = current.get("runtimeAdapter") == expected["runtimeAdapter"]
        if adapter_current:
            return generated_python
        install_runtime_adapter(uv, generated_python, adapter_wheel, root, offline_env)
        verify_environment(uv, generated_python, root, offline_env)
        marker.write_text(json.dumps(expected, indent=2) + "\n", encoding="utf-8")
        return generated_python

    if environment.exists():
        shutil.rmtree(environment)

    python = find_bundled_python(root)
    subprocess.run(
        [str(uv), "venv", str(environment), "--python", str(python), "--no-project"],
        cwd=root,
        env=offline_env,
        check=True,
    )
    subprocess.run(
        [
            str(uv),
            "pip",
            "install",
            "--python",
            str(environment_python(environment)),
            "--offline",
            "--no-index",
            "--find-links",
            str(wheelhouse),
            "--requirement",
            str(requirements),
            str(adapter_wheel),
        ],
        cwd=root,
        env=offline_env,
        check=True,
    )
    verify_environment(uv, generated_python, root, offline_env)
    marker.write_text(json.dumps(expected, indent=2) + "\n", encoding="utf-8")
    return generated_python


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--environment",
        type=Path,
        help="Writable destination for the generated virtual environment",
    )
    args = parser.parse_args()
    try:
        python = prepare(environment_override=args.environment)
    except (BootstrapError, OSError, subprocess.CalledProcessError) as error:
        print(f"[DRPA offline] runtime preparation failed: {error}", file=sys.stderr)
        return 1
    print(python)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
