from __future__ import annotations

import sqlite3
from collections.abc import Iterable, Iterator, Mapping, Sequence
from contextlib import contextmanager
from pathlib import Path
from typing import Any, TypeAlias

SqlParameters: TypeAlias = Mapping[str, Any] | Sequence[Any]


class SqlClient:
    """Small SQLite client exposed as ``ctx.sql``."""

    def __init__(self, database_path: str | Path) -> None:
        self.database_path = Path(database_path).resolve()
        self.database_path.parent.mkdir(parents=True, exist_ok=True)
        self._connection = sqlite3.connect(
            self.database_path,
            timeout=30,
            isolation_level=None,
        )
        self._connection.row_factory = sqlite3.Row
        self._connection.execute("PRAGMA foreign_keys = ON")
        self._connection.execute("PRAGMA busy_timeout = 30000")
        self._connection.execute("PRAGMA journal_mode = WAL")
        self._transaction_depth = 0

    def execute(self, sql: str, parameters: SqlParameters = ()) -> int:
        """Execute one statement and return the affected row count."""

        cursor = self._connection.execute(sql, parameters)
        return max(0, cursor.rowcount)

    def executemany(self, sql: str, rows: Iterable[SqlParameters]) -> int:
        """Execute the same statement for multiple parameter rows."""

        cursor = self._connection.executemany(sql, rows)
        return max(0, cursor.rowcount)

    def query(
        self,
        sql: str,
        parameters: SqlParameters = (),
        *,
        limit: int | None = None,
    ) -> list[dict[str, Any]]:
        """Run a query and return rows as dictionaries."""

        cursor = self._connection.execute(sql, parameters)
        if cursor.description is None:
            return []
        rows = cursor.fetchall() if limit is None else cursor.fetchmany(max(0, int(limit)))
        return [dict(row) for row in rows]

    def scalar(
        self,
        sql: str,
        parameters: SqlParameters = (),
        *,
        default: Any = None,
    ) -> Any:
        """Return the first column of the first row, or ``default``."""

        row = self._connection.execute(sql, parameters).fetchone()
        return default if row is None else row[0]

    @contextmanager
    def transaction(self) -> Iterator["SqlClient"]:
        """Execute writes atomically; nested blocks use SQLite savepoints."""

        savepoint = f"drpa_nested_{self._transaction_depth}"
        if self._transaction_depth == 0:
            self._connection.execute("BEGIN IMMEDIATE")
        else:
            self._connection.execute(f"SAVEPOINT {savepoint}")
        self._transaction_depth += 1
        try:
            yield self
        except BaseException:
            self._transaction_depth -= 1
            if self._transaction_depth == 0:
                self._connection.execute("ROLLBACK")
            else:
                self._connection.execute(f"ROLLBACK TO SAVEPOINT {savepoint}")
                self._connection.execute(f"RELEASE SAVEPOINT {savepoint}")
            raise
        else:
            self._transaction_depth -= 1
            if self._transaction_depth == 0:
                self._connection.execute("COMMIT")
            else:
                self._connection.execute(f"RELEASE SAVEPOINT {savepoint}")

    def close(self) -> None:
        self._connection.close()

    def __enter__(self) -> "SqlClient":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()
