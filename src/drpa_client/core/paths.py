from __future__ import annotations

import os
from pathlib import Path

try:
    from platformdirs import user_data_dir
except ImportError:  # pragma: no cover - used only in stripped-down runtimes
    user_data_dir = None


APP_NAME = "DRPA Client"
APP_AUTHOR = "drpa"


def get_data_dir() -> Path:
    override = os.getenv("DRPA_DATA_DIR")
    if override:
        return Path(override).expanduser().resolve()
    if user_data_dir is not None:
        return Path(user_data_dir(APP_NAME, APP_AUTHOR)).resolve()
    return (Path.home() / ".drpa-client").resolve()


def ensure_data_layout(data_dir: Path | None = None) -> Path:
    root = data_dir or get_data_dir()
    for child in ("packages", "logs", "outputs", "cache"):
        (root / child).mkdir(parents=True, exist_ok=True)
    return root
