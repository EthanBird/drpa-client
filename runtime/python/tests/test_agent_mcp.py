from __future__ import annotations

import json
import subprocess
import sys

from drpa_runner import agent_mcp


def test_mcp_exposes_chrome_rpaz_and_run_debug_tools() -> None:
    names = {tool["name"] for tool in agent_mcp.TOOL_DEFINITIONS}

    assert {
        "browser_open",
        "browser_snapshot",
        "browser_click",
        "browser_type",
        "browser_screenshot",
        "rpaz_run_package",
        "run_list",
        "run_get_detail",
    } <= names


def test_stdio_mcp_initialize_and_list_tools() -> None:
    requests = "\n".join(
        [
            json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
            "",
        ]
    )
    result = subprocess.run(
        [sys.executable, "-m", "drpa_runner.agent_mcp"],
        input=requests,
        text=True,
        capture_output=True,
        check=False,
        timeout=10,
    )

    assert result.returncode == 0, result.stderr
    responses = [json.loads(line) for line in result.stdout.splitlines()]
    assert responses[0]["result"]["serverInfo"]["name"] == "drpa-agent-tools"
    assert any(tool["name"] == "browser_status" for tool in responses[1]["result"]["tools"])
