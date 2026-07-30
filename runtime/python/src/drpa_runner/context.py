from __future__ import annotations

import logging
import os
import importlib.util
import sys
import uuid
from pathlib import Path
from typing import Any

from .events import EventWriter
from .paths import resolve_child
from .sql import SqlClient


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
        database_path: Path,
        events: EventWriter,
        package_catalog: dict[str, dict[str, Any]] | None = None,
        invocation_stack: list[str] | None = None,
    ) -> None:
        self.run_id = run_id
        self.package_id = package_id
        self.params = params
        self.package_dir = package_dir.resolve()
        self.output_dir = output_dir.resolve()
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.sql = SqlClient(database_path)
        self._events = events
        self._package_catalog = package_catalog or {}
        self._invocation_stack = list(invocation_stack or [package_id])
        self.log = self._build_logger()

    def progress(self, value: int | float, message: str = "") -> None:
        normalized = min(100.0, max(0.0, float(value)))
        self._events.emit("progress", value=normalized, message=message or None)

    def output_file(self, relative_path: str | Path, label: str = "") -> Path:
        path = resolve_child(self.output_dir, relative_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        self._events.emit("artifact", path=str(path), label=label or path.name, media_type=None)
        return path

    def open_output_directory(self) -> None:
        """Ask the desktop Host to reveal this run's output directory."""

        self._events.emit("open_directory", path=str(self.output_dir))

    def browser(self, *, headless: bool | None = None):
        """Create a DrissionPage browser only when the package requests it."""

        try:
            from DrissionPage import ChromiumOptions, ChromiumPage
        except ImportError as exc:  # pragma: no cover - optional adapter feature
            raise RuntimeError("browser capability requires the DrissionPage runtime feature") from exc

        if headless is None:
            headless = bool(self.params.get("headless", False))
        options = ChromiumOptions()
        browser_path = os.environ.get("DRPA_BROWSER_PATH")
        if browser_path:
            options.set_browser_path(browser_path)
        options.headless(headless)
        download_dir = resolve_child(self.output_dir, "downloads")
        download_dir.mkdir(parents=True, exist_ok=True)
        options.set_download_path(str(download_dir))
        return ChromiumPage(options)

    def invoke(self, package_id: str, params: dict[str, Any] | None = None) -> Any:
        """Invoke an installed RPAZ package and return its Python result.

        The child package shares the read-only workspace database and receives its
        own output subdirectory. Circular calls and excessive nesting are rejected.
        """

        descriptor = self._package_catalog.get(package_id)
        if descriptor is None:
            raise KeyError(f"installed RPAZ package not found: {package_id}")
        if package_id in self._invocation_stack:
            chain = " -> ".join([*self._invocation_stack, package_id])
            raise RuntimeError(f"circular RPAZ package invocation: {chain}")
        if len(self._invocation_stack) >= 16:
            raise RuntimeError("RPAZ package invocation depth exceeds 16")
        package_dir = Path(str(descriptor["package_dir"])).resolve()
        entrypoint = resolve_child(package_dir, str(descriptor["entrypoint"]), must_exist=True)
        callable_name = str(descriptor.get("callable") or "main")
        child_output = resolve_child(
            self.output_dir,
            Path("packages") / package_id.replace(".", "_") / uuid.uuid4().hex,
        )
        child = RuntimeContext(
            run_id=self.run_id,
            package_id=package_id,
            params=dict(params or {}),
            package_dir=package_dir,
            output_dir=child_output,
            database_path=self.sql.database_path,
            events=self._events,
            package_catalog=self._package_catalog,
            invocation_stack=[*self._invocation_stack, package_id],
        )
        module_name = f"drpa_package_{package_id.replace('.', '_')}_{uuid.uuid4().hex}"
        spec = importlib.util.spec_from_file_location(module_name, entrypoint)
        if spec is None or spec.loader is None:
            raise RuntimeError(f"unable to load RPAZ package entrypoint: {entrypoint.name}")
        module = importlib.util.module_from_spec(spec)
        sys.modules[module_name] = module
        try:
            spec.loader.exec_module(module)
            entry = getattr(module, callable_name, None)
            if not callable(entry):
                raise TypeError(f"RPAZ package must define callable {callable_name}(ctx)")
            return entry(child)
        finally:
            child.sql.close()
            sys.modules.pop(module_name, None)

    invoke_package = invoke

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
