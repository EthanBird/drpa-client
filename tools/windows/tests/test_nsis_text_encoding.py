from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
INSTALLER = ROOT / "installer" / "windows" / "drpa-next.nsi"


def test_nsis_source_is_utf8_and_contains_readable_chinese_copy() -> None:
    raw = INSTALLER.read_bytes()
    text = raw.decode("utf-8")

    assert "Unicode true" in text
    assert "打开组件安装向导" in text
    assert "请选择非系统盘上的安装目录" in text
    assert "卸载 DRPA Next.exe" in text
    assert "工作区数据不能安装到 Windows 系统盘" in text

    mojibake_fragments = ("鍚", "璇", "绯荤", "锛", "銆", "路 Offline")
    assert not any(fragment in text for fragment in mojibake_fragments)


def test_nsis_uninstaller_removes_all_packaged_dependency_directories() -> None:
    text = INSTALLER.read_text(encoding="utf-8")

    for directory in ("runtime", "webview2", "examples", "jcode", "components", "component-packs", "state"):
        assert f'RMDir /r "$INSTDIR\\{directory}"' in text

    assert 'Delete "$INSTDIR\\drpa-updater.exe"' not in text


def test_nsis_contains_only_core_and_launches_native_component_installer() -> None:
    text = INSTALLER.read_text(encoding="utf-8")

    assert 'Section "DRPA Core（命令行、启动器与组件向导）"' in text
    assert "SectionIn RO" in text
    assert "component install" not in text
    assert "component-packs\\org.drpa" not in text
    assert "WriteReg" not in text
    assert '"$PLUGINSDIR\\drpa.exe" install locate' in text
    assert 'install reconcile-core "$INSTDIR\\core-files.json"' in text
    assert '!define MUI_FINISHPAGE_RUN "$INSTDIR\\DRPA Component Installer.exe"' in text
    assert '--scan $\\"$EXEDIR$\\"' in text
    assert "SetCompressor /SOLID" not in text
