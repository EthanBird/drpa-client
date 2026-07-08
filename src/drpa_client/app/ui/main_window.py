from __future__ import annotations

import json
from importlib import resources
from pathlib import Path
from typing import Any

from PySide6.QtCore import QDate, QObject, Qt, QThread, QTimer, QUrl, Signal
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

try:
    from PySide6.QtWebEngineWidgets import QWebEngineView
except ImportError:  # pragma: no cover - depends on optional QtWebEngine runtime
    QWebEngineView = None

from drpa_client.core.database import RunStore
from drpa_client.core.default_packages import default_package_path
from drpa_client.core.models import InstalledPackage, TaskEvent
from drpa_client.core.package_manager import PackageManager
from drpa_client.core.paths import get_data_dir
from drpa_client.core.recorder import BrowserRecorderSession, RecorderPackageGenerator, Recording
from drpa_client.core.runtime_manager import RuntimeManager
from drpa_client.core.settings import AppSettings, SettingsStore
from drpa_client.core.task_runner import RunningTask, TaskRunner


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("DRPA Client")
        self.resize(1180, 760)

        self.settings_store = SettingsStore()
        self.package_manager = PackageManager(settings_store=self.settings_store)
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
        self.editor_page = CodeEditorPage()
        self.recorder_page = RecorderPage()
        self.history_page = HistoryPage(self.run_store)
        self.settings_page = SettingsPage(self.settings_store)

        root_layout.addWidget(self.sidebar)
        root_layout.addWidget(self.stack, 1)
        self.setCentralWidget(root)

        self.sidebar.currentRowChanged.connect(self._change_page)
        self.packages_page.packages_changed.connect(self._refresh_all)
        self.tasks_page.run_finished.connect(self._refresh_history)
        self.settings_page.settings_changed.connect(self._rebuild_navigation)
        self._rebuild_navigation()
        self.sidebar.setCurrentRow(0)

    def _rebuild_navigation(self) -> None:
        current = self.stack.currentWidget()
        pages: list[tuple[str, QWidget]] = [
            ("首页", self.dashboard_page),
            ("脚本包", self.packages_page),
            ("运行任务", self.tasks_page),
            ("代码编辑", self.editor_page),
        ]
        if self.settings_store.load().advanced_recorder_enabled:
            pages.append(("浏览器录制", self.recorder_page))
        pages.extend(
            [
                ("运行历史", self.history_page),
                ("设置", self.settings_page),
            ]
        )

        self.sidebar.blockSignals(True)
        self.stack.blockSignals(True)
        self.sidebar.set_titles([title for title, _ in pages])
        while self.stack.count():
            widget = self.stack.widget(0)
            self.stack.removeWidget(widget)
        selected_index = 0
        for index, (_, page) in enumerate(pages):
            self.stack.addWidget(page)
            if page is current:
                selected_index = index
        self.stack.blockSignals(False)
        self.sidebar.blockSignals(False)
        self.sidebar.setCurrentRow(selected_index)
        self.stack.setCurrentIndex(selected_index)

    def _change_page(self, index: int) -> None:
        self.stack.setCurrentIndex(index)
        page = self.stack.currentWidget()
        if page is self.dashboard_page:
            self.dashboard_page.refresh()
        elif page is self.tasks_page:
            self.tasks_page.refresh_packages()
        elif page is self.history_page:
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
        self.set_titles([])

    def set_titles(self, titles: list[str]) -> None:
        self.clear()
        for title in titles:
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
        super().__init__("DRPA 工作台", "管理脚本包、运行任务、录制浏览器流程，并用 Monaco 编辑 Python RPA 脚本。")
        self.package_manager = package_manager
        self.run_store = run_store
        self.package_stats = QLabel()
        self.package_stats.setObjectName("HeroNumber")
        self.run_stats = QLabel()
        self.run_stats.setObjectName("HeroNumber")
        self.success_stats = QLabel()
        self.success_stats.setObjectName("HeroNumber")
        self.health_stats = QLabel()
        self.recent_runs = QTableWidget(0, 4)
        self.recent_runs.setHorizontalHeaderLabels(["时间", "脚本包", "状态", "退出码"])
        self.recent_runs.horizontalHeader().setStretchLastSection(True)
        self.recent_runs.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)

        hero = Card()
        stats_row = QHBoxLayout()
        package_box = QVBoxLayout()
        package_box.addWidget(QLabel("已安装脚本包"))
        package_box.addWidget(self.package_stats)
        run_box = QVBoxLayout()
        run_box.addWidget(QLabel("历史运行次数"))
        run_box.addWidget(self.run_stats)
        success_box = QVBoxLayout()
        success_box.addWidget(QLabel("成功运行"))
        success_box.addWidget(self.success_stats)
        stats_row.addLayout(package_box)
        stats_row.addLayout(run_box)
        stats_row.addLayout(success_box)
        stats_row.addStretch(1)
        hero.layout.addLayout(stats_row)
        hero.layout.addWidget(self.health_stats)
        hero.layout.addWidget(
            QLabel(
                "建议流程：安装脚本包 -> 在代码编辑器中修订脚本 -> 运行任务 -> 查看历史和输出产物。"
            )
        )

        quick = Card()
        quick.layout.addWidget(QLabel("快捷开始"))
        quick_row = QHBoxLayout()
        for title, desc in (
            ("安装默认包", "导入 Bing 每日一图示例，验证运行链路。"),
            ("编辑脚本", "使用 Monaco Editor 修改 main.py / manifest.yaml。"),
            ("浏览器录制", "录制操作并生成需要人工修订的草稿包。"),
            ("构建 rpaz", "按 skill 规范打包 Python RPA 脚本包。"),
        ):
            box = QVBoxLayout()
            label = QLabel(title)
            label.setObjectName("SectionTitle")
            text = QLabel(desc)
            text.setObjectName("MutedText")
            text.setWordWrap(True)
            box.addWidget(label)
            box.addWidget(text)
            quick_row.addLayout(box)
        quick.layout.addLayout(quick_row)

        map_card = Card()
        map_card.layout.addWidget(QLabel("能力地图"))
        for item in (
            "脚本包管理：安装、卸载、重建 venv、离线 wheels",
            "运行任务：列表选择、参数表单、实时日志、进程树停止",
            "代码编辑：Monaco Editor Web 编辑器，参考 VS Code 体验",
            "浏览器录制：JS Agent 捕获事件，生成可审查草稿 rpaz",
            "运行历史：SQLite 记录状态、日志和输出目录",
        ):
            line = QLabel("• " + item)
            line.setObjectName("MutedText")
            map_card.layout.addWidget(line)

        recent = Card()
        recent.layout.addWidget(QLabel("最近运行"))
        recent.layout.addWidget(self.recent_runs)

        self.content.addWidget(hero)
        self.content.addWidget(quick)
        self.content.addWidget(map_card)
        self.content.addWidget(recent, 1)
        self.refresh()

    def refresh(self) -> None:
        package_count = len(self.package_manager.list_installed())
        total_runs = len(self.run_store.list_runs(limit=10000))
        failed_runs = self.run_store.count_by_status(["failed"])
        success_runs = self.run_store.count_by_status(["success"])
        self.package_stats.setText(str(package_count))
        self.run_stats.setText(str(total_runs))
        self.success_stats.setText(str(success_runs))
        self.health_stats.setText(f"成功 {success_runs} 次 / 失败 {failed_runs} 次")
        runs = self.run_store.list_runs(limit=8)
        self.recent_runs.setRowCount(len(runs))
        for row, run in enumerate(runs):
            values = [
                run.started_at,
                run.package_name,
                run.status,
                "" if run.exit_code is None else str(run.exit_code),
            ]
            for column, value in enumerate(values):
                self.recent_runs.setItem(row, column, QTableWidgetItem(value))


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
        self.install_default_button = QPushButton("安装默认 Bing 每日一图")
        self.rebuild_button = QPushButton("重建环境")
        self.uninstall_button = QPushButton("卸载")
        self.install_dependencies = QCheckBox("安装依赖")
        self.install_dependencies.setChecked(True)
        toolbar.addWidget(self.install_button)
        toolbar.addWidget(self.install_default_button)
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
        self.install_default_button.clicked.connect(self._install_default_bing_package)
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
            "RPA Packages or Python Files (*.rpaz *.zip *.py)",
        )
        if not path:
            return
        self._install_package(Path(path))

    def _install_package(self, path: Path) -> None:
        self.install_button.setEnabled(False)
        self.install_default_button.setEnabled(False)
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
        self.install_default_button.setEnabled(True)
        self.rebuild_button.setEnabled(True)
        self.uninstall_button.setEnabled(True)
        self.install_log.append(f"安装完成：{package_name}")
        self.refresh()
        self.packages_changed.emit()

    def _install_failed(self, message: str) -> None:
        self.install_button.setEnabled(True)
        self.install_default_button.setEnabled(True)
        self.rebuild_button.setEnabled(True)
        self.uninstall_button.setEnabled(True)
        self.install_log.append(f"安装失败：{message}")
        QMessageBox.critical(self, "安装失败", message)

    def _install_default_bing_package(self) -> None:
        package_path = default_package_path("bing_daily_image.rpaz")
        if not package_path.exists():
            QMessageBox.critical(self, "默认包缺失", f"找不到默认脚本包：{package_path}")
            return
        self._install_package(package_path)

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
        self.install_default_button.setEnabled(False)
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
                if self.path.suffix.lower() == ".py":
                    self.message.emit("正在从单个 Python 文件生成脚本包...")
                    package = self.manager.install_python_file(
                        self.path,
                        install_dependencies=False,
                        log=self.message.emit,
                    )
                else:
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
        body = QHBoxLayout()
        body.setSpacing(18)

        self.package_list = QListWidget()
        self.package_list.setObjectName("PackageList")
        self.package_list.setMinimumWidth(300)
        self.package_list.setMaximumWidth(360)

        left_panel = QVBoxLayout()
        list_title = QLabel("可运行脚本包")
        list_title.setObjectName("SectionTitle")
        self.package_hint = QLabel("选择一个脚本包后，右侧会显示参数和运行日志。")
        self.package_hint.setObjectName("MutedText")
        self.refresh_button = QPushButton("刷新列表")
        left_panel.addWidget(list_title)
        left_panel.addWidget(self.package_hint)
        left_panel.addWidget(self.package_list, 1)
        left_panel.addWidget(self.refresh_button)

        right_panel = QVBoxLayout()
        self.selected_title = QLabel("尚未选择脚本包")
        self.selected_title.setObjectName("SectionTitle")
        self.selected_description = QLabel("请从左侧列表选择要运行的 RPA 脚本包。")
        self.selected_description.setObjectName("MutedText")
        self.selected_description.setWordWrap(True)
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

        right_panel.addWidget(self.selected_title)
        right_panel.addWidget(self.selected_description)
        right_panel.addLayout(self.form)
        right_panel.addLayout(buttons)
        right_panel.addWidget(self.log, 1)

        body.addLayout(left_panel)
        body.addLayout(right_panel, 1)
        card.layout.addLayout(body)
        self.content.addWidget(card, 1)

        self.package_list.currentRowChanged.connect(self._render_params)
        self.refresh_button.clicked.connect(self.refresh_packages)
        self.run_button.clicked.connect(self._run)
        self.stop_button.clicked.connect(self._stop)
        self.event_bridge.event.connect(self._handle_event)
        self.refresh_packages()

    def refresh_packages(self) -> None:
        self.packages = self.package_manager.list_installed()
        current_row = self.package_list.currentRow()
        self.package_list.blockSignals(True)
        self.package_list.clear()
        for package in self.packages:
            item = QListWidgetItem(f"{package.manifest.name}\n{package.manifest.id} · {package.manifest.version}")
            item.setToolTip(package.manifest.description or package.display_name)
            item.setData(Qt.ItemDataRole.UserRole, package.manifest.id)
            self.package_list.addItem(item)
        self.package_list.blockSignals(False)
        if self.packages:
            self.package_list.setCurrentRow(min(max(current_row, 0), len(self.packages) - 1))
        self._render_params()

    def _render_params(self) -> None:
        while self.form.rowCount():
            self.form.removeRow(0)
        self.param_widgets.clear()
        package = self._selected_package()
        if package is None:
            self.selected_title.setText("尚未选择脚本包")
            self.selected_description.setText("请先在左侧列表选择要运行的 RPA 脚本包。")
            self.form.addRow(QLabel("尚未安装脚本包"))
            return

        self.selected_title.setText(package.display_name)
        description = package.manifest.description or "这个脚本包没有填写描述。"
        self.selected_description.setText(
            f"{description}\nID: {package.manifest.id} · Runtime: {package.manifest.runtime.isolation}"
        )

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
        index = self.package_list.currentRow()
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


