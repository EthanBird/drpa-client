from __future__ import annotations

import json
import sqlite3
from collections.abc import Iterable
from pathlib import Path
from typing import Any

from .models import TaskRunRecord
from .paths import ensure_data_layout


class RunStore:
    """Small SQLite store for task run history.

    The package registry still uses install.lock files so that packages remain
    portable on disk. Task runs benefit from SQLite because they are append-heavy
    and need sorted queries for the GUI history page.
    """

    def __init__(self, data_dir: Path | None = None):
        self.data_dir = ensure_data_layout(data_dir)
        self.db_path = self.data_dir / "drpa-client.sqlite3"
        self._init_schema()

    def create_run(
        self,
        *,
        run_id: str,
        package_id: str,
        package_name: str,
        package_version: str,
        params: dict[str, Any],
        output_dir: Path,
        log_file: Path,
        started_at: str,
    ) -> None:
        with self._connect() as conn:
            conn.execute(
                """
                insert into task_runs(
                    id, package_id, package_name, package_version, status,
                    params_json, output_dir, log_file, started_at
                ) values (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    run_id,
                    package_id,
                    package_name,
                    package_version,
                    "running",
                    json.dumps(params, ensure_ascii=False),
                    str(output_dir),
                    str(log_file),
                    started_at,
                ),
            )

    def update_run(
        self,
        run_id: str,
        *,
        status: str | None = None,
        finished_at: str | None = None,
        exit_code: int | None = None,
    ) -> None:
        assignments: list[str] = []
        values: list[Any] = []
        if status is not None:
            assignments.append("status = ?")
            values.append(status)
        if finished_at is not None:
            assignments.append("finished_at = ?")
            values.append(finished_at)
        if exit_code is not None:
            assignments.append("exit_code = ?")
            values.append(exit_code)
        if not assignments:
            return

        values.append(run_id)
        with self._connect() as conn:
            conn.execute(
                f"update task_runs set {', '.join(assignments)} where id = ?",
                values,
            )

    def list_runs(self, limit: int = 200) -> list[TaskRunRecord]:
        with self._connect() as conn:
            rows = conn.execute(
                """
                select id, package_id, package_name, package_version, status,
                       params_json, output_dir, log_file, started_at, finished_at, exit_code
                from task_runs
                order by started_at desc
                limit ?
                """,
                (limit,),
            ).fetchall()
        return [self._row_to_record(row) for row in rows]

    def count_by_status(self, statuses: Iterable[str]) -> int:
        status_list = list(statuses)
        if not status_list:
            return 0
        placeholders = ",".join("?" for _ in status_list)
        with self._connect() as conn:
            row = conn.execute(
                f"select count(*) from task_runs where status in ({placeholders})",
                status_list,
            ).fetchone()
        return int(row[0])

    def _init_schema(self) -> None:
        with self._connect() as conn:
            conn.execute(
                """
                create table if not exists task_runs(
                    id text primary key,
                    package_id text not null,
                    package_name text not null,
                    package_version text not null,
                    status text not null,
                    params_json text not null,
                    output_dir text not null,
                    log_file text not null,
                    started_at text not null,
                    finished_at text,
                    exit_code integer
                )
                """
            )
            conn.execute(
                "create index if not exists idx_task_runs_started_at on task_runs(started_at desc)"
            )
            conn.execute("create index if not exists idx_task_runs_status on task_runs(status)")

    def _connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        return conn

    def _row_to_record(self, row: sqlite3.Row) -> TaskRunRecord:
        return TaskRunRecord(
            id=row["id"],
            package_id=row["package_id"],
            package_name=row["package_name"],
            package_version=row["package_version"],
            status=row["status"],
            params=json.loads(row["params_json"]),
            output_dir=Path(row["output_dir"]),
            log_file=Path(row["log_file"]),
            started_at=row["started_at"],
            finished_at=row["finished_at"],
            exit_code=row["exit_code"],
        )
