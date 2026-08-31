from __future__ import annotations

import subprocess
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "installer/windows/drpa-next.nsi"
ICON = ROOT / "apps/desktop/src-tauri/icons/icon.ico"


def find_makensis() -> Path | None:
    candidates = (
        Path(r"C:\Program Files (x86)\NSIS\makensis.exe"),
        Path(r"C:\Program Files\NSIS\makensis.exe"),
        ROOT / "target/packaging-tools/nsis-3.12.0/portable/makensis.exe",
    )
    return next((path for path in candidates if path.is_file()), None)


@pytest.mark.skipif(find_makensis() is None, reason="NSIS compiler is not available")
def test_modular_installer_compiles_without_registry(tmp_path: Path) -> None:
    stage = tmp_path / "stage"
    stage.mkdir(parents=True)
    (stage / "DRPA Next.exe").write_bytes(b"launcher")
    (stage / "DRPA Component Installer.exe").write_bytes(b"egui wizard")
    (stage / "drpa.exe").write_bytes(b"core")
    (stage / "core-files.json").write_text('{"schema":1,"files":[]}', encoding="utf-8")
    output = tmp_path / "setup.exe"
    completed = subprocess.run(
        [
            str(find_makensis()),
            "/INPUTCHARSET",
            "UTF8",
            f"/DPAYLOAD_DIR={stage}",
            f"/DOUTPUT_FILE={output}",
            f"/DICON_FILE={ICON}",
            str(SCRIPT),
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    assert output.is_file()
    assert output.stat().st_size > 0
    assert output.stat().st_size < 1024 * 1024