class CodeEditorPage(Page):
    def __init__(self):
        super().__init__("代码编辑", "基于 Monaco Editor 的 Web 代码编辑器，参考 VS Code，不使用原生文本控件实现。")
        self.current_file: Path | None = None
        self.web_view = None

        card = Card()
        toolbar = QHBoxLayout()
        self.open_button = QPushButton("打开文件")
        self.save_button = QPushButton("保存")
        self.save_as_button = QPushButton("另存为")
        self.path_label = QLabel("尚未打开文件")
        self.path_label.setObjectName("MutedText")
        toolbar.addWidget(self.open_button)
        toolbar.addWidget(self.save_button)
        toolbar.addWidget(self.save_as_button)
        toolbar.addWidget(self.path_label, 1)

        card.layout.addLayout(toolbar)

        if QWebEngineView is None:
            info = QLabel(
                "当前运行环境缺少 QtWebEngine，无法加载 Monaco Editor。\n"
                "代码编辑器设计参考 VS Code/Monaco，不回退到原生 QTextEdit。\n"
                "产品化安装包需要包含 PySide6 QtWebEngine 组件，或内置 Monaco 静态资源。"
            )
            info.setWordWrap(True)
            info.setObjectName("MutedText")
            card.layout.addWidget(info, 1)
        else:
            self.web_view = QWebEngineView()
            self.web_view.setMinimumHeight(520)
            html = resources.files("drpa_client.resources.editor").joinpath("monaco.html").read_text(
                encoding="utf-8"
            )
            self.web_view.setHtml(html, QUrl("https://drpa-editor.local/"))
            card.layout.addWidget(self.web_view, 1)

        self.content.addWidget(card, 1)

        self.open_button.clicked.connect(self._open_file)
        self.save_button.clicked.connect(self._save_file)
        self.save_as_button.clicked.connect(self._save_file_as)

    def _open_file(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self,
            "打开代码文件",
            str(Path.home()),
            "Code Files (*.py *.yaml *.yml *.json *.md *.txt);;All Files (*)",
        )
        if not path:
            return
        self.current_file = Path(path)
        content = self.current_file.read_text(encoding="utf-8")
        self.path_label.setText(str(self.current_file))
        self._set_editor_content(content, _language_for_path(self.current_file))

    def _save_file(self) -> None:
        if self.current_file is None:
            self._save_file_as()
            return
        self._get_editor_content(lambda value: self._write_file(self.current_file, value))

    def _save_file_as(self) -> None:
        path, _ = QFileDialog.getSaveFileName(
            self,
            "保存代码文件",
            str(self.current_file or Path.home() / "main.py"),
            "Code Files (*.py *.yaml *.yml *.json *.md *.txt);;All Files (*)",
        )
        if not path:
            return
        self.current_file = Path(path)
        self.path_label.setText(str(self.current_file))
        self._get_editor_content(lambda value: self._write_file(self.current_file, value))

    def _set_editor_content(self, content: str, language: str) -> None:
        if self.web_view is None:
            QMessageBox.information(self, "编辑器不可用", "当前环境缺少 QtWebEngine，无法加载 Monaco Editor。")
            return
        script = f"setEditorValue({json.dumps(content)}, {json.dumps(language)});"
        self.web_view.page().runJavaScript(script)

    def _get_editor_content(self, callback) -> None:
        if self.web_view is None:
            QMessageBox.information(self, "编辑器不可用", "当前环境缺少 QtWebEngine，无法保存 Monaco Editor 内容。")
            return
        self.web_view.page().runJavaScript("getEditorValue();", callback)

    def _write_file(self, path: Path, content: str) -> None:
        path.write_text(content or "", encoding="utf-8")
        self.path_label.setText(str(path))
        QMessageBox.information(self, "保存成功", f"已保存：{path}")


