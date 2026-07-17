from __future__ import annotations

import importlib.util
import sys
import traceback
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .context import RuntimeContext
from .events import EventWriter
from .paths import resolve_child


@dataclass(frozen=True)
class ExecutionRequest:
    protocol: int
    run_id: str
    package_id: str
    package_dir: Path
    output_dir: Path
    entrypoint: str
    callable: str
    parameters: dict[str, Any]
    database_path: Path | None = None

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> "ExecutionRequest":
        return cls(
            protocol=int(raw["protocol"]),
            run_id=str(raw["run_id"]),
            package_id=str(raw["package_id"]),
            package_dir=Path(raw["package_dir"]),
            output_dir=Path(raw["output_dir"]),
            entrypoint=str(raw["entrypoint"]),
            callable=str(raw.get("callable") or "main"),
            parameters=dict(raw.get("parameters") or {}),
            database_path=Path(raw["database_path"]) if raw.get("database_path") else None,
        )


def execute_request(request: ExecutionRequest, events: EventWriter) -> int:
    if request.protocol != 1:
        events.emit("error", message=f"unsupported runtime protocol: {request.protocol}", traceback=None)
        return 2

    package_dir = request.package_dir.resolve()
    entrypoint = resolve_child(package_dir, request.entrypoint, must_exist=True)
    output_dir = request.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    context = RuntimeContext(
        run_id=request.run_id,
        package_id=request.package_id,
        params=request.parameters,
        package_dir=package_dir,
        output_dir=output_dir,
        database_path=(
            request.database_path.resolve()
            if request.database_path is not None
            else (output_dir / ".drpa-runtime.sqlite3").resolve()
        ),
        events=events,
    )

    events.emit("ready", protocol=1)
    try:
        module = _load_entrypoint(entrypoint, request.run_id)
        entry = getattr(module, request.callable, None)
        if not callable(entry):
            raise TypeError(f"package entrypoint must define callable {request.callable}(ctx)")
        entry(context)
    except KeyboardInterrupt:
        events.emit("warning", message="run cancelled")
        events.emit("completed", exit_code=130)
        return 130
    except Exception as exc:  # noqa: BLE001 - package code is the isolation boundary
        events.emit("error", message=str(exc), traceback=traceback.format_exc())
        events.emit("completed", exit_code=1)
        return 1
    finally:
        context.sql.close()

    events.emit("completed", exit_code=0)
    return 0


def _load_entrypoint(path: Path, run_id: str):
    module_name = f"drpa_package_{run_id.replace('-', '_')}"
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to load package entrypoint: {path.name}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module
