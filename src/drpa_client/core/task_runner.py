from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import threading
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

try:
    import psutil
except ImportError:  # pragma: no cover - fallback for minimal bootstrap environments
    psutil = None

from .database import RunStore
from .models import InstalledPackage, TaskEvent
from .paths import ensure_data_layout
from .runtime_manager import RuntimeManager


EventCallback = Callable[[TaskEvent], None]


class RunningTask:
    def __init__(self, run_id: str, process: subprocess.Popen[str], config_path: Path):
        self.run_id = run_id
        self.process = process
        self.config_path = config_path

    def stop(self) -> None:
        if self.process.poll() is not None:
            return
        if psutil is None:
            self.process.terminate()
            return
        try:
            parent = psutil.Process(self.process.pid)
            children = parent.children(recursive=True)
            for child in children:
                child.terminate()
            parent.terminate()
            _, alive = psutil.wait_procs([*children, parent], timeout=5)
            for proc in alive:
                proc.kill()
        except psutil.Error:
            self.process.terminate()


class TaskRunner:
    def __init__(
        self,
        data_dir: Path | None = None,
        runtime_manager: RuntimeManager | None = None,
        run_store: RunStore | None = None,
    ):
        self.data_dir = ensure_data_layout(data_dir)
        self.runtime_manager = runtime_manager or RuntimeManager()
        self.run_store = run_store or RunStore(self.data_dir)

    def start(
        self,
        package: InstalledPackage,
        params: dict[str, Any],
        on_event: EventCallback,
    ) -> RunningTask:
        run_id = datetime.now(UTC).strftime("%Y%m%d%H%M%S%f")
        started_at = datetime.now(UTC).isoformat()
        output_dir = self.data_dir / "outputs" / package.manifest.id / run_id
        log_dir = self.data_dir / "logs" / package.manifest.id
        output_dir.mkdir(parents=True, exist_ok=True)
        log_dir.mkdir(parents=True, exist_ok=True)
        log_file = log_dir / f"{run_id}.log"

        config = {
            "run_id": run_id,
            "package_id": package.manifest.id,
            "package_name": package.manifest.name,
            "package_version": package.manifest.version,
            "package_dir": str(package.package_dir),
            "entry": package.manifest.entry,
            "params": params,
            "output_dir": str(output_dir),
            "log_file": str(log_file),
        }
        self.run_store.create_run(
            run_id=run_id,
            package_id=package.manifest.id,
            package_name=package.manifest.name,
            package_version=package.manifest.version,
            params=params,
            output_dir=output_dir,
            log_file=log_file,
            started_at=started_at,
        )
        config_fd, config_name = tempfile.mkstemp(prefix="drpa-task-", suffix=".json")
        config_path = Path(config_name)
        with os.fdopen(config_fd, "w", encoding="utf-8") as fp:
            json.dump(config, fp, ensure_ascii=False)

        python = self.runtime_manager.python_executable(package.venv_dir)
        bootstrap = Path(__file__).resolve().parents[1] / "runtime" / "bootstrap.py"

        env = os.environ.copy()
        pythonpath = [str(Path(__file__).resolve().parents[2])]
        if env.get("PYTHONPATH"):
            pythonpath.append(env["PYTHONPATH"])
        env["PYTHONPATH"] = os.pathsep.join(pythonpath)
        env.setdefault("PYTHONIOENCODING", "utf-8")
        env.setdefault("PYTHONUTF8", "1")

        process = subprocess.Popen(
            [str(python), str(bootstrap), str(config_path)],
            cwd=package.package_dir,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )

        task = RunningTask(run_id, process, config_path)
        threading.Thread(
            target=self._pump_events,
            args=(task, on_event),
            name=f"drpa-task-{run_id}",
            daemon=True,
        ).start()
        return task

    def _pump_events(self, task: RunningTask, on_event: EventCallback) -> None:
        assert task.process.stdout is not None
        status = "running"
        for line in task.process.stdout:
            line = line.rstrip("\n")
            if not line:
                continue
            try:
                raw = json.loads(line)
                event = TaskEvent(type=str(raw.get("type", "log")), payload=raw)
                event.payload.setdefault("run_id", task.run_id)
                if event.type == "error":
                    status = "failed"
                elif event.type == "status" and raw.get("value"):
                    value = str(raw["value"])
                    if value in {"success", "failed", "cancelled", "running"}:
                        status = value
                on_event(event)
            except json.JSONDecodeError:
                on_event(
                    TaskEvent(
                        type="log",
                        payload={"level": "info", "message": line, "run_id": task.run_id},
                    )
                )

        exit_code = task.process.wait()
        if exit_code == 0 and status == "running":
            status = "success"
        elif exit_code == 130:
            status = "cancelled"
        elif exit_code != 0 and status not in {"failed", "cancelled"}:
            status = "failed"
        self.run_store.update_run(
            task.run_id,
            status=status,
            finished_at=datetime.now(UTC).isoformat(),
            exit_code=exit_code,
        )
        on_event(TaskEvent(type="finished", payload={"exit_code": exit_code, "run_id": task.run_id}))
        try:
            task.config_path.unlink(missing_ok=True)
        except OSError:
            print(f"failed to remove temp config: {task.config_path}", file=sys.stderr)
