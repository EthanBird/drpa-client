from __future__ import annotations

import json
import os
import re
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Iterable


PLUGIN_VERSION = "0.2.0"
STATE_PATH = Path(os.environ.get("DRPA_PLUGIN_CONFIG", "state.json"))
CONVERSATIONS_PATH = STATE_PATH.with_name("conversations.json")
CONVERSATION_LOCK = threading.Lock()


def load_conversations() -> dict[str, str]:
    try:
        value = json.loads(CONVERSATIONS_PATH.read_text(encoding="utf-8"))
        return {str(key): str(item) for key, item in value.items() if key and item}
    except (FileNotFoundError, json.JSONDecodeError, OSError, AttributeError):
        return {}


CONVERSATIONS: dict[str, str] = load_conversations()


def log(message: str) -> None:
    print(f"[dify-bridge] {message}", file=sys.stderr, flush=True)


def load_config() -> dict[str, Any]:
    state = json.loads(STATE_PATH.read_text(encoding="utf-8"))
    config = state.get("config", {})
    return {
        "base_url": str(config.get("base_url", "http://127.0.0.1:5001/v1")).rstrip("/"),
        "api_key": str(config.get("api_key", "")),
        "app_type": str(config.get("app_type", "chat")),
        "input_key": str(config.get("input_key", "query")),
        "model": str(config.get("model", "dify-app")),
        "port": int(config.get("port", 34121)),
        "user_prefix": str(config.get("user_prefix", "drpa")),
        "tool_bridge": bool(config.get("tool_bridge", True)),
        "timeout_seconds": max(5, min(600, int(config.get("timeout_seconds", 120)))),
    }


def compact_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def normalize_session(payload: dict[str, Any], config: dict[str, Any]) -> str:
    requested = str(payload.get("user") or payload.get("session_id") or "default")
    safe = re.sub(r"[^a-zA-Z0-9_.-]+", "-", requested).strip("-")[:96] or "default"
    return f"{config['user_prefix']}-{safe}"


def message_text(message: dict[str, Any]) -> str:
    content = message.get("content", "")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            str(item.get("text", ""))
            for item in content
            if isinstance(item, dict) and item.get("type") in {"text", "input_text"}
        )
    return str(content or "")


def transcript(messages: list[dict[str, Any]]) -> str:
    lines: list[str] = []
    for message in messages:
        role = str(message.get("role", "user"))
        text = message_text(message)
        if role == "assistant" and message.get("tool_calls"):
            text = f"{text}\nTOOL_CALLS={compact_json(message['tool_calls'])}".strip()
        if role == "tool":
            role = f"tool:{message.get('tool_call_id', '')}"
        lines.append(f"[{role}]\n{text}")
    return "\n\n".join(lines)


def tool_bridge_query(messages: list[dict[str, Any]], tools: list[dict[str, Any]]) -> str:
    functions = [tool.get("function", tool) for tool in tools]
    return f"""你是 DRPA 外部工具桥的决策模型。根据对话判断下一步。

可用工具 JSON Schema：
{json.dumps(functions, ensure_ascii=False, indent=2)}

严格只输出以下两种 JSON 对象之一，不要使用 Markdown 代码围栏：
1. 调用工具：{{"type":"tool_calls","calls":[{{"id":"call_xxx","name":"工具名称","arguments":{{}}}}]}}
2. 最终回答：{{"type":"final","content":"给用户的完整回答"}}

工具名称必须来自可用工具列表，arguments 必须符合对应 JSON Schema。看到 tool 角色结果后继续推理，不要重复已经成功的调用。

对话：
{transcript(messages)}"""


def plain_query(messages: list[dict[str, Any]]) -> str:
    if len(messages) == 1 and messages[0].get("role") == "user":
        return message_text(messages[0])
    return transcript(messages)


def dify_endpoint(config: dict[str, Any]) -> str:
    app_type = config["app_type"]
    if app_type == "completion":
        return f"{config['base_url']}/completion-messages"
    if app_type == "workflow":
        return f"{config['base_url']}/workflows/run"
    return f"{config['base_url']}/chat-messages"


