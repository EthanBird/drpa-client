from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from .events import EventWriter
from .paths import resolve_child


class RuntimeContext:
    """Narrow SDK surface exposed to package entrypoints."""

    def __init__(
        self,
        *,
        run_id: str,
        package_id: str,
        params: dict[str, Any],
        package_dir: Path,
        output_dir: Path,
        events: EventWriter,
    ) -> None:
        self.run_id = run_id
        self.package_id = package_id
        self.params = params
        self.package_dir = package_dir.resolve()
        self.output_dir = output_dir.resolve()
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self._events = events
        self.log = self._build_logger()

    def progress(self, value: int | float, message: str = "") -> None:
        normalized = min(100.0, max(0.0, float(value)))
        self._events.emit("progress", value=normalized, message=message or None)

    def output_file(self, relative_path: str | Path, label: str = "") -> Path:
        path = resolve_child(self.output_dir, relative_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        self._events.emit("artifact", path=str(path), label=label or path.name, media_type=None)
        return path

    def browser(self, *, headless: bool | None = None):
        """Create a DrissionPage browser only when the package requests it."""

        try:
            from DrissionPage import ChromiumOptions, ChromiumPage
        except ImportError as exc:  # pragma: no cover - optional adapter feature
            raise RuntimeError("browser capability requires the DrissionPage runtime feature") from exc

        if headless is None:
            headless = bool(self.params.get("headless", False))
        options = ChromiumOptions()
        options.headless(headless)
        download_dir = resolve_child(self.output_dir, "downloads")
        download_dir.mkdir(parents=True, exist_ok=True)
        options.set_download_path(str(download_dir))
        return ChromiumPage(options)

    def _build_logger(self) -> logging.Logger:
        logger = logging.getLogger(f"drpa.runtime.{self.run_id}")
        logger.handlers.clear()
        logger.setLevel(logging.INFO)
        logger.propagate = False
        logger.addHandler(_RuntimeLogHandler(self._events))
        return logger


class _RuntimeLogHandler(logging.Handler):
    def __init__(self, events: EventWriter) -> None:
        super().__init__()
        self._events = events

    def emit(self, record: logging.LogRecord) -> None:
        self._events.emit(
            "log",
            level=record.levelname.lower(),
            scope="package",
            message=record.getMessage(),
        )
