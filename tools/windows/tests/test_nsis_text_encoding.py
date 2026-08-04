from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
INSTALLER = ROOT / "installer" / "windows" / "drpa-next.nsi"


def test_nsis_source_is_utf8_and_contains_readable_chinese_copy() -> None:
    raw = INSTALLER.read_bytes()
    text = raw.decode("utf-8")

    assert "Unicode true" in text
    assert "启动 DRPA Next" in text
    assert "请选择非系统盘上的安装目录" in text
    assert "卸载 DRPA Next.exe" in text
    assert "工作区数据不能安装到 Windows 系统盘" in text

    mojibake_fragments = ("鍚", "璇", "绯荤", "锛", "銆", "路 Offline")
    assert not any(fragment in text for fragment in mojibake_fragments)


def test_nsis_uninstaller_removes_all_packaged_dependency_directories() -> None:
    text = INSTALLER.read_text(encoding="utf-8")

    for directory in ("runtime", "webview2", "examples", "jcode"):
        assert f'RMDir /r "$INSTDIR\\{directory}"' in text

    assert 'Delete "$INSTDIR\\drpa-updater.exe"' not in text
