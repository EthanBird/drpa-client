from __future__ import annotations

from importlib import resources

from PySide6.QtWidgets import QApplication

from drpa_client.core.settings import ThemeName


def apply_theme(app: QApplication, theme: ThemeName) -> None:
    resource = resources.files("drpa_client.app.ui.themes").joinpath(f"{theme}.qss")
    app.setStyleSheet(resource.read_text(encoding="utf-8"))
