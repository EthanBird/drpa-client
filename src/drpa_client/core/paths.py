from __future__ import annotations

from pathlib import Path


APP_NAME = "DRPA Client"
APP_AUTHOR = "drpa"


def get_data_dir() -> Path:
    return get_project_root() / ".drpa-data"


def get_project_root() -> Path:
    for path in (Path.cwd().resolve(), *Path(__file__).resolve().parents):
        if (path / "pyproject.toml").exists() or (path / "examples").exists():
            return path
    return Path.cwd().resolve()


def get_project_venv_dir() -> Path:
    return get_project_root() / ".venv"


def ensure_data_layout(data_dir: Path | None = None) -> Path:
    root = data_dir or get_data_dir()
    for child in ("packages", "logs", "outputs", "cache"):
        (root / child).mkdir(parents=True, exist_ok=True)
    return root
