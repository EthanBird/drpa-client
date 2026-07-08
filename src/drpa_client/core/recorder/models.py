from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any, Literal


EventType = Literal[
    "navigate",
    "click",
    "input",
    "change",
    "submit",
    "keydown",
    "scroll",
    "download",
    "todo",
]


@dataclass(frozen=True)
class SelectorCandidate:
    kind: str
    value: str
    score: int
    reason: str = ""

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> SelectorCandidate:
        return cls(
            kind=str(raw.get("kind", "css")),
            value=str(raw.get("value", "")),
            score=int(raw.get("score", 0)),
            reason=str(raw.get("reason", "")),
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "kind": self.kind,
            "value": self.value,
            "score": self.score,
            "reason": self.reason,
        }


@dataclass(frozen=True)
class RecordingEvent:
    id: str
    type: EventType
    timestamp: str
    url: str
    title: str = ""
    target: dict[str, Any] = field(default_factory=dict)
    value: dict[str, Any] | None = None
    sensitive: bool = False
    confidence: str = "medium"
    notes: tuple[str, ...] = ()

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> RecordingEvent:
        return cls(
            id=str(raw.get("id", "")),
            type=str(raw.get("type", "todo")),
            timestamp=str(raw.get("timestamp") or datetime.now(UTC).isoformat()),
            url=str(raw.get("url", "")),
            title=str(raw.get("title", "")),
            target=dict(raw.get("target") or {}),
            value=raw.get("value"),
            sensitive=bool(raw.get("sensitive", False)),
            confidence=str(raw.get("confidence", "medium")),
            notes=tuple(str(item) for item in raw.get("notes") or ()),
        )

    @property
    def selectors(self) -> list[SelectorCandidate]:
        target = self.target or {}
        raw_selectors = target.get("selectors") or []
        candidates = [
            SelectorCandidate.from_dict(item)
            for item in raw_selectors
            if isinstance(item, dict) and item.get("value")
        ]
        return sorted(candidates, key=lambda item: item.score, reverse=True)

    @property
    def primary_selector(self) -> SelectorCandidate | None:
        selectors = self.selectors
        return selectors[0] if selectors else None

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "type": self.type,
            "timestamp": self.timestamp,
            "url": self.url,
            "title": self.title,
            "target": self.target,
            "value": self.value,
            "sensitive": self.sensitive,
            "confidence": self.confidence,
            "notes": list(self.notes),
        }


@dataclass(frozen=True)
class Recording:
    schema_version: str
    tool: str
    created_at: str
    start_url: str
    browser: dict[str, Any]
    events: tuple[RecordingEvent, ...]

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> Recording:
        return cls(
            schema_version=str(raw.get("schema_version", "1.0")),
            tool=str(raw.get("tool", "drpa-browser-recorder")),
            created_at=str(raw.get("created_at") or datetime.now(UTC).isoformat()),
            start_url=str(raw.get("start_url", "")),
            browser=dict(raw.get("browser") or {}),
            events=tuple(
                RecordingEvent.from_dict(item)
                for item in raw.get("events") or ()
                if isinstance(item, dict)
            ),
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "tool": self.tool,
            "created_at": self.created_at,
            "start_url": self.start_url,
            "browser": self.browser,
            "events": [event.to_dict() for event in self.events],
        }
