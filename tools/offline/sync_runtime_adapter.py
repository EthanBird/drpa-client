from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

try:
    from tools.offline.build_runtime_bundle import create_inventory, create_wheelhouse_lock
except ModuleNotFoundError:  # Direct execution adds tools/offline to sys.path.
    from build_runtime_bundle import create_inventory, create_wheelhouse_lock


ROOT = Path(__file__).resolve().parents[2]
RUNTIME_PROJECT = ROOT / "runtime" / "python"
BOOTSTRAP = ROOT / "offline" / "bootstrap" / "bootstrap_runtime.py"


def verify_adapter_wheel(wheel: Path) -> None:
    with zipfile.ZipFile(wheel) as archive:
        context = archive.read("drpa_runner/context.py").decode("utf-8")
    if "def open_output_directory" not in context:
        raise RuntimeError("runtime adapter wheel is missing RuntimeContext.open_output_directory")


def build_adapter(output: Path) -> Path:
    subprocess.run(
        [
            sys.executable,
            "-m",
            "pip",
            "wheel",
            "--no-index",
            "--no-deps",
            "--no-build-isolation",
            "--disable-pip-version-check",
            "--wheel-dir",
            str(output),
            str(RUNTIME_PROJECT),
        ],
        cwd=ROOT,
        check=True,
    )
    wheels = list(output.glob("drpa_runtime_python-*.whl"))
    if len(wheels) != 1:
        raise RuntimeError(f"expected one runtime adapter wheel, found {len(wheels)}")
    verify_adapter_wheel(wheels[0])
    return wheels[0]


def sync(runtime_root: Path) -> Path:
    runtime_root = runtime_root.resolve()
    wheelhouse = runtime_root / "wheelhouse"
    wheelhouse.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="drpa-runtime-adapter-") as temporary:
        wheel = build_adapter(Path(temporary))
        destination = wheelhouse / wheel.name
        for stale in wheelhouse.glob("drpa_runtime_python-*.whl"):
            if stale.name != destination.name:
                stale.unlink()
        shutil.copy2(wheel, destination)
    shutil.copy2(BOOTSTRAP, runtime_root / BOOTSTRAP.name)
    verify_adapter_wheel(destination)
    refresh_sealed_metadata(runtime_root)
    return destination


def refresh_sealed_metadata(runtime_root: Path) -> None:
    manifest_path = runtime_root / "manifest.json"
    wheel_lock_path = runtime_root / "wheelhouse-lock.json"
    if not manifest_path.is_file() or not wheel_lock_path.is_file():
        raise RuntimeError("runtime manifest and wheelhouse lock are required before adapter sync")

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    wheel_lock = json.loads(wheel_lock_path.read_text(encoding="utf-8"))
    platform_id = str(manifest["platform"])
    chrome_platforms = {
        "windows-x86_64": "win64",
        "linux-x86_64": "linux64",
        "macos-arm64": "mac-arm64",
        "macos-x86_64": "mac-x64",
    }
    try:
        chrome_platform = chrome_platforms[platform_id]
    except KeyError as error:
        raise RuntimeError(f"unsupported runtime platform: {platform_id}") from error

    create_wheelhouse_lock(
        runtime_root,
        platform_id,
        str(manifest["pythonVersion"]),
        list(wheel_lock.get("sourceBuilds", [])),
    )
    create_inventory(runtime_root, manifest, platform_id, chrome_platform)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build and sync the lightweight DRPA Python adapter into a runtime/update tree."
    )
    parser.add_argument("--runtime-root", type=Path, required=True)
    args = parser.parse_args()
    destination = sync(args.runtime_root)
    print(f"synced {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
