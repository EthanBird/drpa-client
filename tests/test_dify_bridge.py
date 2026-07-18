from __future__ import annotations

import importlib.util
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
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


def test_conversation_mapping_is_persisted(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    path = tmp_path / "conversations.json"
    monkeypatch.setattr(dify_bridge, "CONVERSATIONS_PATH", path)
    dify_bridge.CONVERSATIONS.clear()

    dify_bridge.remember_conversation("drpa-session", {"conversation_id": "conversation-1"})

    assert json.loads(path.read_text(encoding="utf-8")) == {
        "drpa-session": "conversation-1",
    }
    assert dify_bridge.load_conversations() == {
        "drpa-session": "conversation-1",
    }


@pytest.mark.parametrize("app_type", ["completion", "workflow"])
def test_non_chat_apps_map_query_to_configured_input(app_type: str) -> None:
    config = {"app_type": app_type, "input_key": "question"}

    payload = dify_bridge.dify_payload(config, "session", "hello", False)

    assert payload["inputs"] == {"question": "hello"}
    assert "query" not in payload


def test_provider_connection_uses_dify_parameters_endpoint() -> None:
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            assert self.path == "/v1/parameters"
            assert self.headers["Authorization"] == "Bearer app-token"
            body = json.dumps({"opening_statement": "hello", "user_input_form": [{"text-input": {}}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *_args: object) -> None:
            return

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        result = dify_bridge.test_dify_connection({
            "base_url": f"http://127.0.0.1:{server.server_port}/v1",
            "api_key": "app-token",
            "timeout_seconds": 5,
        })
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)

    assert result["ok"] is True
    assert result["details"]["input_fields"] == 1
