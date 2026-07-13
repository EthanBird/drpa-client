from __future__ import annotations

import json
import logging
from collections.abc import Callable
from pathlib import Path
from typing import Any


EmitFunc = Callable[[str], None]


class Context:
    """Runtime context passed to user RPA scripts."""

    def __init__(
        self,
        run_id: str,
        package_id: str,
        package_name: str,
        params: dict[str, Any],
        package_dir: Path,
        output_dir: Path,
        log_file: Path,
        emit: Callable[..., None],
    ):
        self.run_id = run_id
        self.package_id = package_id
        self.package_name = package_name
        self.params = params
        self.package_dir = package_dir
        self.output_dir = output_dir
        self.log_file = log_file
        self._emit = emit
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.log_file.parent.mkdir(parents=True, exist_ok=True)
        self.log = self._build_logger()

    def progress(self, value: int | float, message: str = "") -> None:
        self._emit("progress", value=value, message=message)

    def output_file(self, path: str | Path, label: str = "") -> Path:
        output_path = Path(path)
        if not output_path.is_absolute():
            output_path = self.output_dir / output_path
        self._emit("artifact", path=str(output_path), label=label or output_path.name)
        return output_path

    def browser(self, headless: bool | None = None):
        from DrissionPage import ChromiumOptions, ChromiumPage

        if headless is None:
            headless = bool(self.params.get("headless", False))
        options = ChromiumOptions()
        options.headless(headless)
        download_dir = self.output_dir / "downloads"
        download_dir.mkdir(parents=True, exist_ok=True)
        options.set_download_path(str(download_dir))
        return ChromiumPage(options)

    def emit_json(self, event_type: str, **payload: Any) -> None:
        self._emit(event_type, **payload)

    def close(self) -> None:
        for handler in list(self.log.handlers):
            handler.flush()
            handler.close()
            self.log.removeHandler(handler)

    def _build_logger(self) -> logging.Logger:
        logger = logging.getLogger(f"drpa.{self.package_id}.{self.run_id}")
        logger.setLevel(logging.INFO)
        logger.propagate = False

        file_handler = logging.FileHandler(self.log_file, encoding="utf-8")
        file_handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
        logger.addHandler(file_handler)

        logger.addHandler(_EventLogHandler(self._emit))
        return logger


class _EventLogHandler(logging.Handler):
    def __init__(self, emit: Callable[..., None]):
        super().__init__()
        self._emit = emit

    def emit(self, record: logging.LogRecord) -> None:
        payload = {
            "level": record.levelname.lower(),
            "message": record.getMessage(),
        }
        self._emit("log", **json.loads(json.dumps(payload, ensure_ascii=False)))
