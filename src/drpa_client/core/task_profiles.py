from __future__ import annotations

import json
import uuid
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from .paths import ensure_data_layout


@dataclass(frozen=True)
class TaskProfile:
    id: str
    name: str
    package_id: str
    package_version: str
    params: dict[str, Any]
    created_at: str
    updated_at: str


class TaskProfileStore:
    def __init__(self, data_dir: Path | None = None):
        self.data_dir = ensure_data_layout(data_dir)
        self.path = self.data_dir / "task-profiles.json"

    def list_profiles(self) -> list[TaskProfile]:
        if not self.path.exists():
            return []
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return []
        return [
            TaskProfile(
                id=str(item["id"]),
                name=str(item["name"]),
                package_id=str(item["package_id"]),
                package_version=str(item["package_version"]),
                params=dict(item.get("params") or {}),
                created_at=str(item.get("created_at", "")),
                updated_at=str(item.get("updated_at", "")),
            )
            for item in raw
            if isinstance(item, dict) and item.get("id")
        ]

    def save_profile(
        self,
        *,
        name: str,
        package_id: str,
        package_version: str,
        params: dict[str, Any],
        profile_id: str | None = None,
    ) -> TaskProfile:
        profiles = self.list_profiles()
        now = datetime.now(UTC).isoformat()
        if profile_id:
            created_at = next((item.created_at for item in profiles if item.id == profile_id), now)
            profile = TaskProfile(
                id=profile_id,
                name=name,
                package_id=package_id,
                package_version=package_version,
                params=params,
                created_at=created_at,
                updated_at=now,
            )
            profiles = [profile if item.id == profile_id else item for item in profiles]
        else:
            profile = TaskProfile(
                id=uuid.uuid4().hex,
                name=name,
                package_id=package_id,
                package_version=package_version,
                params=params,
                created_at=now,
                updated_at=now,
            )
            profiles.append(profile)
        self._write(profiles)
        return profile

    def delete_profile(self, profile_id: str) -> None:
        self._write([item for item in self.list_profiles() if item.id != profile_id])

    def _write(self, profiles: list[TaskProfile]) -> None:
        payload = [asdict(item) for item in profiles]
        self.path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
