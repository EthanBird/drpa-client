"""Per-turn duplicate command guard for the bundled JCode harness.

JCode invokes this module through lifecycle hooks.  A successful command with
empty stdout is still a completed execution; an identical command must not be
started again in the same turn unless another state-changing tool ran first.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
import sys
from pathlib import Path
from typing import Iterator


BLOCK_MARKER = "DRPA_DUPLICATE_COMMAND"
COMMAND_TOOLS = frozenset({"bash", "shell", "cmd", "powershell"})
READ_ONLY_TOOLS = frozenset(
    {
        "agentgrep",
        "bg",
        "discover",
        "find",
        "glob",
        "grep",
        "ls",
        "memory",
        "read",
        "todo",
        "webfetch",
        "websearch",
    }
)


def _safe_session_name(session_id: str) -> str:
    digest = hashlib.sha256(session_id.encode("utf-8", errors="replace")).hexdigest()
    return digest[:32]


def _canonical_input(source: str) -> str:
    try:
        parsed = json.loads(source or "{}")
    except json.JSONDecodeError:
        return source.strip()
    return json.dumps(parsed, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _fingerprint(tool_name: str, source: str) -> str:
    canonical = f"{tool_name}\0{_canonical_input(source)}"
    return hashlib.sha256(canonical.encode("utf-8", errors="replace")).hexdigest()


def _default_state_root() -> Path:
    configured = os.environ.get("DRPA_JCODE_TOOL_GUARD_STATE", "").strip()
    if configured:
        return Path(configured)
    home = os.environ.get("JCODE_HOME", "").strip()
    if home:
        return Path(home) / "tool-guard-state"
    return Path.cwd() / ".drpa-jcode-tool-guard"


@contextlib.contextmanager
def _locked(lock_path: Path) -> Iterator[None]:
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    with lock_path.open("a+b") as handle:
        handle.seek(0, os.SEEK_END)
        if handle.tell() == 0:
            handle.write(b"0")
            handle.flush()
        handle.seek(0)
        if os.name == "nt":
            import msvcrt

            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
            try:
                yield
            finally:
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl

            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def _load(path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return {"barrier": 0}
    return value if isinstance(value, dict) else {"barrier": 0}


def _save(path: Path, state: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(
        json.dumps(state, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
        encoding="utf-8",
    )
    temporary.replace(path)


def _integer(value: object, default: int = 0) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return default


def process_event(
    event: str,
    session_id: str,
    tool_name: str = "",
    tool_input: str = "",
    status: str = "",
    error: str = "",
    state_root: Path | None = None,
) -> tuple[int, str]:
    """Process one JCode hook event and return ``(exit_code, stderr)``."""

    root = state_root or _default_state_root()
    key = _safe_session_name(session_id or "unknown")
    state_path = root / f"{key}.json"
    lock_path = root / f"{key}.lock"
    normalized_tool = tool_name.strip().lower()

    with _locked(lock_path):
        if event in {"turn_end", "session_end"}:
            state_path.unlink(missing_ok=True)
            return 0, ""

        state = _load(state_path)
        barrier = _integer(state.get("barrier", 0))

        if event == "post_tool" and normalized_tool in COMMAND_TOOLS:
            if BLOCK_MARKER in error or "blocked by pre_tool hook" in error:
                return 0, ""
            last = state.get("last_command")
            if isinstance(last, dict) and last.get("status") == "pending":
                last["status"] = "completed" if status == "ok" else "failed"
                state["last_command"] = last
                _save(state_path, state)
            return 0, ""

        if event != "pre_tool":
            return 0, ""

        if normalized_tool not in COMMAND_TOOLS:
            if normalized_tool not in READ_ONLY_TOOLS:
                state["barrier"] = barrier + 1
                _save(state_path, state)
            return 0, ""

        fingerprint = _fingerprint(normalized_tool, tool_input)
        last = state.get("last_command")
        if (
            isinstance(last, dict)
            and last.get("fingerprint") == fingerprint
            and _integer(last.get("barrier"), -1) == barrier
            and last.get("status") in {"pending", "completed"}
        ):
            last["blocked_count"] = _integer(last.get("blocked_count")) + 1
            state["last_command"] = last
            _save(state_path, state)
            return (
                2,
                f"{BLOCK_MARKER}: identical command already completed or is still running in "
                "this turn. Reuse the previous result. Exit 0 with empty stdout is success, "
                "not a reason to run the command again.",
            )

        state["last_command"] = {
            "fingerprint": fingerprint,
            "barrier": barrier,
            "status": "pending",
            "blocked_count": 0,
        }
        _save(state_path, state)
        return 0, ""


def main() -> int:
    event = os.environ.get("JCODE_HOOK_EVENT", "").strip()
    tool_input = sys.stdin.read() if event == "pre_tool" else ""
    exit_code, message = process_event(
        event=event,
        session_id=os.environ.get("JCODE_HOOK_SESSION_ID", "unknown"),
        tool_name=os.environ.get("JCODE_HOOK_TOOL_NAME", ""),
        tool_input=tool_input,
        status=os.environ.get("JCODE_HOOK_STATUS", ""),
        error=os.environ.get("JCODE_HOOK_ERROR", ""),
    )
    if message:
        sys.stderr.write(message + "\n")
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
