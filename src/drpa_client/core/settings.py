from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal

from .paths import ensure_data_layout


ThemeName = Literal["dark", "light"]


@dataclass(frozen=True)
class AppSettings:
    advanced_recorder_enabled: bool = False
    theme: ThemeName = "dark"


class SettingsStore:
    def __init__(self, data_dir: Path | None = None):
        self.data_dir = ensure_data_layout(data_dir)
        self.path = self.data_dir / "settings.json"

    def load(self) -> AppSettings:
        if not self.path.exists():
            return AppSettings()
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return AppSettings()
        return AppSettings(
            advanced_recorder_enabled=bool(raw.get("advanced_recorder_enabled", False)),
            theme=_theme(raw.get("theme")),
        )

    def save(self, settings: AppSettings) -> None:
        self.path.write_text(json.dumps(asdict(settings), ensure_ascii=False, indent=2), encoding="utf-8")


def _theme(value: object) -> ThemeName:
    if value in {"dark", "light"}:
        return value
    return "dark"
