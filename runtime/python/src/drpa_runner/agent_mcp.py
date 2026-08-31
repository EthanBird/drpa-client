from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import time
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlparse


# MCP is a UTF-8 JSON protocol. Windows may otherwise inherit a legacy console
# code page and fail as soon as a page snapshot contains non-ASCII characters.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


_PAGE: Any | None = None


def _browser_target() -> tuple[int, Path]:
    raw_port = os.environ.get("DRPA_BROWSER_PORT", "9222")
    try:
        port = int(raw_port)
    except ValueError as exc:
        raise RuntimeError(f"DRPA_BROWSER_PORT is invalid: {raw_port}") from exc
    if not 1024 <= port <= 65535:
        raise RuntimeError("DRPA_BROWSER_PORT must be in 1024..65535")
    root = Path(
        os.environ.get("DRPA_BROWSER_PROFILE_ROOT")
        or (Path.home() / ".drpa" / "browser" / "drissionpage")
    ).resolve()
    return port, root / "visible"


def _page() -> Any:
    global _PAGE
    if _PAGE is not None:
        return _PAGE
    try:
        from DrissionPage import ChromiumOptions, ChromiumPage
    except ImportError as exc:
        raise RuntimeError("DrissionPage is missing from the bundled DRPA runtime") from exc
    port, profile = _browser_target()
    profile.mkdir(parents=True, exist_ok=True)
    options = ChromiumOptions()
    browser_path = os.environ.get("DRPA_BROWSER_PATH", "").strip()
    if browser_path:
        path = Path(browser_path)
        if not path.is_file():
            raise RuntimeError(f"bundled Chrome executable is missing: {path}")
        options.set_browser_path(str(path))
    options.set_local_port(port)
    options.set_user_data_path(str(profile))
    options.headless(False)
    _PAGE = ChromiumPage(options)
    return _PAGE


def _run_json(script: str) -> Any:
    value = _page().run_js(script)
    if isinstance(value, str):
        return json.loads(value)
    return value


def browser_open(arguments: dict[str, Any]) -> dict[str, Any]:
    url = str(arguments.get("url", "")).strip()
    parsed = urlparse(url)
    if parsed.scheme not in {"http", "https", "file", "about"}:
        raise ValueError("url must use http, https, file, or about")
    page = _page()
    page.get(url)
    return {
        "ok": True,
        "url": str(page.url),
        "title": str(page.title),
        "summary": f"Chrome opened {page.title or page.url}",
    }


def browser_snapshot(arguments: dict[str, Any]) -> dict[str, Any]:
    max_chars = max(1_000, min(100_000, int(arguments.get("maxChars", 20_000))))
    script = f"""
    return (() => {{
      const maxChars = {max_chars};
      const selector = 'a,button,input,textarea,select,[role="button"],[contenteditable="true"],summary';
      let next = Number(document.documentElement.dataset.drpaAgentRefCounter || '0');
      const elements = [];
      for (const element of document.querySelectorAll(selector)) {{
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        if (rect.width <= 0 || rect.height <= 0 || style.visibility === 'hidden' || style.display === 'none') continue;
        let ref = element.getAttribute('data-drpa-agent-ref');
        if (!ref) {{ ref = `e${{++next}}`; element.setAttribute('data-drpa-agent-ref', ref); }}
        const text = (element.innerText || element.value || element.getAttribute('aria-label') || element.title || '').trim().slice(0, 240);
        elements.push({{
          ref,
          tag: element.tagName.toLowerCase(),
          role: element.getAttribute('role') || '',
          type: element.getAttribute('type') || '',
          text,
          disabled: Boolean(element.disabled),
        }});
        if (elements.length >= 500) break;
      }}
      document.documentElement.dataset.drpaAgentRefCounter = String(next);
      return JSON.stringify({{
        title: document.title,
        url: location.href,
        text: (document.body?.innerText || '').slice(0, maxChars),
        elements,
      }});
    }})()
    """
    snapshot = _run_json(script)
    snapshot.update(
        ok=True,
        summary=f"Captured Chrome snapshot with {len(snapshot.get('elements', []))} interactive elements",
    )
    return snapshot


