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

from .models import InstalledPackage, TaskEvent
from .paths import ensure_data_layout
from .runtime_manager import RuntimeManager


EventCallback = Callable[[TaskEvent], None]


class RunningTask:
    def __init__(self, process: subprocess.Popen[str], config_path: Path):
        self.process = process
        self.config_path = config_path

    def stop(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()


class TaskRunner:
    def __init__(self, data_dir: Path | None = None, runtime_manager: RuntimeManager | None = None):
        self.data_dir = ensure_data_layout(data_dir)
        self.runtime_manager = runtime_manager or RuntimeManager()

    def start(
        self,
        package: InstalledPackage,
        params: dict[str, Any],
        on_event: EventCallback,
    ) -> RunningTask:
        run_id = datetime.now(UTC).strftime("%Y%m%d%H%M%S%f")
        output_dir = self.data_dir / "outputs" / package.manifest.id / run_id
        log_dir = self.data_dir / "logs" / package.manifest.id
        output_dir.mkdir(parents=True, exist_ok=True)
        log_dir.mkdir(parents=True, exist_ok=True)

        config = {
            "run_id": run_id,
            "package_id": package.manifest.id,
            "package_name": package.manifest.name,
            "package_dir": str(package.package_dir),
            "entry": package.manifest.entry,
            "params": params,
            "output_dir": str(output_dir),
            "log_file": str(log_dir / f"{run_id}.log"),
        }
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

        process = subprocess.Popen(
            [str(python), str(bootstrap), str(config_path)],
            cwd=package.package_dir,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )

        task = RunningTask(process, config_path)
        threading.Thread(
            target=self._pump_events,
            args=(task, on_event),
            name=f"drpa-task-{run_id}",
            daemon=True,
        ).start()
        return task

    def _pump_events(self, task: RunningTask, on_event: EventCallback) -> None:
        assert task.process.stdout is not None
        for line in task.process.stdout:
            line = line.rstrip("\n")
            if not line:
                continue
            try:
                raw = json.loads(line)
                on_event(TaskEvent(type=str(raw.get("type", "log")), payload=raw))
            except json.JSONDecodeError:
                on_event(TaskEvent(type="log", payload={"level": "info", "message": line}))

        exit_code = task.process.wait()
        on_event(TaskEvent(type="finished", payload={"exit_code": exit_code}))
        try:
            task.config_path.unlink(missing_ok=True)
        except OSError:
            print(f"failed to remove temp config: {task.config_path}", file=sys.stderr)