def dify_payload(
    config: dict[str, Any],
    session: str,
    query: str,
    streaming: bool,
) -> dict[str, Any]:
    body: dict[str, Any] = {
        "inputs": {},
        "response_mode": "streaming" if streaming else "blocking",
        "user": session,
    }
    if config["app_type"] in {"completion", "workflow"}:
        body["inputs"] = {config["input_key"]: query}
    else:
        body["query"] = query
    if config["app_type"] == "chat":
        with CONVERSATION_LOCK:
            conversation_id = CONVERSATIONS.get(session)
        if conversation_id:
            body["conversation_id"] = conversation_id
    return body


def open_dify(
    config: dict[str, Any],
    session: str,
    query: str,
    streaming: bool,
):
    body = compact_json(dify_payload(config, session, query, streaming)).encode("utf-8")
    headers = {"Content-Type": "application/json", "Accept": "text/event-stream" if streaming else "application/json"}
    if config["api_key"]:
        headers["Authorization"] = f"Bearer {config['api_key']}"
    request = urllib.request.Request(dify_endpoint(config), data=body, headers=headers, method="POST")
    try:
        return urllib.request.urlopen(request, timeout=config["timeout_seconds"])
    except urllib.error.HTTPError as error:
        detail = error.read(65536).decode("utf-8", errors="replace")
        raise RuntimeError(f"Dify HTTP {error.code}: {detail}") from error
    except urllib.error.URLError as error:
        raise RuntimeError(f"Dify 连接失败：{error.reason}") from error