def _element_script(ref: str, body: str) -> str:
    encoded = json.dumps(ref)
    return f"""
    return (() => {{
      const element = document.querySelector('[data-drpa-agent-ref="' + CSS.escape({encoded}) + '"]');
      if (!element) return JSON.stringify({{ok:false,error:'element ref not found'}});
      {body}
    }})()
    """


def browser_click(arguments: dict[str, Any]) -> dict[str, Any]:
    ref = str(arguments.get("ref", "")).strip()
    if not ref:
        raise ValueError("ref is required")
    result = _run_json(_element_script(ref, "element.scrollIntoView({block:'center'}); element.click(); return JSON.stringify({ok:true});"))
    if not result.get("ok"):
        raise RuntimeError(str(result.get("error", "click failed")))
    time.sleep(0.15)
    page = _page()
    return {"ok": True, "ref": ref, "url": str(page.url), "summary": f"Clicked Chrome element {ref}"}


def browser_back(_arguments: dict[str, Any]) -> dict[str, Any]:
    page = _page()
    page.back()
    return {
        "ok": True,
        "url": str(page.url),
        "title": str(page.title),
        "summary": f"Chrome navigated back to {page.title or page.url}",
    }


def browser_reload(_arguments: dict[str, Any]) -> dict[str, Any]:
    page = _page()
    page.refresh()
    return {
        "ok": True,
        "url": str(page.url),
        "title": str(page.title),
        "summary": f"Reloaded Chrome page {page.title or page.url}",
    }


def browser_type(arguments: dict[str, Any]) -> dict[str, Any]:
    ref = str(arguments.get("ref", "")).strip()
    text = str(arguments.get("text", ""))
    submit = bool(arguments.get("submit", False))
    if not ref:
        raise ValueError("ref is required")
    encoded_text = json.dumps(text)
    submit_script = "if (element.form?.requestSubmit) element.form.requestSubmit();" if submit else ""
    body = f"""
      element.scrollIntoView({{block:'center'}});
      element.focus();
      if ('value' in element) {{
        const prototype = element instanceof HTMLTextAreaElement
          ? HTMLTextAreaElement.prototype
          : element instanceof HTMLSelectElement
            ? HTMLSelectElement.prototype
            : HTMLInputElement.prototype;
        const setter = Object.getOwnPropertyDescriptor(prototype, 'value')?.set;
        if (setter) setter.call(element, {encoded_text}); else element.value = {encoded_text};
      }} else {{ element.textContent = {encoded_text}; }}
      element.dispatchEvent(new InputEvent('input', {{bubbles:true,inputType:'insertText',data:{encoded_text}}}));
      element.dispatchEvent(new Event('change', {{bubbles:true}}));
      {submit_script}
      return JSON.stringify({{ok:true}});
    """
    result = _run_json(_element_script(ref, body))
    if not result.get("ok"):
        raise RuntimeError(str(result.get("error", "type failed")))
    return {"ok": True, "ref": ref, "submitted": submit, "summary": f"Typed into Chrome element {ref}"}


def browser_select(arguments: dict[str, Any]) -> dict[str, Any]:
    ref = str(arguments.get("ref", "")).strip()
    value = str(arguments.get("value", ""))
    if not ref:
        raise ValueError("ref is required")
    encoded_value = json.dumps(value)
    body = f"""
      if (!(element instanceof HTMLSelectElement)) return JSON.stringify({{ok:false,error:'element is not a select'}});
      const option = Array.from(element.options).find((item) => item.value === {encoded_value} || item.text === {encoded_value});
      if (!option) return JSON.stringify({{ok:false,error:'select option not found'}});
      element.value = option.value;
      element.dispatchEvent(new Event('input', {{bubbles:true}}));
      element.dispatchEvent(new Event('change', {{bubbles:true}}));
      return JSON.stringify({{ok:true,value:option.value,text:option.text}});
    """
    result = _run_json(_element_script(ref, body))
    if not result.get("ok"):
        raise RuntimeError(str(result.get("error", "select failed")))
    return {
        "ok": True,
        "ref": ref,
        "value": result.get("value"),
        "text": result.get("text"),
        "summary": f"Selected Chrome option for {ref}",
    }


