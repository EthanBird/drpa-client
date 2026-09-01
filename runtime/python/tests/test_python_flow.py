from __future__ import annotations

import ast
import json
from pathlib import Path
import subprocess
import sys

import pytest

from drpa_runner.python_flow import (
    PythonFlowSyntaxError,
    PythonFlowValidationError,
    flow_from_json,
    flow_to_json,
    flow_to_python,
    handle_json_request,
    python_to_flow,
    validate_flow,
)


def _types(flow) -> list[str]:
    return [node.type for node in flow.nodes]


def _node(flow, node_type: str):
    return next(node for node in flow.nodes if node.type == node_type)


def test_python_to_flow_preserves_order_spans_stable_ids_and_source() -> None:
    source = (
        "def main(ctx):\n"
        "    count = 1\n"
        "    print(count)\n"
        "    return count\n"
    )

    first = python_to_flow(source)
    second = python_to_flow(source)

    assert first.schema_version == 1
    assert first.kind == "drpa.python-flow"
    assert _types(first) == ["start", "assign", "call", "return", "end"]
    assert [node.id for node in first.nodes] == [node.id for node in second.nodes]
    assert [node.code for node in first.nodes[1:-1]] == [
        "count = 1",
        "print(count)",
        "return count",
    ]
    assert first.nodes[1].span.to_dict() == {
        "startLine": 2,
        "startColumn": 4,
        "endLine": 2,
        "endColumn": 13,
    }
    assert [(edge.source, edge.target, edge.kind) for edge in first.edges] == [
        (first.nodes[0].id, first.nodes[1].id, "next"),
        (first.nodes[1].id, first.nodes[2].id, "next"),
        (first.nodes[2].id, first.nodes[3].id, "next"),
        (first.nodes[3].id, first.nodes[4].id, "return"),
    ]


def test_branches_loops_and_try_have_structured_children_and_edges() -> None:
    source = """def main(ctx):
    if ctx.params.get("enabled"):
        ctx.progress(10, "start")
    else:
        ctx.log.info("skip")
    for item in ctx.params.get("items", []):
        print(item)
    while ctx.params.get("retry"):
        ctx.progress(50)
    try:
        value = compute()
    except ValueError as exc:
        ctx.log.error(str(exc))
    finally:
        ctx.progress(100)
    return value
"""

    flow = python_to_flow(source)

    assert {"if", "for", "while", "try"}.issubset(_types(flow))
    if_node = _node(flow, "if")
    assert if_node.data["test"] == 'ctx.params.get("enabled")'
    assert len(if_node.data["body"]) == 1
    assert len(if_node.data["orelse"]) == 1
    assert {edge.kind for edge in flow.edges if edge.source == if_node.id} == {
        "true",
        "false",
    }

    for_node = _node(flow, "for")
    assert for_node.data["target"] == "item"
    assert for_node.data["iterator"] == 'ctx.params.get("items", [])'
    assert any(edge.source == for_node.id and edge.kind == "body" for edge in flow.edges)
    assert any(edge.source == for_node.id and edge.kind == "exit" for edge in flow.edges)

    try_node = _node(flow, "try")
    assert try_node.data["handlers"][0]["type"] == "ValueError"
    assert try_node.data["handlers"][0]["name"] == "exc"
    assert len(try_node.data["finalbody"]) == 1
    validate_flow(flow)


def test_ctx_rpa_and_generic_calls_are_distinguished_with_aliases() -> None:
    source = """import rpa as r

def main(ctx):
    page = ctx.browser()
    ctx.sql.execute("select 1")
    r.init()
    r.click("login")
    notify("done")
"""

    flow = python_to_flow(source)

    calls = [node for node in flow.nodes if node.type.endswith("call")]
    assert [(node.type, node.data["callName"]) for node in calls] == [
        ("ctx-call", "ctx.browser"),
        ("ctx-call", "ctx.sql.execute"),
        ("rpa-call", "r.init"),
        ("rpa-call", "r.click"),
        ("call", "notify"),
    ]
    assert calls[0].data["assignTargets"] == ["page"]


def test_raw_code_preserves_unstructured_statements_and_round_trip_is_valid() -> None:
    source = """from contextlib import nullcontext

def main(ctx):
    with nullcontext() as value:
        ctx.log.info("inside")
    match ctx.params.get("kind"):
        case "one":
            result = 1
        case _:
            result = 0
    return result
"""

    flow = python_to_flow(source)
    raw_nodes = [node for node in flow.nodes if node.type == "raw-code"]

    assert len(raw_nodes) == 2
    assert raw_nodes[0].code.startswith("with nullcontext()")
    assert raw_nodes[1].code.startswith('match ctx.params.get("kind")')
    regenerated = flow_to_python(flow)
    ast.parse(regenerated)
    assert "with nullcontext() as value:" in regenerated
    assert 'case "one":' in regenerated


def test_round_trip_preserves_comments_and_blank_lines_between_statements() -> None:
    source = """def main(ctx):
    first = 1

    # keep this AI-facing explanation
    second = first + 1
    return second
"""

    flow = python_to_flow(source)
    regenerated = flow_to_python(flow)

    assert "\n\n    # keep this AI-facing explanation\n" in regenerated
    ast.parse(regenerated)