def _language_for_path(path: Path) -> str:
    suffix = path.suffix.lower()
    if suffix == ".py":
        return "python"
    if suffix in {".yaml", ".yml"}:
        return "yaml"
    if suffix == ".json":
        return "json"
    if suffix == ".md":
        return "markdown"
    return "plaintext"


def _recorded_event_label(event) -> str:
    target = event.target or {}
    for key in ("label", "text", "placeholder"):
        value = target.get(key)
        if value:
            return str(value)
    attributes = target.get("attributes") or {}
    for key in ("data-testid", "aria-label", "name", "id"):
        value = attributes.get(key)
        if value:
            return str(value)
    return event.id


class RecorderPage(Page):
    def __init__(self):
        super().__init__("浏览器录制", "录制浏览器操作，生成需要人工修订的 .rpaz 草稿脚本包。")
        self.session: BrowserRecorderSession | None = None
        self.recording: Recording | None = None
        self.poll_timer = QTimer(self)
        self.poll_timer.setInterval(700)
        self.poll_timer.timeout.connect(self._poll_events)

        card = Card()
        form = QFormLayout()
        self.start_url = QLineEdit("https://www.bing.com")
        self.headless = QCheckBox("无头录制")
        self.package_id = QLineEdit()
        self.package_id.setPlaceholderText("可选，例如 recorded_bing_search")
        form.addRow("起始 URL", self.start_url)
        form.addRow("Package ID", self.package_id)
        form.addRow("", self.headless)

        buttons = QHBoxLayout()
        self.start_button = QPushButton("开始录制")
        self.start_button.setObjectName("PrimaryButton")
        self.stop_button = QPushButton("停止并生成草稿包")
        self.stop_button.setEnabled(False)
        self.import_button = QPushButton("从 recording.json 生成")
        buttons.addWidget(self.start_button)
        buttons.addWidget(self.stop_button)
        buttons.addWidget(self.import_button)
        buttons.addStretch(1)

        self.event_table = QTableWidget(0, 4)
        self.event_table.setHorizontalHeaderLabels(["事件", "URL", "目标", "置信度"])
        self.event_table.horizontalHeader().setStretchLastSection(True)
        self.event_table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.event_table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)

        self.log = QTextEdit()
        self.log.setReadOnly(True)
        self.log.setMinimumHeight(180)
        self.log.setPlaceholderText("录制器日志和生成结果会显示在这里。")

        card.layout.addLayout(form)
        card.layout.addLayout(buttons)
        card.layout.addWidget(QLabel("事件时间线"))
        card.layout.addWidget(self.event_table)
        card.layout.addWidget(self.log)
        self.content.addWidget(card, 1)

        self.start_button.clicked.connect(self._start_recording)
        self.stop_button.clicked.connect(self._stop_recording)
        self.import_button.clicked.connect(self._generate_from_file)

    def _start_recording(self) -> None:
        if not self.start_url.text().strip():
            QMessageBox.warning(self, "缺少 URL", "请填写起始 URL。")
            return
        try:
            self.session = BrowserRecorderSession(
                self.start_url.text().strip(),
                headless=self.headless.isChecked(),
            )
            self.session.start()
        except Exception as exc:  # noqa: BLE001 - surfaced to UI
            QMessageBox.critical(self, "录制启动失败", str(exc))
            self.session = None
            return
        self.event_table.setRowCount(0)
        self.log.append("录制已启动，请在浏览器中操作。")
        self.start_button.setEnabled(False)
        self.stop_button.setEnabled(True)
        self.import_button.setEnabled(False)
        self.poll_timer.start()

    def _poll_events(self) -> None:
        if self.session is None:
            return
        events = self.session.poll_events()
        for event in events:
            self._append_event(event.type, event.url, _recorded_event_label(event), event.confidence)

    def _stop_recording(self) -> None:
        if self.session is None:
            return
        self.poll_timer.stop()
        try:
            self.recording = self.session.stop()
            self.session = None
            archive = self._generate_archive(self.recording)
        except Exception as exc:  # noqa: BLE001 - surfaced to UI
            QMessageBox.critical(self, "生成失败", str(exc))
            return
        finally:
            self.start_button.setEnabled(True)
            self.stop_button.setEnabled(False)
            self.import_button.setEnabled(True)
        self.log.append(f"已生成草稿脚本包：{archive}")

    def _generate_from_file(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self,
            "选择 recording.json",
            str(Path.home()),
            "Recording JSON (*.json)",
        )
        if not path:
            return
        try:
            raw = json.loads(Path(path).read_text(encoding="utf-8"))
            self.recording = Recording.from_dict(raw)
            self.event_table.setRowCount(0)
            for event in self.recording.events:
                self._append_event(event.type, event.url, _recorded_event_label(event), event.confidence)
            archive = self._generate_archive(self.recording)
        except Exception as exc:  # noqa: BLE001 - surfaced to UI
            QMessageBox.critical(self, "生成失败", str(exc))
            return
        self.log.append(f"已从 recording.json 生成草稿脚本包：{archive}")

    def _generate_archive(self, recording: Recording) -> Path:
        output_dir = QFileDialog.getExistingDirectory(
            self,
            "选择草稿包输出目录",
            str(get_data_dir() / "outputs"),
        )
        if not output_dir:
            raise RuntimeError("已取消选择输出目录")
        package_id = self.package_id.text().strip() or None
        generator = RecorderPackageGenerator(Path(output_dir))
        return generator.generate(recording, package_id=package_id)

    def _append_event(self, event_type: str, url: str, label: str, confidence: str) -> None:
        row = self.event_table.rowCount()
        self.event_table.insertRow(row)
        for column, value in enumerate((event_type, url, label, confidence)):
            self.event_table.setItem(row, column, QTableWidgetItem(value))


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
    settings_changed = Signal()

    def __init__(self, settings_store: SettingsStore):
        super().__init__("设置", "管理高级功能、本地 Python 检测和脚本包运行环境策略。")
        self.settings_store = settings_store
        self.runtime_manager = RuntimeManager()

        card = Card()
        data_dir = QLabel(str(get_data_dir()))
        data_dir.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        data_dir.setFont(QFont("monospace"))
        card.layout.addWidget(QLabel("数据目录"))
        card.layout.addWidget(data_dir)

        self.recorder_enabled = QCheckBox("启用高级功能：浏览器录制")
        self.runtime_mode = QComboBox()
        self.runtime_mode.addItem("每个脚本包新建 venv（默认，隔离性最好）", "new_venv")
        self.runtime_mode.addItem("共享当前 DRPA Python 环境", "shared")
        self.runtime_mode.addItem("使用已有 venv", "existing_venv")
        self.existing_venv_path = QLineEdit()
        self.existing_venv_path.setPlaceholderText("选择已有 venv 目录，例如 /path/to/.venv")
        self.choose_venv_button = QPushButton("选择 venv")

        runtime_form = QFormLayout()
        runtime_form.addRow("", self.recorder_enabled)
        runtime_form.addRow("运行环境策略", self.runtime_mode)
        venv_row = QHBoxLayout()
        venv_row.addWidget(self.existing_venv_path, 1)
        venv_row.addWidget(self.choose_venv_button)
        runtime_form.addRow("已有 venv", venv_row)
        card.layout.addLayout(runtime_form)

        buttons = QHBoxLayout()
        self.save_button = QPushButton("保存设置")
        self.save_button.setObjectName("PrimaryButton")
        self.detect_button = QPushButton("检测本地 Python")
        buttons.addWidget(self.save_button)
        buttons.addWidget(self.detect_button)
        buttons.addStretch(1)
        card.layout.addLayout(buttons)

        self.python_table = QTableWidget(0, 4)
        self.python_table.setHorizontalHeaderLabels(["Python", "版本", "是否 venv", "Prefix"])
        self.python_table.horizontalHeader().setStretchLastSection(True)
        self.python_table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        card.layout.addWidget(QLabel("本地 Python 环境"))
        card.layout.addWidget(self.python_table)

        self.content.addWidget(card)
        self.content.addStretch(1)
        self.choose_venv_button.clicked.connect(self._choose_venv)
        self.detect_button.clicked.connect(self.detect_python)
        self.save_button.clicked.connect(self.save)
        self.load()

    def load(self) -> None:
        settings = self.settings_store.load()
        self.recorder_enabled.setChecked(settings.advanced_recorder_enabled)
        index = self.runtime_mode.findData(settings.runtime_mode)
        self.runtime_mode.setCurrentIndex(max(index, 0))
        self.existing_venv_path.setText(settings.existing_venv_path)
        self.detect_python()

    def save(self) -> None:
        settings = AppSettings(
            advanced_recorder_enabled=self.recorder_enabled.isChecked(),
            runtime_mode=self.runtime_mode.currentData(),
            existing_venv_path=self.existing_venv_path.text().strip(),
        )
        if settings.runtime_mode == "existing_venv" and not settings.existing_venv_path:
            QMessageBox.warning(self, "缺少 venv", "选择“使用已有 venv”时必须配置 venv 路径。")
            return
        self.settings_store.save(settings)
        self.settings_changed.emit()
        QMessageBox.information(self, "设置已保存", "设置已保存，导航和后续脚本包安装会使用新配置。")

    def _choose_venv(self) -> None:
        path = QFileDialog.getExistingDirectory(self, "选择已有 venv 目录", str(Path.home()))
        if path:
            self.existing_venv_path.setText(path)

    def detect_python(self) -> None:
        items = self.runtime_manager.detect_python_environments()
        self.python_table.setRowCount(len(items))
        for row, item in enumerate(items):
            values = [
                item.get("executable", ""),
                item.get("version", ""),
                item.get("is_venv", ""),
                item.get("prefix", ""),
            ]
            for column, value in enumerate(values):
                self.python_table.setItem(row, column, QTableWidgetItem(value))
