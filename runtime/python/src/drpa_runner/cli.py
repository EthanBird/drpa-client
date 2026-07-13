from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .events import EventWriter
from .executor import ExecutionRequest, execute_request


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="drpa-python-runtime")
    parser.add_argument("--request", type=Path, required=True, help="Host-created execution request")
    args = parser.parse_args(argv)
    events = EventWriter()

    try:
        with args.request.open("r", encoding="utf-8") as handle:
            raw = json.load(handle)
        request = ExecutionRequest.from_dict(raw)
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
        events.emit("error", message=f"invalid execution request: {exc}", traceback=None)
        return 2

    return execute_request(request, events)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
