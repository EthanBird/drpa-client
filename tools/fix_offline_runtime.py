from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def find_bundled_python(project_root: Path) -> Path | None:
    candidates: list[Path] = []
    for base in (
        project_root / ".tools" / "python",
        project_root / ".tools" / "python" / "windows",
    ):
        if not base.exists():
            continue
        for item in sorted(base.glob("cpython-3.11.9-*")):
            python_exe = item / ("python.exe" if sys.platform == "win32" else "bin/python3")
            if python_exe.exists():
                candidates.append(item)
    return candidates[0] if candidates else None


def get_venv_dir(project_root: Path) -> Path:
    root_venv = project_root / ".venv"
    if root_venv.exists():
        return root_venv
    return project_root / "runtimes" / "windows" / ".venv"


def get_site_packages(venv_dir: Path) -> Path:
    return venv_dir / "Lib" / "site-packages"


def patch_pyvenv_cfg(project_root: Path, venv_dir: Path, bundled_python: Path) -> bool:
    cfg_path = venv_dir / "pyvenv.cfg"
    if not cfg_path.exists():
        return False

    lines: list[str] = []
    for line in cfg_path.read_text(encoding="utf-8").splitlines():
        if line.startswith("home = "):
            lines.append(f"home = {bundled_python}")
        else:
            lines.append(line)
    cfg_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return True


def fix_editable_install(project_root: Path, venv_dir: Path) -> None:
    site_packages = get_site_packages(venv_dir)
    src_path = project_root / "src"
    for pth in site_packages.glob("__editable__.drpa_client*.pth"):
        pth.write_text(f"{src_path}\n", encoding="utf-8")


def write_portable_launcher(project_root: Path, venv_dir: Path) -> None:
    """Avoid uv trampoline exes that embed build-machine absolute paths."""
    if sys.platform != "win32":
        return
    scripts_dir = venv_dir / "Scripts"
    launcher = scripts_dir / "drpa-client-portable.cmd"
    launcher.write_text(
        "@echo off\r\n"
        'set "PY=%~dp0python.exe"\r\n'
        'if not exist "%PY%" (\r\n'
        '  echo [DRPA] Missing venv python: %PY%\r\n'
        "  exit /b 1\r\n"
        ")\r\n"
        '"%PY%" -m drpa_client.app.main %*\r\n',
        encoding="utf-8",
    )


def probe_import(project_root: Path, venv_dir: Path) -> tuple[bool, str]:
    if sys.platform == "win32":
        python_exe = venv_dir / "Scripts" / "python.exe"
    else:
        python_exe = venv_dir / "bin" / "python"
    if not python_exe.exists():
        return False, f"Python 不存在：{python_exe}"

    try:
        result = subprocess.run(
            [str(python_exe), "-c", "import drpa_client; print(drpa_client.__file__)"],
            cwd=project_root,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:
        return False, f"无法启动 Python：{exc}"

    if result.returncode == 0:
        return True, result.stdout.strip()

    details = (result.stderr or result.stdout or "unknown import error").strip()
    return False, details


def diagnose(project_root: Path) -> list[str]:
    issues: list[str] = []
    venv_dir = get_venv_dir(project_root)
    bundled_python = find_bundled_python(project_root)

    if bundled_python is None:
        issues.append(f"缺少内置 Python：{project_root / '.tools' / 'python'}")
    if not venv_dir.exists():
        issues.append(f"缺少虚拟环境：{venv_dir}")
    if not (project_root / "src" / "drpa_client").exists():
        issues.append(f"缺少源码目录：{project_root / 'src' / 'drpa_client'}")

    if venv_dir.exists():
        ok, detail = probe_import(project_root, venv_dir)
        if not ok:
            issues.append(f"import drpa_client 失败：{detail}")
    return issues


def prepare_runtime(project_root: Path) -> tuple[bool, str]:
    venv_dir = get_venv_dir(project_root)
    bundled_python = find_bundled_python(project_root)
    if bundled_python is None:
        return False, "找不到内置 Python"
    if not venv_dir.exists():
        return False, "找不到 .venv"

    patch_pyvenv_cfg(project_root, venv_dir, bundled_python)
    fix_editable_install(project_root, venv_dir)
    write_portable_launcher(project_root, venv_dir)

    ok, detail = probe_import(project_root, venv_dir)
    if ok:
        return True, detail
    return False, detail


def main() -> int:
    project_root = Path(__file__).resolve().parents[1]
    ok, detail = prepare_runtime(project_root)
    if ok:
        print("ready")
        print(f"[runtime] {detail}")
        return 0

    print("broken")
    print(f"[runtime] {detail}")
    for line in diagnose(project_root):
        print(f"[runtime] {line}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
