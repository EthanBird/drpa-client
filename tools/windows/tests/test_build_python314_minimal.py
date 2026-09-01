from __future__ import annotations

import importlib.util
import json
import sys
import zipfile
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "build_python314_minimal.py"
SPEC = importlib.util.spec_from_file_location("build_python314_minimal", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.path.insert(0, str(MODULE_PATH.parent))
SPEC.loader.exec_module(MODULE)


def test_embedded_search_path_adds_vendor_without_site(tmp_path: Path) -> None:
    pth = tmp_path / "python314._pth"
    pth.write_text("python314.zip\n.\n#import site\n", encoding="utf-8")

    MODULE.configure_embedded_search_path(tmp_path)

    assert pth.read_text(encoding="utf-8") == "python314.zip\n.\nvendor\n"


def test_runtime_manifest_declares_frozen_minimal_features(tmp_path: Path) -> None:
    MODULE.write_runtime_manifest(tmp_path, "2.1.1-test")

    manifest = json.loads((tmp_path / "manifest.json").read_text(encoding="utf-8"))

    assert manifest["environmentMode"] == "frozen"
    assert manifest["pythonVersion"] == MODULE.PYTHON_VERSION
    assert "studio.kernel" in manifest["features"]
    assert "jupyter" not in manifest["features"]


def test_extract_archive_rejects_parent_traversal(tmp_path: Path) -> None:
    archive = tmp_path / "bad.zip"
    with zipfile.ZipFile(archive, "w") as output:
        output.writestr("../escape.txt", "bad")

    try:
        MODULE.extract_archive(archive, tmp_path / "stage")
    except ValueError as error:
        assert "unsafe" in str(error)
    else:
        raise AssertionError("parent traversal must be rejected")


def test_update_catalog_includes_all_component_packs(tmp_path: Path) -> None:
    (tmp_path / "catalog.json").write_text(
        json.dumps({"schema": 1, "version": "2.1.1", "components": [], "artifacts": []}),
        encoding="utf-8",
    )
    (tmp_path / "a.drpac").write_bytes(b"a")
    (tmp_path / "b.drpac").write_bytes(b"bb")

    MODULE.update_component_catalog(tmp_path, "2.1.1")

    catalog = json.loads((tmp_path / "catalog.json").read_text(encoding="utf-8"))
    assert catalog["components"] == ["a.drpac", "b.drpac"]
    assert [item["bytes"] for item in catalog["artifacts"]] == [1, 2]
