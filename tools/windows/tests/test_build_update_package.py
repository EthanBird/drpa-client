from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).parents[1] / "build_update_package.py"
SPEC = importlib.util.spec_from_file_location("build_update_package", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def write_fixture(stage: Path) -> None:
    files = {
        "DRPA Next.exe": b"desktop-v2",
        "drpa-updater.exe": b"worker-v2",
        "README.md": b"documentation",
        "examples/sample.rpaz": b"sample",
        "runtime/python/python.exe": b"python-runtime",
        "runtime/browser/chrome.exe": b"chrome-runtime",
        "webview2/msedgewebview2.exe": b"webview-runtime",
        "data/projects/keep.txt": b"user-data",
    }
    for relative, content in files.items():
        target = stage / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)


def test_default_update_omits_repeated_runtime_components(tmp_path: Path) -> None:
    stage = tmp_path / "stage"
    output = tmp_path / "update.drpa-update"
    write_fixture(stage)

    manifest = MODULE.build(stage, output, "0.2.1-preview-1")
    paths = {item["path"] for item in manifest["files"]}

    assert manifest["schema"] == 2
    assert manifest["hostProtocol"] == 2
    assert manifest["workerProtocol"] == 2
    assert manifest["packageKind"] == "bootstrap"
    assert manifest["minimumHostVersion"] == "0.3.0"
    assert all("sha256" not in item for item in manifest["files"])
    assert "sha256" not in manifest["worker"]
    assert "DRPA Next.exe" in paths
    assert "README.md" in paths
    assert "install-manifest.json" in paths
    assert "runtime/python/python.exe" not in paths
    assert "runtime/browser/chrome.exe" not in paths
    assert "webview2/msedgewebview2.exe" not in paths
    assert "data/projects/keep.txt" not in paths
    assert "drpa-updater.exe" in paths
    assert (stage / "install-manifest.json").is_file()
    inventory = json.loads((stage / "install-manifest.json").read_text(encoding="utf-8"))
    inventory_paths = {item["path"] for item in inventory["files"]}
    assert "runtime/python/python.exe" in inventory_paths
    assert "runtime/browser/chrome.exe" in inventory_paths
    assert "webview2/msedgewebview2.exe" in inventory_paths
    assert "drpa-updater.exe" in inventory_paths
    assert "data/projects/keep.txt" not in inventory_paths

    with zipfile.ZipFile(output) as archive:
        names = set(archive.namelist())
        assert "worker/drpa-updater.exe" in names
        assert "files/DRPA Next.exe" in names
        assert "files/drpa-updater.exe" in names
        assert "files/runtime/browser/chrome.exe" not in names


def test_base_catalog_includes_only_changed_files_and_removed_paths(tmp_path: Path) -> None:
    stage = tmp_path / "stage"
    output = tmp_path / "update.drpa-update"
    write_fixture(stage)
    catalog, _ = MODULE.make_catalog(stage, "0.2.0-preview-1")
    for item in catalog["files"]:
        if item["path"] == "DRPA Next.exe":
            item["sha256"] = "0" * 64
    catalog["files"].append(
        {"path": "obsolete.dll", "bytes": 3, "sha256": "1" * 64, "component": "application"}
    )
    base = tmp_path / "base.json"
    base.write_text(json.dumps(catalog), encoding="utf-8")

    manifest = MODULE.build(stage, output, "0.2.1-preview-2", base_manifest=base)
    paths = {item["path"] for item in manifest["files"]}

    assert manifest["packageKind"] == "delta"
    assert paths == {"DRPA Next.exe", "install-manifest.json"}
    assert manifest["remove"] == ["obsolete.dll"]


def test_catalog_delta_updates_changed_browser_but_keeps_webview_installer_only(tmp_path: Path) -> None:
    stage = tmp_path / "stage"
    output = tmp_path / "update.drpa-update"
    write_fixture(stage)
    catalog, _ = MODULE.make_catalog(stage, "0.2.0-preview-1")
    for item in catalog["files"]:
        if item["path"] in {"runtime/browser/chrome.exe", "webview2/msedgewebview2.exe"}:
            item["sha256"] = "0" * 64
    base = tmp_path / "base.json"
    base.write_text(json.dumps(catalog), encoding="utf-8")

    manifest = MODULE.build(stage, output, "0.2.1-preview-2", base_manifest=base)
    paths = {item["path"] for item in manifest["files"]}

    assert "runtime/browser/chrome.exe" in paths
    assert "webview2/msedgewebview2.exe" not in paths


def test_partial_update_merges_overlay_into_complete_inventory(tmp_path: Path) -> None:
    full_stage = tmp_path / "full"
    write_fixture(full_stage)
    base_catalog, _ = MODULE.make_catalog(full_stage, "0.2.0-preview-10")
    base = tmp_path / "base.json"
    base.write_text(json.dumps(base_catalog), encoding="utf-8")

    overlay = tmp_path / "overlay"
    overlay.mkdir()
    (overlay / "DRPA Next.exe").write_bytes(b"desktop-v3")
    (overlay / "drpa-updater.exe").write_bytes(b"worker-v3")
    output = tmp_path / "partial.drpa-update"
    manifest = MODULE.build(
        overlay,
        output,
        "0.2.0-preview-11",
        base_manifest=base,
        worker=overlay / "drpa-updater.exe",
        partial=True,
    )

    paths = {item["path"] for item in manifest["files"]}
    assert paths == {"DRPA Next.exe", "drpa-updater.exe", "install-manifest.json"}
    assert manifest["remove"] == []
    merged = json.loads((overlay / "install-manifest.json").read_text(encoding="utf-8"))
    merged_paths = {item["path"] for item in merged["files"]}
    assert "runtime/python/python.exe" in merged_paths
    assert "runtime/browser/chrome.exe" in merged_paths
    assert "webview2/msedgewebview2.exe" in merged_paths


def test_catalog_rejects_external_directory_links(tmp_path: Path) -> None:
    stage = tmp_path / "stage"
    external = tmp_path / "external-runtime"
    stage.mkdir()
    external.mkdir()
    (stage / "DRPA Next.exe").write_bytes(b"host")
    (external / "python.exe").write_bytes(b"python")
    link = stage / "runtime"
    if os.name == "nt":
        created = subprocess.run(
            ["cmd", "/c", "mklink", "/J", str(link), str(external)],
            capture_output=True,
            text=True,
            check=False,
        )
        if created.returncode != 0:
            pytest.skip(f"directory junctions are unavailable: {created.stderr}")
    else:
        os.symlink(external, link, target_is_directory=True)

    with pytest.raises(ValueError, match="link or reparse point"):
        MODULE.make_catalog(stage, "0.3.0")
