from __future__ import annotations

import argparse
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RUNTIME_PROJECT = ROOT / "runtime" / "python"
BOOTSTRAP = ROOT / "offline" / "bootstrap" / "bootstrap_runtime.py"


def verify_adapter_wheel(wheel: Path) -> None:
    with zipfile.ZipFile(wheel) as archive:
        context = archive.read("drpa_runner/context.py").decode("utf-8")
    if "def open_output_directory" not in context:
        raise RuntimeError("runtime adapter wheel is missing RuntimeContext.open_output_directory")


def build_adapter(output: Path) -> Path:
    uv = shutil.which("uv")
    if not uv:
        raise RuntimeError("uv is required to build the runtime adapter")
    subprocess.run(
        [uv, "build", "--wheel", str(RUNTIME_PROJECT), "--out-dir", str(output)],
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
    return destination


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
