from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from PySide6.QtCore import QObject, Qt, QThread, Signal
from PySide6.QtGui import QFont
from PySide6.QtWidgets import (
    QAbstractItemView,
    QCheckBox,
    QComboBox,
    QFileDialog,
    QFormLayout,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QStackedWidget,
    QTableWidget,
    QTableWidgetItem,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from drpa_client.core.models import InstalledPackage, TaskEvent
from drpa_client.core.package_manager import PackageManager
from drpa_client.core.paths import get_data_dir
from drpa_client.core.task_runner import RunningTask, TaskRunner


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("DRPA Client")
        self.resize(1180, 760)

        self.package_manager = PackageManager()
        self.task_runner = TaskRunner()

        root = QWidget()
        root_layout = QHBoxLayout(root)
        root_layout.setContentsMargins(0, 0, 0, 0)
        root_layout.setSpacing(0)

        self.sidebar = Sidebar()
        self.stack = QStackedWidget()

        self.dashboard_page = DashboardPage(self.package_manager)
        self.packages_page = PackagesPage(self.package_manager)
        self.tasks_page = TasksPage(self.package_manager, self.task_runner)
        self.settings_page = SettingsPage()

        for page in (
            self.dashboard_page,
            self.packages_page,
            self.tasks_page,
            self.settings_page,
        ):
            self.stack.addWidget(page)

        root_layout.addWidget(self.sidebar)
        root_layout.addWidget(self.stack, 1)
        self.setCentralWidget(root)

        self.sidebar.currentRowChanged.connect(self._change_page)
        self.packages_page.packages_changed.connect(self._refresh_all)
        self.sidebar.setCurrentRow(0)

    def _change_page(self, index: int) -> None:
        self.stack.setCurrentIndex(index)
        if index == 0:
            self.dashboard_page.refresh()
        elif index == 2:
            self.tasks_page.refresh_packages()

    def _refresh_all(self) -> None:
        self.dashboard_page.refresh()
        self.tasks_page.refresh_packages()


class Sidebar(QListWidget):
    def __init__(self):
        super().__init__()
        self.setObjectName("Sidebar")
        self.setFixedWidth(220)
        self.setFrameShape(QFrame.Shape.NoFrame)
        self.setSpacing(8)
        for title in ("首页", "脚本包", "运行任务", "设置"):
            item = QListWidgetItem(title)
            item.setTextAlignment(Qt.AlignmentFlag.AlignVCenter)
            item.setSizeHint(item.sizeHint().expandedTo(item.sizeHint() * 1.6))
            self.addItem(item)


class Page(QWidget):
    def __init__(self, title: str, subtitle: str):
        super().__init__()
        self.content = QVBoxLayout(self)
        self.content.setContentsMargins(32, 28, 32, 28)
        self.content.setSpacing(18)

        title_label = QLabel(title)
        title_label.setObjectName("PageTitle")
        subtitle_label = QLabel(subtitle)
        subtitle_label.setObjectName("PageSubtitle")
        self.content.addWidget(title_label)
        self.content.addWidget(subtitle_label)


class Card(QFrame):
    def __init__(self):
        super().__init__()
        self.setObjectName("Card")
        self.setFrameShape(QFrame.Shape.StyledPanel)
        self.layout = QVBoxLayout(self)
        self.layout.setContentsMargins(22, 20, 22, 20)
        self.layout.setSpacing(12)


class DashboardPage(Page):
    def __init__(self, package_manager: PackageManager):
        super().__init__("DRPA Client", "轻量级 Python RPA Worker，支持脚本包安装、依赖隔离和跨平台运行。")
        self.package_manager = package_manager
        self.stats = QLabel()
        self.stats.setObjectName("HeroNumber")

        card = Card()
        card.layout.addWidget(QLabel("当前状态"))
        card.layout.addWidget(self.stats)
        card.layout.addWidget(
            QLabel(
                "已内置脚本包 manifest、独立 venv、离线 wheels、JSON Lines 任务事件和 PySide6 桌面框架。"
            )
        )
        self.content.addWidget(card)
        self.content.addStretch(1)
        self.refresh()

    def refresh(self) -> None:
        count = len(self.package_manager.list_installed())
        self.stats.setText(f"{count} 个脚本包已安装")


class PackagesPage(Page):
    packages_changed = Signal()

    def __init__(self, package_manager: PackageManager):
        super().__init__("脚本包", "导入 .rpaz/.zip 脚本包，支持 manifest、requirements 和包内 wheels。")
        self.package_manager = package_manager
        self.worker: PackageInstallWorker | None = None
        self.worker_thread: QThread | None = None

        toolbar = QHBoxLayout()
        self.install_button = QPushButton("安装脚本包")
        self.install_button.setObjectName("PrimaryButton")
        self.install_dependencies = QCheckBox("安装依赖")
        self.install_dependencies.setChecked(True)
        toolbar.addWidget(self.install_button)
        toolbar.addWidget(self.install_dependencies)
        toolbar.addStretch(1)

        self.table = QTableWidget(0, 4)
        self.table.setHorizontalHeaderLabels(["名称", "ID", "版本", "目录"])
        self.table.horizontalHeader().setStretchLastSection(True)
        self.table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)

        self.install_log = QTextEdit()
        self.install_log.setReadOnly(True)
        self.install_log.setPlaceholderText("安装日志会显示在这里")

        card = Card()
        card.layout.addLayout(toolbar)
        card.layout.addWidget(self.table)
        card.layout.addWidget(self.install_log)
        self.content.addWidget(card, 1)

        self.install_button.clicked.connect(self._choose_package)
        self.refresh()

    def refresh(self) -> None:
        packages = self.package_manager.list_installed()
        self.table.setRowCount(len(packages))
        for row, package in enumerate(packages):
            values = [
                package.manifest.name,
                package.manifest.id,
                package.manifest.version,
                str(package.root_dir),
            ]
            for column, value in enumerate(values):
                self.table.setItem(row, column, QTableWidgetItem(value))

    def _choose_package(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self,
            "选择脚本包",
            str(Path.home()),
            "RPA Packages (*.rpaz *.zip)",
        )
        if not path:
            return
        self._install_package(Path(path))

    def _install_package(self, path: Path) -> None:
        self.install_button.setEnabled(False)
        self.install_log.append(f"开始安装：{path}")
        self.worker_thread = QThread()
        self.worker = PackageInstallWorker(
            self.package_manager,
            path,
            self.install_dependencies.isChecked(),
        )
        self.worker.moveToThread(self.worker_thread)
        self.worker_thread.started.connect(self.worker.run)
        self.worker.message.connect(self.install_log.append)
        self.worker.finished.connect(self._install_finished)
        self.worker.failed.connect(self._install_failed)
        self.worker.finished.connect(self.worker_thread.quit)
        self.worker.failed.connect(self.worker_thread.quit)
        self.worker_thread.finished.connect(self.worker.deleteLater)
        self.worker_thread.finished.connect(self.worker_thread.deleteLater)
        self.worker_thread.start()

    def _install_finished(self, package_name: str) -> None:
        self.install_button.setEnabled(True)
        self.install_log.append(f"安装完成：{package_name}")
        self.refresh()
        self.packages_changed.emit()

    def _install_failed(self, message: str) -> None:
        self.install_button.setEnabled(True)
        self.install_log.append(f"安装失败：{message}")
        QMessageBox.critical(self, "安装失败", message)