def browser_scroll(arguments: dict[str, Any]) -> dict[str, Any]:
    direction = str(arguments.get("direction", "down")).strip().lower()
    if direction not in {"up", "down", "left", "right"}:
        raise ValueError("direction must be up, down, left, or right")
    amount = max(100, min(5_000, int(arguments.get("amount", 700))))
    dx = amount if direction == "right" else -amount if direction == "left" else 0
    dy = amount if direction == "down" else -amount if direction == "up" else 0
    result = _run_json(
        f"window.scrollBy({{left:{dx},top:{dy},behavior:'auto'}}); return JSON.stringify({{ok:true,x:window.scrollX,y:window.scrollY}});"
    )
    return {
        "ok": True,
        "direction": direction,
        "amount": amount,
        "x": result.get("x"),
        "y": result.get("y"),
        "summary": f"Scrolled Chrome {direction} by {amount}px",
    }


def browser_wait(arguments: dict[str, Any]) -> dict[str, Any]:
    seconds = max(0.0, min(120.0, float(arguments.get("seconds", 1.0))))
    expected = str(arguments.get("text", ""))
    deadline = time.monotonic() + seconds
    while True:
        if not expected:
            time.sleep(seconds)
            break
        found = _page().run_js(
            f"return (document.body?.innerText || '').includes({json.dumps(expected)});"
        )
        if found:
            break
        if time.monotonic() >= deadline:
            raise TimeoutError(f"text did not appear before timeout: {expected}")
        time.sleep(0.1)
    return {"ok": True, "text": expected, "summary": "Chrome wait condition completed"}


def browser_screenshot(arguments: dict[str, Any]) -> dict[str, Any]:
    root = Path(os.environ.get("DRPA_AGENT_ARTIFACT_ROOT") or (Path.cwd() / ".drpa-browser"))
    root.mkdir(parents=True, exist_ok=True)
    target = root / f"chrome-{time.time_ns()}.png"
    _page().get_screenshot(path=str(target), full_page=bool(arguments.get("fullPage", True)))
    return {"ok": True, "path": str(target), "summary": f"Saved Chrome screenshot to {target}"}


def browser_status(_arguments: dict[str, Any]) -> dict[str, Any]:
    page = _page()
    port, profile = _browser_target()
    return {
        "ok": True,
        "connected": True,
        "port": port,
        "profile": str(profile),
        "url": str(page.url),
        "title": str(page.title),
        "summary": f"DRPA Chrome Bridge is connected on port {port}",
    }


