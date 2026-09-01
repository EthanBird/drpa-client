from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]


def test_windows_webviews_share_the_low_memory_policy() -> None:
    config = json.loads(
        (ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text(encoding="utf-8")
    )
    windows = config["app"]["windows"]
    arguments = {window.get("additionalBrowserArgs", "") for window in windows}

    assert len(arguments) == 1
    [value] = arguments
    for required in (
        "--disable-gpu",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-extensions",
        "--disable-sync",
        "msSmartScreenProtection",
    ):
        assert required in value