class PackageInstallWorker(QObject):
    message = Signal(str)
    finished = Signal(str)
    failed = Signal(str)

    def __init__(self, manager: PackageManager, path: Path, install_dependencies: bool):
        super().__init__()
        self.manager = manager
        self.path = path
        self.install_dependencies = install_dependencies

    def run(self) -> None:
        try:
            self.message.emit("正在解压和校验 manifest...")
            package = self.manager.install_archive(self.path, self.install_dependencies)
            self.finished.emit(package.display_name)
        except Exception as exc:  # noqa: BLE001 - surfaced to UI
            self.failed.emit(str(exc))


class TasksPage(Page):
    def __init__(self, package_manager: PackageManager, task_runner: TaskRunner):
        super().__init__("运行任务", "选择脚本包、填写参数并启动 RPA，实时查看结构化日志。")
        self.package_manager = package_manager
        self.task_runner = task_runner
        self.packages: list[InstalledPackage] = []
        self.current_task: RunningTask | None = None
        self.event_bridge = TaskEventBridge()
        self.param_widgets: dict[str, QWidget] = {}

        card = Card()
        self.package_combo = QComboBox()
        self.form = QFormLayout()
        self.form.setLabelAlignment(Qt.AlignmentFlag.AlignRight)

        self.run_button = QPushButton("运行")
        self.run_button.setObjectName("PrimaryButton")
        self.stop_button = QPushButton("停止")
        self.stop_button.setEnabled(False)

        buttons = QHBoxLayout()
        buttons.addWidget(self.run_button)
        buttons.addWidget(self.stop_button)
        buttons.addStretch(1)

        self.log = QTextEdit()
        self.log.setReadOnly(True)
        self.log.setMinimumHeight(260)

        card.layout.addWidget(QLabel("脚本包"))
        card.layout.addWidget(self.package_combo)
        card.layout.addLayout(self.form)
        card.layout.addLayout(buttons)
        card.layout.addWidget(self.log)
        self.content.addWidget(card, 1)

        self.package_combo.currentIndexChanged.connect(self._render_params)
        self.run_button.clicked.connect(self._run)
        self.stop_button.clicked.connect(self._stop)
        self.event_bridge.event.connect(self._handle_event)
        self.refresh_packages()

    def refresh_packages(self) -> None:
        self.packages = self.package_manager.list_installed()
        self.package_combo.blockSignals(True)
        self.package_combo.clear()
        for package in self.packages:
            self.package_combo.addItem(package.display_name, package.manifest.id)
        self.package_combo.blockSignals(False)
        self._render_params()

    def _render_params(self) -> None:
        while self.form.rowCount():
            self.form.removeRow(0)
        self.param_widgets.clear()
        package = self._selected_package()
        if package is None:
            self.form.addRow(QLabel("尚未安装脚本包"))
            return

        for param in package.manifest.params:
            if param.type == "boolean":
                widget: QWidget = QCheckBox()
                if param.default is not None:
                    widget.setChecked(bool(param.default))
            else:
                widget = QLineEdit()
                if param.type == "password":
                    widget.setEchoMode(QLineEdit.EchoMode.Password)
                if param.default is not None:
                    widget.setText(str(param.default))
            widget.setToolTip(param.description)
            self.param_widgets[param.name] = widget
            label = f"{param.label}{' *' if param.required else ''}"
            self.form.addRow(label, widget)

    def _selected_package(self) -> InstalledPackage | None:
        index = self.package_combo.currentIndex()
        if index < 0 or index >= len(self.packages):
            return None
        return self.packages[index]

    def _collect_params(self) -> dict[str, Any]:
        params: dict[str, Any] = {}
        for name, widget in self.param_widgets.items():
            if isinstance(widget, QCheckBox):
                params[name] = widget.isChecked()
            elif isinstance(widget, QLineEdit):
                params[name] = widget.text()
        return params

    def _run(self) -> None:
        package = self._selected_package()
        if package is None:
            QMessageBox.warning(self, "无法运行", "请先安装脚本包")
            return
        self.log.clear()
        self.run_button.setEnabled(False)
        self.stop_button.setEnabled(True)
        self.current_task = self.task_runner.start(
            package,
            self._collect_params(),
            self.event_bridge.emit_event,
        )

    def _stop(self) -> None:
        if self.current_task is not None:
            self.current_task.stop()
            self.log.append("[status] 已发送停止信号")

    def _handle_event(self, event: TaskEvent) -> None:
        payload = event.payload
        if event.type == "log":
            self.log.append(f"[{payload.get('level', 'info')}] {payload.get('message', '')}")
        elif event.type == "progress":
            self.log.append(f"[progress] {payload.get('value')}% {payload.get('message', '')}")
        elif event.type == "artifact":
            self.log.append(f"[artifact] {payload.get('label', '')}: {payload.get('path', '')}")
        elif event.type == "error":
            self.log.append(f"[error] {payload.get('message', '')}")
            if payload.get("traceback"):
                self.log.append(str(payload["traceback"]))
        elif event.type == "status":
            self.log.append(f"[status] {payload.get('message', payload.get('value', ''))}")
        elif event.type == "finished":
            self.log.append(f"[finished] exit_code={payload.get('exit_code')}")
            self.run_button.setEnabled(True)
            self.stop_button.setEnabled(False)
            self.current_task = None
        else:
            self.log.append(json.dumps(payload, ensure_ascii=False))


class TaskEventBridge(QObject):
    event = Signal(object)

    def emit_event(self, event: TaskEvent) -> None:
        self.event.emit(event)


class SettingsPage(Page):
    def __init__(self):
        super().__init__("设置", "运行目录、浏览器路径和未来的打包策略会集中放在这里。")
        card = Card()
        data_dir = QLabel(str(get_data_dir()))
        data_dir.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        data_dir.setFont(QFont("monospace"))
        card.layout.addWidget(QLabel("数据目录"))
        card.layout.addWidget(data_dir)
        card.layout.addWidget(QLabel("下一步会增加：Chrome/Edge 路径检测、主题切换、脚本仓库配置。"))
        self.content.addWidget(card)
        self.content.addStretch(1)
