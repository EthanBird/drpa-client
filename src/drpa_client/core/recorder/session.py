from __future__ import annotations

import json
from datetime import UTC, datetime
from importlib import resources
from typing import Any

from .models import Recording, RecordingEvent


class RecorderSessionError(RuntimeError):
    """Raised when a browser recording session cannot be controlled."""


class BrowserRecorderSession:
    """Best-effort browser recorder session using DrissionPage.

    This is the first-stage recorder backend. It focuses on launching a page,
    injecting the JS recorder agent, and pulling structured DOM events. Complex
    cases such as iframe/shadow DOM replay are intentionally left for later
    review and generated TODOs.
    """

    def __init__(self, start_url: str, headless: bool = False):
        self.start_url = start_url
        self.headless = headless
        self.created_at = datetime.now(UTC).isoformat()
        self._page: Any | None = None
        self._events: list[RecordingEvent] = []
        self._agent_source = _load_agent_source()

    def start(self) -> None:
        try:
            from DrissionPage import ChromiumOptions, ChromiumPage
        except ImportError as exc:
            raise RecorderSessionError("浏览器录制需要安装 DrissionPage") from exc

        options = ChromiumOptions()
        options.headless(self.headless)
        self._page = ChromiumPage(options)
        self._page.get(self.start_url)
        self.inject_agent()
        self._events.append(
            RecordingEvent(
                id="evt_000000",
                type="navigate",
                timestamp=datetime.now(UTC).isoformat(),
                url=self.start_url,
                title=self._safe_title(),
                confidence="high",
            )
        )

    def inject_agent(self) -> None:
        if self._page is None:
            raise RecorderSessionError("录制会话尚未启动")
        try:
            self._page.run_js(self._agent_source)
        except Exception as exc:  # noqa: BLE001 - DrissionPage may raise several runtime errors
            raise RecorderSessionError("注入浏览器录制脚本失败") from exc

    def poll_events(self) -> list[RecordingEvent]:
        if self._page is None:
            return []
        script = """
        const q = window.__DRPA_RECORDER_QUEUE__ || [];
        window.__DRPA_RECORDER_QUEUE__ = [];
        return JSON.stringify(q);
        """
        try:
            raw = self._page.run_js(script)
        except Exception:
            # Navigation may clear the injected script. Try to re-inject once.
            self.inject_agent()
            return []

        if not raw:
            return []
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError:
            return []
        events = [
            RecordingEvent.from_dict(item)
            for item in payload
            if isinstance(item, dict) and item.get("type")
        ]
        self._events.extend(events)
        return events

    def snapshot(self) -> Recording:
        return Recording(
            schema_version="1.0",
            tool="drpa-browser-recorder",
            created_at=self.created_at,
            start_url=self.start_url,
            browser={
                "engine": "chromium",
                "headless": self.headless,
            },
            events=tuple(self._events),
        )

    def stop(self) -> Recording:
        self.poll_events()
        recording = self.snapshot()
        if self._page is not None:
            try:
                self._page.quit()
            except Exception:
                pass
            self._page = None
        return recording

    def _safe_title(self) -> str:
        if self._page is None:
            return ""
        try:
            return str(self._page.title)
        except Exception:
            return ""


def _load_agent_source() -> str:
    resource = resources.files("drpa_client.resources.recorder").joinpath("agent.js")
    return resource.read_text(encoding="utf-8")
