from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "installer" / "windows" / "drpa-next.nsi"
WORKFLOW = ROOT / ".github" / "workflows" / "desktop-release.yml"
MODULAR_BUILD = ROOT / "tools" / "windows" / "build_modular_setup.ps1"


def test_nsis_script_declares_utf8_before_non_ascii_text() -> None:
    source = SCRIPT.read_bytes().decode("utf-8", errors="strict")
    first_two_lines = source.splitlines()[:2]

    assert "# -*- coding: utf-8 -*-" in first_two_lines
    assert "Unicode true" in source
    assert "请选择非系统盘上的安装目录" in source
    assert "卸载 DRPA Next" in source


def test_release_workflow_forces_utf8_input_charset() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    modular_build = MODULAR_BUILD.read_text(encoding="utf-8")

    assert "build_modular_setup.ps1" in workflow
    assert '& $Makensis "/INPUTCHARSET" "UTF8"' in modular_build


def test_full_rebuild_moves_the_existing_stable_tag() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert 'git/refs/tags/${tag}' in workflow
    assert '-f sha="$GITHUB_SHA" -F force=true' in workflow
    assert 'gh release edit "$tag"' in workflow
    assert '--target "$GITHUB_SHA"' in workflow