def test_generated_nodes_can_be_edited_and_written_back() -> None:
    flow = python_to_flow(
        "def main(ctx):\n"
        "    count = 1\n"
        "    return count\n"
    )
    assignment = _node(flow, "assign")
    assignment.code = "count = 2"
    assignment.data["statement"] = "count = 2"

    regenerated = flow_to_python(flow)

    ast.parse(regenerated)
    assert "    count = 2\n" in regenerated
    assert "    return count\n" in regenerated


def test_schema_json_and_structured_control_flow_round_trip() -> None:
    source = """import rpa as r

def main(ctx):
    if ctx.params.get("ok"):
        r.click("go")
    else:
        ctx.log.info("skip")
    for item in [1, 2]:
        print(item)
    try:
        value = work()
    except ValueError:
        value = 0
    finally:
        ctx.progress(100)
    return value
"""

    restored = flow_from_json(flow_to_json(python_to_flow(source)))
    regenerated = flow_to_python(restored)

    ast.parse(regenerated)
    assert "if ctx.params.get(\"ok\"):" in regenerated
    assert "for item in [1, 2]:" in regenerated
    assert "except ValueError:" in regenerated
    assert "finally:" in regenerated


def test_one_line_entrypoint_and_multiline_unicode_value_keep_ast_semantics() -> None:
    one_line = flow_to_python(python_to_flow("def main(ctx): return 1\n"))
    ast.parse(one_line)
    assert one_line == "def main(ctx):\n    return 1\n"

    source = '''def main(ctx):
    message = """中文
    保留缩进
    """
    return message
'''
    regenerated = flow_to_python(python_to_flow(source))

    original_ast = ast.dump(ast.parse(source), include_attributes=False)
    regenerated_ast = ast.dump(ast.parse(regenerated), include_attributes=False)
    assert regenerated_ast == original_ast


def test_decorated_entrypoint_keeps_the_complete_function_header() -> None:
    source = """def traced(function):
    return function

@traced
def main(
    ctx,
):
    return ctx.params
"""

    regenerated = flow_to_python(python_to_flow(source))

    ast.parse(regenerated)
    assert "@traced\ndef main(\n    ctx,\n):" in regenerated


def test_syntax_error_reports_source_location() -> None:
    with pytest.raises(PythonFlowSyntaxError) as exc_info:
        python_to_flow("def main(ctx):\n    if True print('bad')\n")

    assert exc_info.value.line == 2
    assert exc_info.value.column is not None
    assert "main.py:2" in str(exc_info.value)


def test_validation_rejects_dangling_edges_and_unknown_node_types() -> None:
    flow = python_to_flow("def main(ctx):\n    return 1\n")
    flow.edges[0].target = "missing-node"

    with pytest.raises(PythonFlowValidationError, match="missing-node"):
        validate_flow(flow)

    flow = python_to_flow("def main(ctx):\n    return 1\n")
    flow.nodes[1].type = "shell"
    with pytest.raises(PythonFlowValidationError, match="shell"):
        validate_flow(flow)


def test_inline_conversion_never_executes_code_and_rejects_path_inputs(tmp_path: Path) -> None:
    marker = tmp_path / "executed.txt"
    source = (
        "def main(ctx):\n"
        f"    __import__('pathlib').Path({str(marker)!r}).write_text('bad')\n"
    )

    flow = python_to_flow(source)

    assert not marker.exists()
    assert "call" in _types(flow)
    with pytest.raises(ValueError, match="source_name"):
        python_to_flow(source, source_name="../main.py")

    response = json.loads(
        handle_json_request(
            json.dumps({"operation": "python-to-flow", "path": str(marker)})
        )
    )
    assert response["ok"] is False
    assert "inline source" in response["error"]
    assert not marker.exists()


def test_cli_json_helper_converts_validates_and_writes_python() -> None:
    source = "def main(ctx):\n    ctx.progress(100)\n"
    converted = json.loads(
        handle_json_request(
            json.dumps(
                {
                    "operation": "python-to-flow",
                    "source": source,
                    "sourceName": "main.py",
                }
            )
        )
    )

    assert converted["ok"] is True
    assert converted["flow"]["schemaVersion"] == 1
    checked = json.loads(
        handle_json_request(
            json.dumps({"operation": "validate-flow", "flow": converted["flow"]})
        )
    )
    assert checked == {"ok": True, "valid": True}

    generated = json.loads(
        handle_json_request(
            json.dumps({"operation": "flow-to-python", "flow": converted["flow"]})
        )
    )
    assert generated["ok"] is True
    ast.parse(generated["source"])


def test_module_cli_serves_one_stdin_request() -> None:
    request = json.dumps(
        {
            "operation": "python-to-flow",
            "source": "def main(ctx):\n    ctx.progress(100)\n",
            "sourceName": "main.py",
        }
    )

    completed = subprocess.run(
        [sys.executable, "-m", "drpa_runner.python_flow"],
        input=request,
        text=True,
        capture_output=True,
        check=False,
        timeout=10,
    )

    assert completed.returncode == 0, completed.stderr
    response = json.loads(completed.stdout)
    assert response["ok"] is True
    assert response["flow"]["kind"] == "drpa.python-flow"
