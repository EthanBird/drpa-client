from __future__ import annotations

import os
import shutil
import subprocess
import zipfile
from datetime import datetime
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
EXCLUDE_DIRS = {
    ".git",
    ".drpa-data",
    "__pycache__",
    ".pytest_cache",
    ".ruff_cache",
    "temp",
    "dist",
    "runtimes",
    "drpa_client.egg-info",
}
EXCLUDE_SUFFIXES = {".pyc", ".pyo"}


def resolve_uv() -> str:
    bundled = ROOT / ".tools" / "uv" / "uv.exe"
    if bundled.exists():
        return str(bundled)
    found = shutil.which("uv")
    if found:
        return found
    raise RuntimeError("找不到 uv，无法构建离线包")


def install_portable_package() -> None:
    """Replace editable install with a wheel so venv survives path changes."""
    venv_python = ROOT / ".venv" / "Scripts" / "python.exe"
    if not venv_python.exists():
        raise RuntimeError("缺少 .venv，无法安装可移植包")

    uv = resolve_uv()
    wheel_dir = DIST / "wheel-build"
    wheel_dir.mkdir(parents=True, exist_ok=True)
    for old_wheel in wheel_dir.glob("drpa_client-*.whl"):
        old_wheel.unlink()

    subprocess.run([uv, "build", "--out-dir", str(wheel_dir)], cwd=ROOT, check=True)
    wheels = sorted(wheel_dir.glob("drpa_client-*.whl"))
    if not wheels:
        raise RuntimeError("uv build 未生成 drpa_client wheel")

    subprocess.run(
        [
            uv,
            "pip",
            "install",
            "--python",
            str(venv_python),
            "--force-reinstall",
            "--no-deps",
            str(wheels[-1]),
        ],
        cwd=ROOT,
        check=True,
    )
    print(f"[pack] installed portable wheel: {wheels[-1].name}")


def ensure_windows_runtime() -> None:
    python_root = ROOT / ".tools" / "python"
    python_root.mkdir(parents=True, exist_ok=True)

    nested = python_root / "windows"
    if nested.exists():
        for item in nested.glob("cpython-3.11.9-*"):
            target = python_root / item.name
            if not target.exists():
                print(f"[pack] flatten python -> {target}")
                shutil.copytree(item, target)

    if not (ROOT / ".venv").exists() and (ROOT / "runtimes" / "windows" / ".venv").exists():
        print("[pack] restore root .venv from runtimes/windows/.venv")
        shutil.copytree(ROOT / "runtimes" / "windows" / ".venv", ROOT / ".venv")

    uv_dir = ROOT / ".tools" / "uv"
    uv_dir.mkdir(parents=True, exist_ok=True)
    uv_src = shutil.which("uv")
    if uv_src and Path(uv_src).exists():
        shutil.copy2(uv_src, uv_dir / "uv.exe")
        print("[pack] bundled uv.exe")

    if (ROOT / ".venv").exists():
        install_portable_package()

    fixer_python = next(python_root.glob("cpython-3.11.9-*/python.exe"), None)
    if fixer_python and (ROOT / ".venv").exists():
        result = subprocess.run(
            [str(fixer_python), str(ROOT / "tools" / "fix_offline_runtime.py")],
            cwd=ROOT,
            check=False,
            text=True,
            capture_output=True,
        )
        print(result.stdout.strip())
        if result.returncode != 0:
            print(result.stderr.strip())
            raise RuntimeError("offline runtime is not ready after packaging fixes")


def create_zip(output: Path) -> Path:
    output.parent.mkdir(parents=True, exist_ok=True)
    target = output
    if target.exists():
        try:
            target.unlink()
        except OSError:
            stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
            target = output.with_name(f"{output.stem}-{stamp}{output.suffix}")
            print(f"[pack] existing zip is locked, writing {target.name} instead")
    count = 0
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=1) as zf:
        for dirpath, dirnames, filenames in os.walk(ROOT):
            dirnames[:] = [d for d in dirnames if d not in EXCLUDE_DIRS]
            for name in filenames:
                if name.endswith(tuple(EXCLUDE_SUFFIXES)):
                    continue
                path = Path(dirpath) / name
                rel = path.relative_to(ROOT.parent)
                zf.write(path, rel.as_posix())
                count += 1
    print(f"[pack] done: {target} ({target.stat().st_size / 1024 / 1024:.1f} MiB), files={count}")
    return target


def main() -> None:
    ensure_windows_runtime()
    create_zip(DIST / "drpa-client-offline.zip")


if __name__ == "__main__":
    main()
