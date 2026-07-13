from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from .paths import ensure_data_layout


class PackageParamStore:
    """Persist last-used parameter values per installed script package."""

    def __init__(self, data_dir: Path | None = None):
        self.data_dir = ensure_data_layout(data_dir)
        self.path = self.data_dir / "package-params.json"

    def get_params(self, package_id: str, package_version: str) -> dict[str, Any] | None:
        entry = self._read().get(self._key(package_id, package_version))
        if not isinstance(entry, dict):
            return None
        params = entry.get("params")
        return dict(params) if isinstance(params, dict) else None

    def save_params(self, package_id: str, package_version: str, params: dict[str, Any]) -> None:
        data = self._read()
        data[self._key(package_id, package_version)] = {
            "package_id": package_id,
            "package_version": package_version,
            "params": params,
            "updated_at": datetime.now(UTC).isoformat(),
        }
        self._write(data)

    def _key(self, package_id: str, package_version: str) -> str:
        return f"{package_id}@{package_version}"

    def _read(self) -> dict[str, Any]:
        if not self.path.exists():
            return {}
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return {}
        return raw if isinstance(raw, dict) else {}

    def _write(self, data: dict[str, Any]) -> None:
        self.path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
