from __future__ import annotations

import json
from pathlib import Path

from tools.offline.sync_runtime_adapter import refresh_sealed_metadata


def test_refresh_sealed_metadata_preserves_provenance_and_rehashes_runtime(tmp_path: Path) -> None:
    runtime = tmp_path / "runtime"
    python = runtime / "python/cpython-3.11-windows-x86_64-none/python.exe"
    browser = runtime / "browser/chrome-win64/chrome.exe"
    wheel = runtime / "wheelhouse/demo-1.0-py3-none-any.whl"
    for path, content in ((python, b"python"), (browser, b"chrome"), (wheel, b"wheel")):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)

    (runtime / "wheelhouse-lock.json").write_text(
        json.dumps({"sourceBuilds": [{"name": "rpa", "revision": "test"}]}),
        encoding="utf-8",
    )
    (runtime / "manifest.json").write_text(
        json.dumps(
            {
                "bundleVersion": "2.1.1-test",
                "platform": "windows-x86_64",
                "pythonVersion": "3.11.9",
                "uvVersion": "0.11.28",
                "chromeForTestingVersion": "150.0.0.0",
            }
        ),
        encoding="utf-8",
    )

    refresh_sealed_metadata(runtime)

    wheel_lock = json.loads((runtime / "wheelhouse-lock.json").read_text(encoding="utf-8"))
    manifest = json.loads((runtime / "manifest.json").read_text(encoding="utf-8"))
    assert wheel_lock["sourceBuilds"] == [{"name": "rpa", "revision": "test"}]
    assert wheel_lock["wheels"][0]["filename"] == wheel.name
    assert manifest["pythonExecutable"] == "python/cpython-3.11-windows-x86_64-none/python.exe"
    assert any(item["path"] == f"wheelhouse/{wheel.name}" for item in manifest["files"])
    assert "manifest.json" in (runtime / "SHA256SUMS").read_text(encoding="utf-8")
