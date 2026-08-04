from __future__ import annotations

import io
import json
import sys
import types
from pathlib import Path

import pytest

from drpa_runner.events import EventWriter
from drpa_runner.executor import ExecutionRequest, execute_request
from drpa_runner.context import RuntimeContext
from drpa_runner.kernel import StudioKernel
from drpa_runner.paths import PathPolicyError, resolve_child
from drpa_runner.sql import SqlClient


class _FakeChromiumOptions:
    created: list["_FakeChromiumOptions"] = []

    def __init__(self) -> None:
        self.local_port: int | None = None
        self.user_data_path = ""
        self.headless_enabled = False
        type(self).created.append(self)

    def set_browser_path(self, _path: str):
        return self

    def set_local_port(self, port: int):
        self.local_port = port
        return self

    def set_user_data_path(self, path: str):
        self.user_data_path = path
        return self

    def headless(self, enabled: bool):
        self.headless_enabled = enabled
        return self

    def set_download_path(self, _path: str):
        return self


class _FakeChromiumPage:
    quit_calls = 0

    def __init__(self, _options: _FakeChromiumOptions) -> None:
        self.set = self
        self.download_path_value = ""

    def download_path(self, path: str) -> None:
        self.download_path_value = path

    def quit(self, *_args, **_kwargs) -> None:
        type(self).quit_calls += 1


@pytest.fixture
def fake_drission_page(monkeypatch: pytest.MonkeyPatch):
    _FakeChromiumPage.quit_calls = 0
    _FakeChromiumOptions.created = []
    module = types.ModuleType("DrissionPage")
    module.ChromiumOptions = _FakeChromiumOptions
    module.ChromiumPage = _FakeChromiumPage
    monkeypatch.setitem(sys.modules, "DrissionPage", module)
    return _FakeChromiumPage


def test_browser_uses_distinct_persistent_visible_and_headless_pools(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    fake_drission_page: type[_FakeChromiumPage],
) -> None:
    profile_root = tmp_path / "browser-profiles"
    monkeypatch.setenv("DRPA_BROWSER_PORT", "19422")
    monkeypatch.setenv("DRPA_BROWSER_PROFILE_ROOT", str(profile_root))
    context = RuntimeContext(
        run_id="browser-pools",
        package_id="com.example.browser-pools",
        params={},
        package_dir=tmp_path,
        output_dir=tmp_path / "output",
        database_path=tmp_path / "runtime.sqlite3",
        events=EventWriter(stream=io.StringIO()),
    )

    visible_page = context.browser(headless=False)
    assert context.browser(headless=False) is visible_page
    context.browser(headless=True)
    context.sql.close()

    visible, headless = _FakeChromiumOptions.created
    assert visible.local_port == 19422
    assert visible.user_data_path == str((profile_root / "visible").resolve())
    assert visible.headless_enabled is False
    assert headless.local_port == 19423
    assert headless.user_data_path == str((profile_root / "headless").resolve())
    assert headless.headless_enabled is True
    assert fake_drission_page.quit_calls == 0


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
        "    ctx.sql.execute('CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY)')\n"
        "    ctx.sql.execute('INSERT INTO runs(id) VALUES (?)', (ctx.run_id,))\n"
        "    ctx.progress(100, 'done')\n"
        "    ctx.open_output_directory()\n",
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
        database_path=tmp_path / "databases" / "workspace.sqlite3",
    )

    exit_code = execute_request(request, EventWriter(stream=stream))
    events = [json.loads(line) for line in stream.getvalue().splitlines()]

    assert exit_code == 0
    assert [event["type"] for event in events] == ["ready", "log", "artifact", "progress", "open_directory", "completed"]
    assert events[-2]["path"] == str(output_dir.resolve())
    assert (output_dir / "nested" / "result.txt").read_text(encoding="utf-8") == "ok"
    with SqlClient(tmp_path / "databases" / "workspace.sqlite3") as sql:
        assert sql.scalar("SELECT id FROM runs") == "test-run"
    assert [event["sequence"] for event in events] == sorted(event["sequence"] for event in events)


