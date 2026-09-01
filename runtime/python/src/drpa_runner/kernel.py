from __future__ import annotations

import ast
import contextlib
import io
import json
import os
import queue
import inspect as pyinspect
import keyword
import rlcompleter
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

    def close(self) -> None:
        return None

    def complete(self, request_id: str, source: str, cursor_pos: int) -> dict[str, Any]:
        cursor_pos = max(0, min(int(cursor_pos), len(source)))
        start = cursor_pos
        while start > 0 and (source[start - 1].isalnum() or source[start - 1] in "._"):
            start -= 1
        token = source[start:cursor_pos]
        completer = rlcompleter.Completer(self.namespace)
        matches: list[str] = []
        index = 0
        while len(matches) < 100:
            match = completer.complete(token, index)
            if match is None:
                break
            if match not in matches:
                matches.append(match)
            index += 1
        if "." not in token:
            matches.extend(word for word in keyword.kwlist if word.startswith(token) and word not in matches)
        return {
            "request_id": request_id,
            "matches": matches,
            "cursor_start": start,
            "cursor_end": cursor_pos,
            "metadata": {"runtime": "stdlib"},
            "status": "ok",
        }

    def inspect(self, request_id: str, source: str, cursor_pos: int, detail_level: int = 0) -> dict[str, Any]:
        del detail_level
        cursor_pos = max(0, min(int(cursor_pos), len(source)))
        start = cursor_pos
        while start > 0 and (source[start - 1].isalnum() or source[start - 1] in "._"):
            start -= 1
        expression = source[start:cursor_pos]
        value: Any = None
        found = False
        if expression:
            try:
                value = eval(expression, self.namespace, self.namespace)
                found = True
            except BaseException:
                pass
        documentation = pyinspect.getdoc(value) if found else None
        return {
            "request_id": request_id,
            "found": found,
            "data": {"text/plain": documentation or _safe_preview(value, limit=20_000)} if found else {},
            "metadata": {"runtime": "stdlib"},
            "status": "ok",
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

    def complete(self, request_id: str, source: str, cursor_pos: int) -> dict[str, Any]:
        """Complete against the live IPython namespace using the Jupyter protocol."""

        cursor_pos = max(0, min(int(cursor_pos), len(source)))
        message_id = self.client.complete(source, cursor_pos=cursor_pos)
        reply = self._shell_reply(message_id)
        content = reply.get("content", {})
        matches = content.get("matches", []) if content.get("status") == "ok" else []
        return {
            "request_id": request_id,
            "matches": [str(match) for match in matches],
            "cursor_start": int(content.get("cursor_start", cursor_pos)),
            "cursor_end": int(content.get("cursor_end", cursor_pos)),
            "metadata": content.get("metadata", {}),
            "status": str(content.get("status", "error")),
        }

    def inspect(self, request_id: str, source: str, cursor_pos: int, detail_level: int = 0) -> dict[str, Any]:
        """Return hover/signature documentation from the live IPython namespace."""

        cursor_pos = max(0, min(int(cursor_pos), len(source)))
        detail_level = max(0, min(int(detail_level), 1))
        message_id = self.client.inspect(source, cursor_pos=cursor_pos, detail_level=detail_level)
        reply = self._shell_reply(message_id)
        content = reply.get("content", {})
        raw_data = content.get("data", {}) if content.get("status") == "ok" else {}
        data = {
            str(key): str(value)[:100_000]
            for key, value in raw_data.items()
            if isinstance(key, str) and isinstance(value, (str, int, float, bool))
        }
        return {
            "request_id": request_id,
            "found": bool(content.get("found", False)),
            "data": data,
            "metadata": content.get("metadata", {}),
            "status": str(content.get("status", "error")),
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
    try:
        kernel: StudioKernel | JupyterKernelBridge = JupyterKernelBridge()
    except (ImportError, ModuleNotFoundError):
        kernel = StudioKernel()
    try:
        for line in sys.stdin:
            request: dict[str, Any] = {}
            request_type: str | None = None
            try:
                request = json.loads(line)
                request_type = request.get("type")
                if request_type == "execute":
                    response = kernel.execute(str(request["request_id"]), str(request.get("code", "")))
                elif request_type == "complete":
                    response = kernel.complete(
                        str(request["request_id"]),
                        str(request.get("code", "")),
                        int(request.get("cursor_pos", 0)),
                    )
                elif request_type == "inspect":
                    response = kernel.inspect(
                        str(request["request_id"]),
                        str(request.get("code", "")),
                        int(request.get("cursor_pos", 0)),
                        int(request.get("detail_level", 0)),
                    )
                else:
                    raise ValueError("unsupported kernel request")
            except BaseException as exception:  # noqa: BLE001 - keep protocol alive for malformed requests
                error = f"{type(exception).__name__}: {exception}"
                if request_type == "complete":
                    cursor_pos = int(request.get("cursor_pos", 0))
                    response = {
                        "request_id": str(request.get("request_id", "")),
                        "matches": [],
                        "cursor_start": cursor_pos,
                        "cursor_end": cursor_pos,
                        "metadata": {"error": error},
                        "status": "error",
                    }
                elif request_type == "inspect":
                    response = {
                        "request_id": str(request.get("request_id", "")),
                        "found": False,
                        "data": {},
                        "metadata": {"error": error},
                        "status": "error",
                    }
                else:
                    response = {
                        "request_id": str(request.get("request_id", "")),
                        "execution_count": 0,
                        "stdout": "",
                        "stderr": "",
                        "result": None,
                        "error": error,
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