def _host_call(name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    endpoint = os.environ.get("DRPA_AGENT_BRIDGE_ENDPOINT", "").strip()
    token = os.environ.get("DRPA_AGENT_BRIDGE_TOKEN", "").strip()
    if not endpoint or not token:
        raise RuntimeError("DRPA Agent Host Bridge is not available")
    host, raw_port = endpoint.rsplit(":", 1)
    request = json.dumps({"token": token, "name": name, "arguments": arguments}, ensure_ascii=False) + "\n"
    with socket.create_connection((host, int(raw_port)), timeout=10) as connection:
        connection.sendall(request.encode("utf-8"))
        stream = connection.makefile("r", encoding="utf-8")
        response = json.loads(stream.readline())
    if not response.get("ok"):
        raise RuntimeError(str(response.get("error", "DRPA Host Bridge request failed")))
    return dict(response.get("result") or {})


TOOLS: dict[str, Callable[[dict[str, Any]], dict[str, Any]]] = {
    "browser_open": browser_open,
    "browser_back": browser_back,
    "browser_reload": browser_reload,
    "browser_snapshot": browser_snapshot,
    "browser_click": browser_click,
    "browser_type": browser_type,
    "browser_select": browser_select,
    "browser_scroll": browser_scroll,
    "browser_wait": browser_wait,
    "browser_screenshot": browser_screenshot,
    "browser_status": browser_status,
    "rpaz_list_packages": lambda arguments: _host_call("rpaz_list_packages", arguments),
    "rpaz_run_package": lambda arguments: _host_call("rpaz_run_package", arguments),
    "run_list": lambda arguments: _host_call("run_list", arguments),
    "run_get_detail": lambda arguments: _host_call("run_get_detail", arguments),
    "vault_list_credentials": lambda arguments: _host_call("vault_list_credentials", arguments),
    "vault_get_credential": lambda arguments: _host_call("vault_get_credential", arguments),
    "vault_upsert_credential": lambda arguments: _host_call("vault_upsert_credential", arguments),
    "document_read": lambda arguments: _host_call("document_read", arguments),
    "document_create": lambda arguments: _host_call("document_create", arguments),
    "document_convert": lambda arguments: _host_call("document_convert", arguments),
}


def _schema(properties: dict[str, Any], required: list[str] | None = None) -> dict[str, Any]:
    schema: dict[str, Any] = {"type": "object", "properties": properties, "additionalProperties": False}
    if required:
        schema["required"] = required
    return schema


TOOL_DEFINITIONS = [
    {"name": "browser_open", "description": "Open a URL in bundled persistent DRPA Chrome.", "inputSchema": _schema({"url": {"type": "string"}}, ["url"])},
    {"name": "browser_back", "description": "Navigate the persistent DRPA Chrome tab back.", "inputSchema": _schema({})},
    {"name": "browser_reload", "description": "Reload the current persistent DRPA Chrome page.", "inputSchema": _schema({})},
    {"name": "browser_snapshot", "description": "Read current Chrome text and interactive element refs.", "inputSchema": _schema({"maxChars": {"type": "integer", "minimum": 1000, "maximum": 100000}})},
    {"name": "browser_click", "description": "Click an element ref from browser_snapshot.", "inputSchema": _schema({"ref": {"type": "string"}}, ["ref"])},
    {"name": "browser_type", "description": "Type into an element ref and optionally submit its form.", "inputSchema": _schema({"ref": {"type": "string"}, "text": {"type": "string"}, "submit": {"type": "boolean"}}, ["ref", "text"])},
    {"name": "browser_select", "description": "Select an option by value or visible text using an element ref.", "inputSchema": _schema({"ref": {"type": "string"}, "value": {"type": "string"}}, ["ref", "value"])},
    {"name": "browser_scroll", "description": "Scroll the current page in one direction.", "inputSchema": _schema({"direction": {"type": "string", "enum": ["up", "down", "left", "right"]}, "amount": {"type": "integer", "minimum": 100, "maximum": 5000}})},
    {"name": "browser_wait", "description": "Wait for time or page text.", "inputSchema": _schema({"seconds": {"type": "number", "minimum": 0, "maximum": 120}, "text": {"type": "string"}})},
    {"name": "browser_screenshot", "description": "Save a Chrome screenshot as an Agent artifact.", "inputSchema": _schema({"fullPage": {"type": "boolean"}})},
    {"name": "browser_status", "description": "Inspect the shared DRPA Chrome connection.", "inputSchema": _schema({})},
    {"name": "rpaz_list_packages", "description": "List installed RPAZ packages and profiles.", "inputSchema": _schema({})},
    {"name": "rpaz_run_package", "description": "Run an installed RPAZ package through DRPA Host so it appears in run history.", "inputSchema": _schema({"packageId": {"type": "string"}, "profileId": {"type": "string"}, "parameters": {"type": "object"}}, ["packageId"])},
    {"name": "run_list", "description": "List DRPA run records.", "inputSchema": _schema({"packageId": {"type": "string"}, "status": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 500}})},
    {"name": "run_get_detail", "description": "Read complete DRPA run details, structured events, and debug logs.", "inputSchema": _schema({"runId": {"type": "string"}}, ["runId"])},
    {"name": "vault_list_credentials", "description": "List unlocked DRPA credential summaries without secret values.", "inputSchema": _schema({})},
    {"name": "vault_get_credential", "description": "Read one sensitive credential after the user has unlocked the local vault.", "inputSchema": _schema({"id": {"type": "string"}}, ["id"])},
    {"name": "vault_upsert_credential", "description": "Create or update a credential in the unlocked local vault.", "inputSchema": _schema({"id": {"type": "string"}, "name": {"type": "string"}, "kind": {"type": "string", "enum": ["login", "apiKey", "token", "database", "ssh", "secureNote"]}, "username": {"type": "string"}, "secret": {"type": "string"}, "uri": {"type": "string"}, "notes": {"type": "string"}, "tags": {"type": "array", "items": {"type": "string"}}, "favorite": {"type": "boolean"}}, ["name", "kind", "secret"])},
    {"name": "document_read", "description": "Read a DRPA conversation attachment by opaque documentId.", "inputSchema": _schema({"documentId": {"type": "string"}}, ["documentId"])},
    {"name": "document_create", "description": "Create a PDF, DOCX, XLSX, or PPTX artifact in the current DRPA conversation.", "inputSchema": _schema({"format": {"type": "string", "enum": ["pdf", "docx", "xlsx", "pptx"]}, "title": {"type": "string"}, "content": {}, "fileName": {"type": "string"}}, ["format", "title", "content"])},
    {"name": "document_convert", "description": "Convert a DRPA conversation attachment or artifact to another office format.", "inputSchema": _schema({"documentId": {"type": "string"}, "targetFormat": {"type": "string", "enum": ["pdf", "docx", "xlsx", "pptx"]}, "title": {"type": "string"}, "fileName": {"type": "string"}}, ["documentId", "targetFormat"])},
]


