from __future__ import annotations

import sys

from PySide6.QtWidgets import QApplication

from drpa_client.app.ui.main_window import MainWindow
from drpa_client.app.ui.theme import apply_theme
from drpa_client.core.settings import SettingsStore


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName("DRPA Client")
    app.setOrganizationName("drpa")

    apply_theme(app, SettingsStore().load().theme)

    window = MainWindow()
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
