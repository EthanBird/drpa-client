from __future__ import annotations

import ast
import contextlib
import io
import json
import os
import queue
import subprocess
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


class JupyterKernelBridge:
    """JSONL bridge backed by the real Jupyter/IPython wire protocol."""

    def __init__(self) -> None:
        from jupyter_client import KernelManager

        self.manager = KernelManager(kernel_name="python3")
        launch_options: dict[str, Any] = {"cwd": os.getcwd(), "env": os.environ.copy()}
        if os.name == "nt":
            launch_options["creationflags"] = subprocess.CREATE_NO_WINDOW
        self.manager.start_kernel(**launch_options)
        self.client = self.manager.client()
        self.client.start_channels()
        self.client.wait_for_ready(timeout=45)

    def close(self) -> None:
        try:
            self.client.stop_channels()
        finally:
            self.manager.shutdown_kernel(now=True)

    def execute(self, request_id: str, source: str) -> dict[str, Any]:
        started = time.perf_counter()
        variable_expression = (
            "{name: (type(value).__name__, repr(value)[:240]) "
            "for name, value in globals().items() "
            "if not name.startswith('_') and not callable(value) "
            "and not hasattr(value, '__spec__')}"
        )
        message_id = self.client.execute(
            source,
            allow_stdin=False,
            store_history=True,
            stop_on_error=False,
            user_expressions={"drpa_variables": variable_expression},
        )
        outputs: list[dict[str, Any]] = []
        stdout: list[str] = []
        stderr: list[str] = []
        result: str | None = None
        error: str | None = None
        trace: list[str] = []
        execution_count = 0

        while True:
            try:
                message = self.client.get_iopub_msg(timeout=1)
            except queue.Empty:
                if not self.manager.is_alive():
                    raise RuntimeError("Jupyter Kernel 已意外退出")
                continue
            if message.get("parent_header", {}).get("msg_id") != message_id:
                continue
            message_type = message.get("msg_type")
            content = message.get("content", {})
            if message_type == "stream":
                text = str(content.get("text", ""))
                name = str(content.get("name", "stdout"))
                outputs.append({"output_type": "stream", "name": name, "text": text})
                (stderr if name == "stderr" else stdout).append(text)
            elif message_type in {"execute_result", "display_data"}:
                output = {
                    "output_type": message_type,
                    "data": content.get("data", {}),
                    "metadata": content.get("metadata", {}),
                }
                if message_type == "execute_result":
                    execution_count = int(content.get("execution_count") or 0)
                    output["execution_count"] = execution_count
                    plain = output["data"].get("text/plain")
                    if isinstance(plain, str):
                        result = plain
                outputs.append(output)
            elif message_type == "error":
                error = f"{content.get('ename', 'Error')}: {content.get('evalue', '')}".rstrip()
                trace = [str(line) for line in content.get("traceback", [])]
                outputs.append(
                    {
                        "output_type": "error",
                        "ename": str(content.get("ename", "Error")),
                        "evalue": str(content.get("evalue", "")),
                        "traceback": trace,
                    }
                )
            elif message_type == "status" and content.get("execution_state") == "idle":
                break

        shell = self._shell_reply(message_id)
        shell_content = shell.get("content", {})
        execution_count = int(shell_content.get("execution_count") or execution_count)
        variables = _decode_variables(shell_content.get("user_expressions", {}))
        if shell_content.get("status") == "error" and error is None:
            error = f"{shell_content.get('ename', 'Error')}: {shell_content.get('evalue', '')}".rstrip()
            trace = [str(line) for line in shell_content.get("traceback", [])]
        return {
            "request_id": request_id,
            "execution_count": execution_count,
            "stdout": "".join(stdout),
            "stderr": "".join(stderr),
            "result": result,
            "error": error,
            "traceback": trace,
            "outputs": outputs,
            "variables": variables,
            "duration_ms": round((time.perf_counter() - started) * 1000),
        }

    def _shell_reply(self, message_id: str) -> dict[str, Any]:
        while True:
            message = self.client.get_shell_msg(timeout=30)
            if message.get("parent_header", {}).get("msg_id") == message_id:
                return message


def _safe_preview(value: Any, *, limit: int) -> str:
    try:
        preview = repr(value)
    except BaseException:  # noqa: BLE001 - repr hooks are user code
        preview = f"<{type(value).__name__}>"
    return preview if len(preview) <= limit else preview[: limit - 1] + "…"


def _decode_variables(user_expressions: dict[str, Any]) -> list[dict[str, str]]:
    response = user_expressions.get("drpa_variables", {})
    text = response.get("data", {}).get("text/plain") if response.get("status") == "ok" else None
    if not isinstance(text, str):
        return []
    try:
        values = ast.literal_eval(text)
    except (SyntaxError, ValueError):
        return []
    if not isinstance(values, dict):
        return []
    variables = []
    for name, value in sorted(values.items()):
        if isinstance(name, str) and isinstance(value, tuple) and len(value) == 2:
            variables.append({"name": name, "type_name": str(value[0]), "preview": str(value[1])})
        if len(variables) >= 100:
            break
    return variables


def main() -> int:
    kernel = JupyterKernelBridge()
    try:
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
                    "execution_count": 0,
                    "stdout": "",
                    "stderr": "",
                    "result": None,
                    "error": f"{type(exception).__name__}: {exception}",
                    "traceback": traceback.format_exception(type(exception), exception, exception.__traceback__),
                    "outputs": [],
                    "variables": [],
                    "duration_ms": 0,
                }
            sys.stdout.write(json.dumps(response, ensure_ascii=False) + "\n")
            sys.stdout.flush()
    finally:
        kernel.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
