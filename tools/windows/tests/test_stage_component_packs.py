from __future__ import annotations

import json
import zipfile
from pathlib import Path

from tools.windows.stage_component_packs import stage_component_packs


def write(path: Path, content: bytes = b"payload") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)


def make_stage(root: Path) -> Path:
    stage = root / "stage"
    write(stage / "runtime/python/python.exe")
    write(stage / "runtime/bootstrap_runtime.py")
    write(stage / "runtime/browser/chrome-win64/chrome.exe")
    write(stage / "webview2/msedgewebview2.exe")
    write(stage / "jcode/jcode.exe")
    write(stage / "DRPA Next.exe", b"desktop")
    (stage / "runtime/manifest.json").write_text(
        json.dumps(
            {
                "platform": "windows-x86_64",
                "pythonExecutable": "python/python.exe",
                "browserExecutable": "browser/chrome-win64/chrome.exe",
            }
        ),
        encoding="utf-8",
    )
    return stage


def manifest(path: Path) -> dict[str, object]:
    with zipfile.ZipFile(path) as archive:
        return json.loads(archive.read("component.json"))


def test_splits_browser_webview_python_and_jcode_into_independent_packs(tmp_path: Path) -> None:
    stage = make_stage(tmp_path)
    outputs = stage_component_packs(stage, "2.1.1")
    assert len(outputs) == 5
    python = manifest(stage / "component-packs/org.drpa.python-runtime.drpac")
    browser = manifest(stage / "component-packs/org.drpa.browser.chromium.drpac")
    assert "runtime.python" in python["provides"]
    assert all(not item["path"].startswith("browser/") for item in python["files"])
    assert browser["entrypoints"]["browser"] == "chrome-win64/chrome.exe"
    core = json.loads((stage / "component-packs/core-files.json").read_text(encoding="utf-8"))
    assert "DRPA Next.exe" in core["files"]
    assert "component-packs/org.drpa.desktop-ui.drpac" in core["files"]
    assert "component-packs/org.drpa.python-runtime.drpac" in core["files"]
    assert "component-packs/core-files.json" in core["files"]


def test_prune_removes_expanded_optional_payload_but_keeps_packs(tmp_path: Path) -> None:
    stage = make_stage(tmp_path)
    launcher = tmp_path / "drpa-launcher.exe"
    launcher.write_bytes(b"launcher")
    outputs = stage_component_packs(stage, "2.1.1", prune_legacy=True, launcher=launcher)
    assert all(path.is_file() for path in outputs)
    assert not (stage / "runtime").exists()
    assert not (stage / "webview2").exists()
    assert not (stage / "jcode").exists()
    assert (stage / "DRPA Next.exe").read_bytes() == b"launcher"
    desktop = manifest(stage / "component-packs/org.drpa.desktop-ui.drpac")
    assert desktop["entrypoints"]["desktop"] == "DRPA Next.exe"


def test_components_only_writes_independent_release_catalog_and_removes_stale_pack(tmp_path: Path) -> None:
    stage = make_stage(tmp_path)
    output = tmp_path / "release-components"
    output.mkdir()
    (output / "obsolete.drpac").write_bytes(b"old")

    outputs = stage_component_packs(
        stage,
        "2.1.1",
        output_root=output,
        write_core_manifest=False,
    )

    assert len(outputs) == 5
    assert not (output / "obsolete.drpac").exists()
    assert not (output / "core-files.json").exists()
    catalog = json.loads((output / "catalog.json").read_text(encoding="utf-8"))
    assert len(catalog["artifacts"]) == 5
    assert all(item["bytes"] > 0 and len(item["sha256"]) == 64 for item in catalog["artifacts"])
    assert (stage / "runtime").is_dir()