def remember_conversation(session: str, data: dict[str, Any]) -> None:
    conversation_id = data.get("conversation_id")
    if conversation_id:
        with CONVERSATION_LOCK:
            CONVERSATIONS[session] = str(conversation_id)
            while len(CONVERSATIONS) > 1000:
                CONVERSATIONS.pop(next(iter(CONVERSATIONS)))
            temporary = CONVERSATIONS_PATH.with_suffix(".tmp")
            temporary.write_text(
                json.dumps(CONVERSATIONS, ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
            temporary.replace(CONVERSATIONS_PATH)


def test_dify_connection(config: dict[str, Any]) -> dict[str, Any]:
    headers = {"Accept": "application/json"}
    if config["api_key"]:
        headers["Authorization"] = f"Bearer {config['api_key']}"
    request = urllib.request.Request(
        f"{config['base_url']}/parameters",
        headers=headers,
        method="GET",
    )
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=min(30, config["timeout_seconds"])) as response:
            data = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read(65536).decode("utf-8", errors="replace")
        raise RuntimeError(f"Dify HTTP {error.code}: {detail}") from error
    except urllib.error.URLError as error:
        raise RuntimeError(f"Dify 连接失败：{error.reason}") from error
    return {
        "ok": True,
        "message": "Dify App API 连接成功",
        "duration_ms": round((time.perf_counter() - started) * 1000),
        "details": {
            "opening_statement": bool(data.get("opening_statement")),
            "input_fields": len(data.get("user_input_form", [])),
            "file_upload": bool(data.get("file_upload", {}).get("enabled")),
        },
    }


def extract_answer(data: dict[str, Any]) -> str:
    if isinstance(data.get("answer"), str):
        return data["answer"]
    result = data.get("data", data)
    if isinstance(result, dict):
        outputs = result.get("outputs")
        if isinstance(outputs, dict):
            if isinstance(outputs.get("answer"), str):
                return outputs["answer"]
            if isinstance(outputs.get("result"), str):
                return outputs["result"]
            return json.dumps(outputs, ensure_ascii=False)
    return json.dumps(data, ensure_ascii=False)


def call_dify_blocking(config: dict[str, Any], session: str, query: str) -> tuple[str, dict[str, Any]]:
    with open_dify(config, session, query, False) as response:
        data = json.loads(response.read().decode("utf-8"))
    remember_conversation(session, data)
    return extract_answer(data), data


def iter_dify_stream(config: dict[str, Any], session: str, query: str) -> Iterable[tuple[str, dict[str, Any]]]:
    with open_dify(config, session, query, True) as response:
        event_data: list[str] = []
        for raw in response:
            line = raw.decode("utf-8", errors="replace").rstrip("\r\n")
            if not line:
                if event_data:
                    data = json.loads("\n".join(event_data))
                    remember_conversation(session, data)
                    answer = data.get("answer", "") if data.get("event") in {"message", "agent_message"} else ""
                    yield str(answer or ""), data
                    event_data.clear()
                continue
            if line.startswith("data:"):
                event_data.append(line[5:].lstrip())
        if event_data:
            data = json.loads("\n".join(event_data))
            remember_conversation(session, data)
            yield str(data.get("answer", "") or ""), data


def strip_json_fence(source: str) -> str:
    text = source.strip()
    if text.startswith("```"):
        text = re.sub(r"^```(?:json)?\s*", "", text, flags=re.IGNORECASE)
        text = re.sub(r"\s*```$", "", text)
    return text.strip()


def parse_tool_envelope(answer: str, tools: list[dict[str, Any]]) -> dict[str, Any]:
    try:
        envelope = json.loads(strip_json_fence(answer))
    except json.JSONDecodeError:
        return {"type": "final", "content": answer}
    if envelope.get("type") != "tool_calls":
        return {"type": "final", "content": str(envelope.get("content", answer))}
    allowed = {
        str(tool.get("function", tool).get("name")): tool.get("function", tool)
        for tool in tools
        if isinstance(tool.get("function", tool), dict)
    }
    calls = []
    for index, call in enumerate(envelope.get("calls", [])):
        name = str(call.get("name", ""))
        if name not in allowed:
            raise RuntimeError(f"Dify 请求了未注册工具：{name}")
        arguments = call.get("arguments", {})
        if not isinstance(arguments, dict):
            raise RuntimeError(f"工具 {name} 的 arguments 必须是 JSON 对象")
        validate_json_schema(arguments, allowed[name].get("parameters", {}), f"工具 {name} arguments")
        calls.append(
            {
                "id": str(call.get("id") or f"call_{uuid.uuid4().hex[:16]}"),
                "type": "function",
                "function": {"name": name, "arguments": compact_json(arguments)},
            }
        )
    if not calls:
        raise RuntimeError("Dify 返回了空工具调用列表")
    return {"type": "tool_calls", "calls": calls}


def validate_json_schema(value: Any, schema: dict[str, Any], path: str) -> None:
    if not isinstance(schema, dict):
        return
    expected = schema.get("type")
    checks = {
        "object": lambda item: isinstance(item, dict),
        "array": lambda item: isinstance(item, list),
        "string": lambda item: isinstance(item, str),
        "integer": lambda item: isinstance(item, int) and not isinstance(item, bool),
        "number": lambda item: isinstance(item, (int, float)) and not isinstance(item, bool),
        "boolean": lambda item: isinstance(item, bool),
        "null": lambda item: item is None,
    }
    if expected in checks and not checks[expected](value):
        raise RuntimeError(f"{path} 类型必须为 {expected}")
    if "enum" in schema and value not in schema["enum"]:
        raise RuntimeError(f"{path} 不在允许值中")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for required in schema.get("required", []):
            if required not in value:
                raise RuntimeError(f"{path} 缺少必填字段：{required}")
        if schema.get("additionalProperties") is False:
            extras = set(value) - set(properties)
            if extras:
                raise RuntimeError(f"{path} 包含未声明字段：{sorted(extras)[0]}")
        for key, item in value.items():
            if key in properties:
                validate_json_schema(item, properties[key], f"{path}.{key}")
    if isinstance(value, list) and isinstance(schema.get("items"), dict):
        for index, item in enumerate(value):
            validate_json_schema(item, schema["items"], f"{path}[{index}]")


def completion_response(model: str, content: str = "", tool_calls: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    message: dict[str, Any] = {"role": "assistant", "content": content or None}
    finish_reason = "stop"
    if tool_calls:
        message["tool_calls"] = tool_calls
        finish_reason = "tool_calls"
    return {
        "id": f"chatcmpl-{uuid.uuid4().hex}",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
    }


def stream_chunk(model: str, delta: dict[str, Any], finish_reason: str | None = None, completion_id: str = "") -> bytes:
    payload = {
        "id": completion_id or f"chatcmpl-{uuid.uuid4().hex}",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": model,
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
    }
    return f"data: {compact_json(payload)}\n\n".encode("utf-8")


class Handler(BaseHTTPRequestHandler):
    server_version = "DRPA-DifyBridge/0.2"

    def log_message(self, format: str, *args: Any) -> None:
        log(format % args)

    def send_json(self, status: int, value: Any) -> None:
        body = json.dumps(value, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        config = load_config()
        if self.path.rstrip("/") in {"/health", "/v1/health"}:
            self.send_json(200, {"status": "ok", "plugin": "dify-loves-hermes", "version": PLUGIN_VERSION})
            return
        if self.path.rstrip("/") == "/v1/models":
            self.send_json(200, {"object": "list", "data": [{"id": config["model"], "object": "model", "owned_by": "dify"}]})
            return
        if self.path.rstrip("/") == "/v1/provider/test":
            try:
                self.send_json(200, test_dify_connection(config))
            except Exception as error:
                self.send_json(502, {"ok": False, "message": str(error), "duration_ms": 0, "details": {}})
            return
        self.send_json(404, {"error": {"message": "Not found", "type": "not_found"}})

    def do_POST(self) -> None:
        if self.path.rstrip("/") != "/v1/chat/completions":
            self.send_json(404, {"error": {"message": "Not found", "type": "not_found"}})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if length <= 0 or length > 8 * 1024 * 1024:
                raise RuntimeError("请求体大小无效")
            payload = json.loads(self.rfile.read(length).decode("utf-8"))
            config = load_config()
            messages = payload.get("messages", [])
            tools = payload.get("tools", []) if config["tool_bridge"] else []
            session = normalize_session(payload, config)
            model = str(payload.get("model") or config["model"])
            query = tool_bridge_query(messages, tools) if tools else plain_query(messages)
            if payload.get("stream") and not tools:
                completion_id = f"chatcmpl-{uuid.uuid4().hex}"
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream; charset=utf-8")
                self.send_header("Cache-Control", "no-cache")
                self.send_header("Connection", "keep-alive")
                self.end_headers()
                self.wfile.write(stream_chunk(model, {"role": "assistant"}, completion_id=completion_id))
                self.wfile.flush()
                for content, _event in iter_dify_stream(config, session, query):
                    if content:
                        self.wfile.write(stream_chunk(model, {"content": content}, completion_id=completion_id))
                        self.wfile.flush()
                self.wfile.write(stream_chunk(model, {}, "stop", completion_id))
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()
                return
            answer, _metadata = call_dify_blocking(config, session, query)
            envelope = parse_tool_envelope(answer, tools) if tools else {"type": "final", "content": answer}
            if payload.get("stream"):
                completion_id = f"chatcmpl-{uuid.uuid4().hex}"
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream; charset=utf-8")
                self.send_header("Cache-Control", "no-cache")
                self.end_headers()
                if envelope["type"] == "tool_calls":
                    delta_calls = [dict(call, index=index) for index, call in enumerate(envelope["calls"])]
                    self.wfile.write(stream_chunk(model, {"role": "assistant", "tool_calls": delta_calls}, completion_id=completion_id))
                    self.wfile.write(stream_chunk(model, {}, "tool_calls", completion_id))
                else:
                    self.wfile.write(stream_chunk(model, {"role": "assistant", "content": envelope["content"]}, completion_id=completion_id))
                    self.wfile.write(stream_chunk(model, {}, "stop", completion_id))
                self.wfile.write(b"data: [DONE]\n\n")
                return
            if envelope["type"] == "tool_calls":
                self.send_json(200, completion_response(model, tool_calls=envelope["calls"]))
            else:
                self.send_json(200, completion_response(model, content=envelope["content"]))
        except Exception as error:
            log(f"request failed: {error}")
            self.send_json(502, {"error": {"message": str(error), "type": "dify_bridge_error"}})


def main() -> None:
    config = load_config()
    server = ThreadingHTTPServer(("127.0.0.1", config["port"]), Handler)
    log(f"listening on http://127.0.0.1:{config['port']}/v1")
    server.serve_forever(poll_interval=0.25)


if __name__ == "__main__":
    main()