def execute(name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
    handler = TOOLS.get(name)
    if handler is None:
        raise KeyError(f"unknown DRPA MCP tool: {name}")
    return handler(arguments or {})


def _mcp_result(request_id: Any, result: Any) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": request_id, "result": result}


def _serve_mcp() -> int:
    for raw_line in sys.stdin.buffer:
        try:
            request = json.loads(raw_line.decode("utf-8"))
            request_id = request.get("id")
            method = request.get("method")
            if method == "initialize":
                response = _mcp_result(request_id, {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {"listChanged": False}},
                    "serverInfo": {"name": "drpa-agent-tools", "version": "3.0.0"},
                })
            elif method == "tools/list":
                response = _mcp_result(request_id, {"tools": TOOL_DEFINITIONS})
            elif method == "tools/call":
                params = request.get("params") or {}
                try:
                    output = execute(str(params.get("name", "")), params.get("arguments") or {})
                    response = _mcp_result(request_id, {
                        "content": [{"type": "text", "text": json.dumps(output, ensure_ascii=False)}],
                        "structuredContent": output,
                        "isError": False,
                    })
                except Exception as exc:
                    response = _mcp_result(request_id, {
                        "content": [{"type": "text", "text": str(exc)}],
                        "isError": True,
                    })
            elif method == "ping":
                response = _mcp_result(request_id, {})
            elif request_id is None:
                continue
            else:
                response = {"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": f"method not found: {method}"}}
        except Exception as exc:
            response = {"jsonrpc": "2.0", "id": None, "error": {"code": -32603, "message": str(exc)}}
        sys.stdout.write(json.dumps(response, ensure_ascii=False, separators=(",", ":")) + "\n")
        sys.stdout.flush()
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="DRPA Chrome and Host MCP bridge")
    parser.add_argument("--call", choices=sorted(TOOLS))
    args = parser.parse_args(argv)
    if args.call:
        try:
            raw = sys.stdin.buffer.read()
            arguments = json.loads(raw.decode("utf-8")) if raw.strip() else {}
            print(json.dumps(execute(args.call, arguments), ensure_ascii=False))
            return 0
        except Exception as exc:
            print(str(exc), file=sys.stderr)
            return 1
    return _serve_mcp()


if __name__ == "__main__":
    raise SystemExit(main())
