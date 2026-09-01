from __future__ import annotations

from pathlib import Path

from drpa_runner.jcode_tool_guard import BLOCK_MARKER, process_event


def pre(root: Path, tool: str, payload: str = "{}") -> tuple[int, str]:
    return process_event("pre_tool", "session-one", tool, payload, state_root=root)


def post(root: Path, tool: str, status: str, error: str = "") -> tuple[int, str]:
    return process_event(
        "post_tool",
        "session-one",
        tool,
        status=status,
        error=error,
        state_root=root,
    )


def test_successful_empty_output_command_is_not_run_twice(tmp_path: Path) -> None:
    command = '{"command":"python -c \\"pass\\"","timeout":30000}'

    assert pre(tmp_path, "bash", command) == (0, "")
    assert post(tmp_path, "bash", "ok") == (0, "")
    exit_code, message = pre(tmp_path, "bash", command)

    assert exit_code == 2
    assert BLOCK_MARKER in message
    assert "empty stdout is success" in message


def test_read_only_calls_do_not_make_a_duplicate_command_fresh(tmp_path: Path) -> None:
    command = '{"command":"git status --porcelain"}'
    assert pre(tmp_path, "bash", command) == (0, "")
    assert post(tmp_path, "bash", "ok") == (0, "")
    assert pre(tmp_path, "read", '{"file_path":"README.md"}') == (0, "")

    assert pre(tmp_path, "bash", command)[0] == 2


def test_state_change_or_new_turn_allows_the_command_again(tmp_path: Path) -> None:
    command = '{"command":"npm test"}'
    assert pre(tmp_path, "bash", command) == (0, "")
    assert post(tmp_path, "bash", "ok") == (0, "")
    assert pre(tmp_path, "edit", '{"file_path":"main.py"}') == (0, "")
    assert pre(tmp_path, "bash", command) == (0, "")

    assert process_event("turn_end", "session-one", state_root=tmp_path) == (0, "")
    assert pre(tmp_path, "bash", command) == (0, "")


def test_failed_command_may_be_retried_but_guard_block_does_not_mark_failure(tmp_path: Path) -> None:
    command = '{"command":"flaky-test"}'
    assert pre(tmp_path, "bash", command) == (0, "")
    assert post(tmp_path, "bash", "error", "exit 1") == (0, "")
    assert pre(tmp_path, "bash", command) == (0, "")
    assert post(tmp_path, "bash", "ok") == (0, "")

    blocked = pre(tmp_path, "bash", command)
    assert blocked[0] == 2
    assert post(tmp_path, "bash", "error", blocked[1]) == (0, "")
    assert pre(tmp_path, "bash", command)[0] == 2


def test_argument_key_order_has_one_fingerprint(tmp_path: Path) -> None:
    first = '{"command":"echo ok","timeout":1000}'
    reordered = '{"timeout":1000,"command":"echo ok"}'
    assert pre(tmp_path, "bash", first) == (0, "")
    assert post(tmp_path, "bash", "ok") == (0, "")

    assert pre(tmp_path, "bash", reordered)[0] == 2
