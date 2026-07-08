from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from PySide6.QtCore import QDate, QObject, Qt, QThread, Signal
from PySide6.QtGui import QFont
from PySide6.QtWidgets import (
    QAbstractItemView,
    QCheckBox,
    QComboBox,
    QDateEdit,
    QDoubleSpinBox,
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
    QSpinBox,
    QStackedWidget,
    QTableWidget,
    QTableWidgetItem,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from drpa_client.core.database import RunStore
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
        self.run_store = RunStore()
        self.task_runner = TaskRunner(run_store=self.run_store)

        root = QWidget()
        root_layout = QHBoxLayout(root)
        root_layout.setContentsMargins(0, 0, 0, 0)
        root_layout.setSpacing(0)

        self.sidebar = Sidebar()
        self.stack = QStackedWidget()

        self.dashboard_page = DashboardPage(self.package_manager, self.run_store)
        self.packages_page = PackagesPage(self.package_manager)
        self.tasks_page = TasksPage(self.package_manager, self.task_runner)
        self.history_page = HistoryPage(self.run_store)
        self.settings_page = SettingsPage()

        for page in (
            self.dashboard_page,
            self.packages_page,
            self.tasks_page,
            self.history_page,
            self.settings_page,
        ):
            self.stack.addWidget(page)

        root_layout.addWidget(self.sidebar)
        root_layout.addWidget(self.stack, 1)
        self.setCentralWidget(root)

        self.sidebar.currentRowChanged.connect(self._change_page)
        self.packages_page.packages_changed.connect(self._refresh_all)
        self.tasks_page.run_finished.connect(self._refresh_history)
        self.sidebar.setCurrentRow(0)

    def _change_page(self, index: int) -> None:
        self.stack.setCurrentIndex(index)
        if index == 0:
            self.dashboard_page.refresh()
        elif index == 2:
            self.tasks_page.refresh_packages()
        elif index == 3:
            self.history_page.refresh()

    def _refresh_all(self) -> None:
        self.dashboard_page.refresh()
        self.tasks_page.refresh_packages()
        self.history_page.refresh()

    def _refresh_history(self) -> None:
        self.dashboard_page.refresh()
        self.history_page.refresh()


class Sidebar(QListWidget):
    def __init__(self):
        super().__init__()
        self.setObjectName("Sidebar")
        self.setFixedWidth(220)
        self.setFrameShape(QFrame.Shape.NoFrame)
        self.setSpacing(8)
        for title in ("首页", "脚本包", "运行任务", "运行历史", "设置"):
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
    def __init__(self, package_manager: PackageManager, run_store: RunStore):
        super().__init__("DRPA Client", "轻量级 Python RPA Worker，支持脚本包安装、依赖隔离和跨平台运行。")
        self.package_manager = package_manager
        self.run_store = run_store
        self.package_stats = QLabel()
        self.package_stats.setObjectName("HeroNumber")
        self.run_stats = QLabel()
        self.run_stats.setObjectName("HeroNumber")
        self.health_stats = QLabel()

        card = Card()
        stats_row = QHBoxLayout()
        package_box = QVBoxLayout()
        package_box.addWidget(QLabel("已安装脚本包"))
        package_box.addWidget(self.package_stats)
        run_box = QVBoxLayout()
        run_box.addWidget(QLabel("历史运行次数"))
        run_box.addWidget(self.run_stats)
        stats_row.addLayout(package_box)
        stats_row.addLayout(run_box)
        stats_row.addStretch(1)
        card.layout.addLayout(stats_row)
        card.layout.addWidget(self.health_stats)
        card.layout.addWidget(
            QLabel(
                "当前版本已具备脚本包安装、独立 venv、离线 wheels、运行历史、进程树停止和 PySide6 桌面框架。"
            )
        )
        self.content.addWidget(card)
        self.content.addStretch(1)
        self.refresh()

    def refresh(self) -> None:
        package_count = len(self.package_manager.list_installed())
        total_runs = len(self.run_store.list_runs(limit=10000))
        failed_runs = self.run_store.count_by_status(["failed"])
        success_runs = self.run_store.count_by_status(["success"])
        self.package_stats.setText(str(package_count))
        self.run_stats.setText(str(total_runs))
        self.health_stats.setText(f"成功 {success_runs} 次 / 失败 {failed_runs} 次")


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
        self.rebuild_button = QPushButton("重建环境")
        self.uninstall_button = QPushButton("卸载")
        self.install_dependencies = QCheckBox("安装依赖")
        self.install_dependencies.setChecked(True)
        toolbar.addWidget(self.install_button)
        toolbar.addWidget(self.rebuild_button)
        toolbar.addWidget(self.uninstall_button)
        toolbar.addWidget(self.install_dependencies)
        toolbar.addStretch(1)

        self.table = QTableWidget(0, 5)
        self.table.setHorizontalHeaderLabels(["名称", "ID", "版本", "Runtime", "目录"])
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
        self.rebuild_button.clicked.connect(self._rebuild_selected)
        self.uninstall_button.clicked.connect(self._uninstall_selected)
        self.refresh()

    def refresh(self) -> None:
        packages = self.package_manager.list_installed()
        self.table.setRowCount(len(packages))
        for row, package in enumerate(packages):
            values = [
                package.manifest.name,
                package.manifest.id,
                package.manifest.version,
                package.manifest.runtime.isolation,
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
        self.rebuild_button.setEnabled(False)
        self.uninstall_button.setEnabled(False)
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
        self.rebuild_button.setEnabled(True)
        self.uninstall_button.setEnabled(True)
        self.install_log.append(f"安装完成：{package_name}")
        self.refresh()
        self.packages_changed.emit()

    def _install_failed(self, message: str) -> None:
        self.install_button.setEnabled(True)
        self.rebuild_button.setEnabled(True)
        self.uninstall_button.setEnabled(True)
        self.install_log.append(f"安装失败：{message}")
        QMessageBox.critical(self, "安装失败", message)

    def _selected_package(self) -> InstalledPackage | None:
        row = self.table.currentRow()
        packages = self.package_manager.list_installed()
        if row < 0 or row >= len(packages):
            return None
        return packages[row]

    def _rebuild_selected(self) -> None:
        package = self._selected_package()
        if package is None:
            QMessageBox.information(self, "请选择脚本包", "请先在表格中选择一个脚本包。")
            return
        self.install_button.setEnabled(False)
        self.rebuild_button.setEnabled(False)
        self.uninstall_button.setEnabled(False)
        self.install_log.append(f"开始重建环境：{package.display_name}")
        self.worker_thread = QThread()
        self.worker = PackageInstallWorker(
            self.package_manager,
            package=package,
            install_dependencies=self.install_dependencies.isChecked(),
            mode="rebuild",
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

    def _uninstall_selected(self) -> None:
        package = self._selected_package()
        if package is None:
            QMessageBox.information(self, "请选择脚本包", "请先在表格中选择一个脚本包。")
            return
        answer = QMessageBox.question(
            self,
            "确认卸载",
            f"确定卸载 {package.display_name}？这会删除脚本包目录和虚拟环境。",
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        self.package_manager.uninstall(package)
        self.install_log.append(f"已卸载：{package.display_name}")
        self.refresh()
        self.packages_changed.emit()


class PackageInstallWorker(QObject):
    message = Signal(str)
    finished = Signal(str)
    failed = Signal(str)

    def __init__(
        self,
        manager: PackageManager,
        path: Path | None = None,
        install_dependencies: bool = True,
        package: InstalledPackage | None = None,
        mode: str = "install",
    ):
        super().__init__()
        self.manager = manager
        self.path = path
        self.install_dependencies = install_dependencies
        self.package = package
        self.mode = mode

    def run(self) -> None:
        try:
            if self.mode == "rebuild":
                if self.package is None:
                    raise RuntimeError("重建环境缺少脚本包")
                package = self.manager.rebuild_environment(
                    self.package,
                    self.install_dependencies,
                    log=self.message.emit,
                )
            else:
                if self.path is None:
                    raise RuntimeError("安装缺少脚本包路径")
                self.message.emit("正在解压和校验 manifest...")
                package = self.manager.install_archive(
                    self.path,
                    self.install_dependencies,
                    log=self.message.emit,
                )
            self.finished.emit(package.display_name)
        except Exception as exc:  # noqa: BLE001 - surfaced to UI
            self.failed.emit(str(exc))


class TasksPage(Page):
    run_finished = Signal()

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
            elif param.type == "integer":
                widget = QSpinBox()
                widget.setRange(-2_147_483_648, 2_147_483_647)
                if param.default is not None:
                    widget.setValue(int(param.default))
            elif param.type == "number":
                widget = QDoubleSpinBox()
                widget.setRange(-1_000_000_000, 1_000_000_000)
                widget.setDecimals(4)
                if param.default is not None:
                    widget.setValue(float(param.default))
            elif param.type == "date":
                widget = QDateEdit()
                widget.setCalendarPopup(True)
                widget.setDisplayFormat("yyyy-MM-dd")
                if param.default:
                    widget.setDate(QDate.fromString(str(param.default), "yyyy-MM-dd"))
                else:
                    widget.setDate(QDate.currentDate())
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
            elif isinstance(widget, QSpinBox):
                params[name] = widget.value()
            elif isinstance(widget, QDoubleSpinBox):
                params[name] = widget.value()
            elif isinstance(widget, QDateEdit):
                params[name] = widget.date().toString("yyyy-MM-dd")
            elif isinstance(widget, QLineEdit):
                params[name] = widget.text()
        return params

    def _validate_params(self, package: InstalledPackage) -> bool:
        params = self._collect_params()
        missing = []
        for param in package.manifest.params:
            if not param.required:
                continue
            value = params.get(param.name)
            if value is None or (isinstance(value, str) and not value.strip()):
                missing.append(param.label)
        if missing:
            QMessageBox.warning(self, "参数不完整", "请填写必填参数：" + "、".join(missing))
            return False
        return True

    def _run(self) -> None:
        package = self._selected_package()
        if package is None:
            QMessageBox.warning(self, "无法运行", "请先安装脚本包")
            return
        if not self._validate_params(package):
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
            self.run_finished.emit()
        else:
            self.log.append(json.dumps(payload, ensure_ascii=False))


class TaskEventBridge(QObject):
    event = Signal(object)

    def emit_event(self, event: TaskEvent) -> None:
        self.event.emit(event)


class HistoryPage(Page):
    def __init__(self, run_store: RunStore):
        super().__init__("运行历史", "查看最近任务运行状态、日志路径和输出目录。")
        self.run_store = run_store

        toolbar = QHBoxLayout()
        self.refresh_button = QPushButton("刷新")
        toolbar.addWidget(self.refresh_button)
        toolbar.addStretch(1)

        self.table = QTableWidget(0, 8)
        self.table.setHorizontalHeaderLabels(
            ["开始时间", "脚本包", "版本", "状态", "退出码", "结束时间", "输出目录", "日志文件"]
        )
        self.table.horizontalHeader().setStretchLastSection(True)
        self.table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)

        card = Card()
        card.layout.addLayout(toolbar)
        card.layout.addWidget(self.table)
        self.content.addWidget(card, 1)

        self.refresh_button.clicked.connect(self.refresh)
        self.refresh()

    def refresh(self) -> None:
        runs = self.run_store.list_runs()
        self.table.setRowCount(len(runs))
        for row, run in enumerate(runs):
            values = [
                run.started_at,
                run.package_name,
                run.package_version,
                run.status,
                "" if run.exit_code is None else str(run.exit_code),
                run.finished_at or "",
                str(run.output_dir),
                str(run.log_file),
            ]
            for column, value in enumerate(values):
                self.table.setItem(row, column, QTableWidgetItem(value))


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
