from __future__ import annotations

import io
import json
from pathlib import Path

import pytest

from drpa_runner.events import EventWriter
from drpa_runner.executor import ExecutionRequest, execute_request
from drpa_runner.kernel import StudioKernel
from drpa_runner.paths import PathPolicyError, resolve_child


def test_resolve_child_rejects_escape(tmp_path: Path) -> None:
    with pytest.raises(PathPolicyError):
        resolve_child(tmp_path, "../outside.txt")


def test_event_writer_keeps_jsonl_ascii_safe() -> None:
    stream = io.StringIO()
    EventWriter(stream=stream).emit("log", message="中文日志")

    line = stream.getvalue()
    assert line.isascii()
    assert json.loads(line)["message"] == "中文日志"


def test_execute_request_emits_versioned_events(tmp_path: Path) -> None:
    package_dir = tmp_path / "package"
    output_dir = tmp_path / "output"
    package_dir.mkdir()
    (package_dir / "main.py").write_text(
        "def main(ctx):\n"
        "    ctx.log.info('started safely')\n"
        "    result = ctx.output_file('nested/result.txt')\n"
        "    result.write_text('ok', encoding='utf-8')\n"
        "    ctx.progress(100, 'done')\n",
        encoding="utf-8",
    )
    stream = io.StringIO()
    request = ExecutionRequest(
        protocol=1,
        run_id="test-run",
        package_id="com.example.test",
        package_dir=package_dir,
        output_dir=output_dir,
        entrypoint="main.py",
        callable="main",
        parameters={},
    )

    exit_code = execute_request(request, EventWriter(stream=stream))
    events = [json.loads(line) for line in stream.getvalue().splitlines()]

    assert exit_code == 0
    assert [event["type"] for event in events] == ["ready", "log", "artifact", "progress", "completed"]
    assert (output_dir / "nested" / "result.txt").read_text(encoding="utf-8") == "ok"
    assert [event["sequence"] for event in events] == sorted(event["sequence"] for event in events)


def test_execute_request_rejects_entrypoint_escape(tmp_path: Path) -> None:
    outside = tmp_path / "outside.py"
    outside.write_text("def main(ctx): pass", encoding="utf-8")
    package_dir = tmp_path / "package"
    package_dir.mkdir()
    request = ExecutionRequest(
        protocol=1,
        run_id="escape-run",
        package_id="com.example.escape",
        package_dir=package_dir,
        output_dir=tmp_path / "output",
        entrypoint="../outside.py",
        callable="main",
        parameters={},
    )

    with pytest.raises(PathPolicyError):
        execute_request(request, EventWriter(stream=io.StringIO()))


def test_studio_kernel_preserves_state_and_reports_variables() -> None:
    kernel = StudioKernel()
    first = kernel.execute("one", "value = 40\nprint('ready')")
    second = kernel.execute("two", "value + 2")

    assert first["stdout"] == "ready\n"
    assert second["result"] == "42"
    assert second["execution_count"] == 2
    assert any(variable["name"] == "value" for variable in second["variables"])
