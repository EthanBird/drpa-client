from __future__ import annotations

import ast
import contextlib
import io
import json
import sys
import time
import traceback
from typing import Any


class StudioKernel:
    def __init__(self) -> None:
        self.namespace: dict[str, Any] = {
            "__name__": "__main__",
            "__package__": None,
        }
        self.execution_count = 0

    def execute(self, request_id: str, source: str) -> dict[str, Any]:
        self.execution_count += 1
        stdout = io.StringIO()
        stderr = io.StringIO()
        result: str | None = None
        error: str | None = None
        trace: list[str] = []
        started = time.perf_counter()
        try:
            tree = ast.parse(source, filename=f"<DRPA cell {self.execution_count}>", mode="exec")
            final_expression = tree.body.pop() if tree.body and isinstance(tree.body[-1], ast.Expr) else None
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                if tree.body:
                    exec(compile(tree, "<DRPA cell>", "exec"), self.namespace)
                if final_expression is not None:
                    value = eval(
                        compile(ast.Expression(final_expression.value), "<DRPA cell result>", "eval"),
                        self.namespace,
                    )
                    if value is not None:
                        result = _safe_preview(value, limit=20_000)
        except BaseException as exception:  # noqa: BLE001 - notebook cells must report user exceptions
            error = f"{type(exception).__name__}: {exception}"
            trace = traceback.format_exception(type(exception), exception, exception.__traceback__)
        return {
            "request_id": request_id,
            "execution_count": self.execution_count,
            "stdout": stdout.getvalue(),
            "stderr": stderr.getvalue(),
            "result": result,
            "error": error,
            "traceback": trace,
            "variables": self._variables(),
            "duration_ms": round((time.perf_counter() - started) * 1000),
        }

    def _variables(self) -> list[dict[str, str]]:
        variables = []
        for name, value in sorted(self.namespace.items()):
            if name.startswith("_") or callable(value) or isinstance(value, type(sys)):
                continue
            variables.append(
                {
                    "name": name,
                    "type_name": type(value).__name__,
                    "preview": _safe_preview(value, limit=240),
                }
            )
            if len(variables) >= 100:
                break
        return variables


def _safe_preview(value: Any, *, limit: int) -> str:
    try:
        preview = repr(value)
    except BaseException:  # noqa: BLE001 - repr hooks are user code
        preview = f"<{type(value).__name__}>"
    return preview if len(preview) <= limit else preview[: limit - 1] + "…"


def main() -> int:
    kernel = StudioKernel()
    for line in sys.stdin:
        request: dict[str, Any] = {}
        try:
            request = json.loads(line)
            if request.get("type") != "execute":
                raise ValueError("unsupported kernel request")
            response = kernel.execute(str(request["request_id"]), str(request.get("code", "")))
        except BaseException as exception:  # noqa: BLE001 - keep protocol alive for malformed requests
            response = {
                "request_id": str(request.get("request_id", "")),
                "execution_count": kernel.execution_count,
                "stdout": "",
                "stderr": "",
                "result": None,
                "error": f"{type(exception).__name__}: {exception}",
                "traceback": traceback.format_exception(type(exception), exception, exception.__traceback__),
                "variables": [],
                "duration_ms": 0,
            }
        sys.stdout.write(json.dumps(response, ensure_ascii=False) + "\n")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
