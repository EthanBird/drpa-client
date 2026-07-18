from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "plugins/builtin/dify-loves-hermes/service/dify_bridge.py"
SPEC = importlib.util.spec_from_file_location("dify_bridge", MODULE_PATH)
assert SPEC and SPEC.loader
dify_bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dify_bridge)


TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "rpaz_validate",
            "description": "validate",
            "parameters": {"type": "object", "properties": {}},
        },
    }
]


def test_tool_bridge_converts_structured_dify_decision_to_openai_tool_calls() -> None:
    envelope = dify_bridge.parse_tool_envelope(
        '{"type":"tool_calls","calls":[{"name":"rpaz_validate","arguments":{}}]}',
        TOOLS,
    )

    assert envelope["type"] == "tool_calls"
    assert envelope["calls"][0]["type"] == "function"
    assert envelope["calls"][0]["function"] == {
        "name": "rpaz_validate",
        "arguments": "{}",
    }


def test_tool_bridge_rejects_unknown_tools_and_keeps_plain_final_text() -> None:
    with pytest.raises(RuntimeError, match="未注册工具"):
        dify_bridge.parse_tool_envelope(
            '{"type":"tool_calls","calls":[{"name":"unknown","arguments":{}}]}',
            TOOLS,
        )

    assert dify_bridge.parse_tool_envelope("普通回答", TOOLS) == {
        "type": "final",
        "content": "普通回答",
    }


def test_tool_bridge_validates_required_tool_arguments() -> None:
    tools = [
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "read a workspace file",
                "parameters": {
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                    "additionalProperties": False,
                },
            },
        }
    ]

    with pytest.raises(RuntimeError, match="缺少必填字段"):
        dify_bridge.parse_tool_envelope(
            '{"type":"tool_calls","calls":[{"name":"read_file","arguments":{}}]}',
            tools,
        )
