from __future__ import annotations

import sys
from pathlib import Path

from PySide6.QtWidgets import QApplication

from drpa_client.app.ui.main_window import MainWindow


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName("DRPA Client")
    app.setOrganizationName("drpa")

    theme_path = Path(__file__).resolve().parent / "ui" / "themes" / "dark.qss"
    if theme_path.exists():
        app.setStyleSheet(theme_path.read_text(encoding="utf-8"))

    window = MainWindow()
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
