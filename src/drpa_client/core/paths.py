from __future__ import annotations

import platform
import shutil
import sys
from pathlib import Path


APP_NAME = "DRPA Client"
APP_AUTHOR = "drpa"

_BOOTSTRAP_HINT = (
    "请先运行 scripts/run-drpa-windows.bat（Windows）或 scripts/run-drpa.sh（Linux/macOS），"
    "或在项目目录执行：uv sync && uv run drpa-client"
)


def get_data_dir() -> Path:
    return get_project_root() / ".drpa-data"


def get_project_root() -> Path:
    for path in (Path.cwd().resolve(), *Path(__file__).resolve().parents):
        if (path / "pyproject.toml").exists() or (path / "examples").exists():
            return path
    return Path.cwd().resolve()


def get_project_venv_dir() -> Path:
    root = get_project_root()
    root_venv = root / ".venv"
    if root_venv.exists():
        return root_venv
    platform_name = "windows" if platform.system().lower() == "windows" else "linux"
    bundled = root / "runtimes" / platform_name / ".venv"
    if bundled.exists():
        return bundled
    return root_venv


def get_project_python() -> Path:
    venv_dir = get_project_venv_dir()
    if platform.system().lower() == "windows":
        return venv_dir / "Scripts" / "python.exe"
    return venv_dir / "bin" / "python"


def get_bundled_uv() -> Path | None:
    root = get_project_root()
    if platform.system().lower() == "windows":
        uv = root / ".tools" / "uv" / "uv.exe"
    else:
        uv = root / ".tools" / "uv" / "uv"
    return uv if uv.exists() else None


def get_bundled_python_root() -> Path:
    root = get_project_root()
    platform_name = "windows" if platform.system().lower() == "windows" else "linux"
    return root / ".tools" / "python" / platform_name


def resolve_uv_executable() -> str:
    bundled = get_bundled_uv()
    if bundled is not None:
        return str(bundled)
    found = shutil.which("uv")
    if found:
        return found
    raise RuntimeError(
        "找不到 uv 可执行文件。"
        f"{_BOOTSTRAP_HINT}"
    )


def require_project_python() -> Path:
    python = get_project_python()
    if not python.exists():
        raise RuntimeError(
            f"项目运行环境不存在：{python}\n{_BOOTSTRAP_HINT}"
        )
    return python


def assert_running_in_project_venv() -> Path:
    python = require_project_python()
    try:
        current = Path(sys.executable).resolve()
        expected = python.resolve()
    except OSError:
        current = Path(sys.executable)
        expected = python
    if current != expected:
        raise RuntimeError(
            "当前未使用项目 .venv 中的 Python，已禁止回退到系统 Python。\n"
            f"当前解释器：{current}\n"
            f"项目解释器：{expected}\n"
            f"{_BOOTSTRAP_HINT}"
        )
    return python


def ensure_data_layout(data_dir: Path | None = None) -> Path:
    root = data_dir or get_data_dir()
    for child in ("packages", "logs", "outputs", "cache"):
        (root / child).mkdir(parents=True, exist_ok=True)
    return root