@pytest.mark.parametrize("should_fail", [False, True])
def test_run_never_closes_shared_browser(
    tmp_path: Path,
    fake_drission_page: type[_FakeChromiumPage],
    should_fail: bool,
) -> None:
    package_dir = tmp_path / "package"
    package_dir.mkdir()
    failure = "    raise RuntimeError('selector failed')\n" if should_fail else ""
    (package_dir / "main.py").write_text(
        "def main(ctx):\n"
        "    page = ctx.browser()\n"
        "    page.quit()\n"
        f"{failure}",
        encoding="utf-8",
    )
    stream = io.StringIO()
    request = ExecutionRequest(
        protocol=1,
        run_id="browser-persistent-run",
        package_id="com.example.browser-persistent",
        package_dir=package_dir,
        output_dir=tmp_path / "output",
        entrypoint="main.py",
        callable="main",
        parameters={},
    )

    exit_code = execute_request(request, EventWriter(stream=stream))
    events = [json.loads(line) for line in stream.getvalue().splitlines()]

    assert exit_code == (1 if should_fail else 0)
    assert fake_drission_page.quit_calls == 0
    assert any("复用" in event.get("message", "") for event in events)


def test_nested_package_browser_is_preserved_by_parent_run(
    tmp_path: Path,
    fake_drission_page: type[_FakeChromiumPage],
) -> None:
    parent = tmp_path / "parent"
    child = tmp_path / "child"
    parent.mkdir()
    child.mkdir()
    (parent / "main.py").write_text(
        "def main(ctx):\n"
        "    return ctx.invoke('com.example.child')\n",
        encoding="utf-8",
    )
    (child / "main.py").write_text(
        "def main(ctx):\n"
        "    page = ctx.browser()\n"
        "    page.quit()\n"
        "    return 'ok'\n",
        encoding="utf-8",
    )
    stream = io.StringIO()
    request = ExecutionRequest(
        protocol=1,
        run_id="nested-browser-run",
        package_id="com.example.parent",
        package_dir=parent,
        output_dir=tmp_path / "output",
        entrypoint="main.py",
        callable="main",
        parameters={},
        package_catalog={
            "com.example.child": {
                "package_dir": str(child),
                "entrypoint": "main.py",
                "callable": "main",
            }
        },
    )

    assert execute_request(request, EventWriter(stream=stream)) == 0
    events = [json.loads(line) for line in stream.getvalue().splitlines()]
    assert fake_drission_page.quit_calls == 0
    assert any("已保留 1 个" in event.get("message", "") for event in events)


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


def test_rpaz_package_can_invoke_an_installed_package(tmp_path: Path) -> None:
    parent = tmp_path / "parent"
    child = tmp_path / "child"
    parent.mkdir()
    child.mkdir()
    (parent / "main.py").write_text(
        "def main(ctx):\n"
        "    child = ctx.invoke('com.example.child', {'value': 21})\n"
        "    return {'answer': child['answer']}\n",
        encoding="utf-8",
    )
    (child / "main.py").write_text(
        "def main(ctx):\n"
        "    return {'answer': ctx.params['value'] * 2}\n",
        encoding="utf-8",
    )
    result_path = tmp_path / "result.json"
    request = ExecutionRequest(
        protocol=1,
        run_id="composition-run",
        package_id="com.example.parent",
        package_dir=parent,
        output_dir=tmp_path / "output",
        entrypoint="main.py",
        callable="main",
        parameters={},
        database_path=tmp_path / "databases" / "workspace.sqlite3",
        package_catalog={
            "com.example.child": {
                "package_dir": str(child),
                "entrypoint": "main.py",
                "callable": "main",
            }
        },
        result_path=result_path,
    )

    assert execute_request(request, EventWriter(stream=io.StringIO())) == 0
    assert json.loads(result_path.read_text(encoding="utf-8")) == {"answer": 42}


def test_studio_kernel_preserves_state_and_reports_variables() -> None:
    kernel = StudioKernel()
    first = kernel.execute("one", "value = 40\nprint('ready')")
    second = kernel.execute("two", "value + 2")

    assert first["stdout"] == "ready\n"
    assert second["result"] == "42"
    assert second["execution_count"] == 2
    assert any(variable["name"] == "value" for variable in second["variables"])


def test_sql_client_queries_and_rolls_back_transactions(tmp_path: Path) -> None:
    sql = SqlClient(tmp_path / "databases" / "workspace.sqlite3")
    sql.execute("CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT NOT NULL)")
    assert sql.executemany("INSERT INTO notes(title) VALUES (?)", [("one",), ("two",)]) == 2
    assert sql.scalar("SELECT COUNT(*) FROM notes") == 2
    assert sql.query("SELECT id, title FROM notes ORDER BY id") == [
        {"id": 1, "title": "one"},
        {"id": 2, "title": "two"},
    ]

    with pytest.raises(RuntimeError):
        with sql.transaction():
            sql.execute("INSERT INTO notes(title) VALUES (?)", ("rolled back",))
            raise RuntimeError("stop")

    assert sql.scalar("SELECT COUNT(*) FROM notes") == 2
    sql.close()
