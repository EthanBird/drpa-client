#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use drpa_install::{
    BrowserResolution, ComponentManifest, ComponentSelection, ComponentState, InstallLayout,
    current_platform, detect_system_browser, discover_installation, system_webview2_version,
    verify_component_pack,
};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText, Stroke};
use semver::Version;

const BLUE: Color32 = Color32::from_rgb(43, 94, 230);
const CYAN: Color32 = Color32::from_rgb(37, 190, 206);
const INK: Color32 = Color32::from_rgb(18, 35, 70);
const MUTED: Color32 = Color32::from_rgb(101, 116, 145);
const SURFACE: Color32 = Color32::from_rgb(247, 249, 253);
const BORDER: Color32 = Color32::from_rgb(220, 226, 239);
const SUCCESS: Color32 = Color32::from_rgb(28, 155, 100);
const WARNING: Color32 = Color32::from_rgb(219, 132, 27);
const DESKTOP_COMPONENT_ID: &str = "org.drpa.desktop-ui";
const WEBVIEW_COMPONENT_ID: &str = "org.drpa.webview2-fixed";
const BROWSER_COMPONENT_ID: &str = "org.drpa.browser.chromium";
const NAVIGATION_HEIGHT: f32 = 84.0;

fn main() {
    let arguments = Arguments::parse(env::args_os().skip(1));
    let options = eframe::NativeOptions {
        renderer: native_renderer(),
        viewport: egui::ViewportBuilder::default()
            .with_title("DRPA 组件安装向导")
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([820.0, 580.0])
            .with_resizable(true),
        centered: true,
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "DRPA Component Setup",
        options,
        Box::new(move |context| Ok(Box::new(InstallerApp::new(context, arguments)))),
    ) {
        show_error(&format!("组件安装向导启动失败：{error}"));
    }
}

#[cfg(windows)]
fn native_renderer() -> eframe::Renderer {
    eframe::Renderer::Glow
}

#[cfg(not(windows))]
fn native_renderer() -> eframe::Renderer {
    eframe::Renderer::Glow
}

#[derive(Default)]
struct Arguments {
    install_root: Option<PathBuf>,
    scan_roots: Vec<PathBuf>,
    packages: Vec<PathBuf>,
    required_component: Option<String>,
}

impl Arguments {
    fn parse(values: impl IntoIterator<Item = std::ffi::OsString>) -> Self {
        let values = values.into_iter().collect::<Vec<_>>();
        let mut result = Self::default();
        let mut index = 0;
        while index < values.len() {
            let value = values[index].to_string_lossy();
            match value.as_ref() {
                "--install-root" if index + 1 < values.len() => {
                    result.install_root = Some(PathBuf::from(&values[index + 1]));
                    index += 2;
                }
                "--scan" if index + 1 < values.len() => {
                    result.scan_roots.push(PathBuf::from(&values[index + 1]));
                    index += 2;
                }
                "--required" if index + 1 < values.len() => {
                    result.required_component =
                        Some(values[index + 1].to_string_lossy().into_owned());
                    index += 2;
                }
                _ => {
                    let path = PathBuf::from(&values[index]);
                    if is_component_pack(&path) {
                        result.packages.push(path);
                    }
                    index += 1;
                }
            }
        }
        result
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WizardStep {
    Welcome,
    Environment,
    Components,
    Installing,
    Complete,
}

impl WizardStep {
    fn index(self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Environment => 1,
            Self::Components => 2,
            Self::Installing => 3,
            Self::Complete => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComponentNextAction {
    Wait,
    Install,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagementAction {
    Repair,
    Uninstall,
}

impl ManagementAction {
    fn display_name(self) -> &'static str {
        match self {
            Self::Repair => "修复",
            Self::Uninstall => "卸载",
        }
    }
}

#[derive(Clone)]
struct Candidate {
    path: PathBuf,
    manifest: ComponentManifest,
    archive_bytes: u64,
    selected: bool,
    system_satisfied: bool,
    status: PackageStatus,
}

#[derive(Clone)]
enum PackageStatus {
    Ready,
    AlreadyInstalled,
    Installing,
    Installed,
    Incompatible,
    Failed(String),
}

enum WorkerMessage {
    Scanned {
        path: PathBuf,
        archive_bytes: u64,
        result: Result<ComponentManifest, String>,
    },
    ScanFinished,
    Installing {
        path: PathBuf,
        current: usize,
        total: usize,
    },
    Installed {
        path: PathBuf,
    },
    InstallFailed {
        path: PathBuf,
        error: String,
    },
    InstallFinished {
        installed: usize,
        failed: usize,
        cancelled: bool,
    },
    ManagementFinished {
        component_id: String,
        action: ManagementAction,
        result: Result<String, String>,
    },
}

struct InstallerApp {
    layout: Option<InstallLayout>,
    install_error: Option<String>,
    active: BTreeMap<String, ComponentSelection>,
    step: WizardStep,
    candidates: Vec<Candidate>,
    scan_errors: Vec<String>,
    scan_busy: bool,
    queued_paths: Vec<PathBuf>,
    worker_tx: Sender<WorkerMessage>,
    worker_rx: Receiver<WorkerMessage>,
    install_progress: f32,
    install_summary: String,
    cancel: Arc<AtomicBool>,
    file_browser: FileBrowser,
    system_webview2: Option<String>,
    system_browser: Option<BrowserResolution>,
    required_component: Option<String>,
    management_busy: Option<(String, ManagementAction)>,
    management_notice: Option<(bool, String)>,
    pending_uninstall: Option<String>,
}

impl InstallerApp {
    fn new(context: &eframe::CreationContext<'_>, arguments: Arguments) -> Self {
        configure_look_and_feel(&context.egui_ctx);
        let (worker_tx, worker_rx) = mpsc::channel();
        let layout = discover_installation(arguments.install_root.as_deref());
        let (layout, install_error) = match layout {
            Ok(layout) => (Some(layout), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let active = layout
            .as_ref()
            .and_then(|layout| layout.read_state().ok())
            .unwrap_or_else(ComponentState::default)
            .active;
        let start_directory = preferred_start_directory(layout.as_ref());
        let required_component = arguments.required_component;
        let mut app = Self {
            layout,
            install_error,
            active,
            step: if required_component.is_some() {
                WizardStep::Environment
            } else {
                WizardStep::Welcome
            },
            candidates: Vec::new(),
            scan_errors: Vec::new(),
            scan_busy: false,
            queued_paths: Vec::new(),
            worker_tx,
            worker_rx,
            install_progress: 0.0,
            install_summary: String::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            file_browser: FileBrowser::new(start_directory),
            system_webview2: system_webview2_version(),
            system_browser: detect_system_browser(),
            required_component,
            management_busy: None,
            management_notice: None,
            pending_uninstall: None,
        };
        let mut roots = automatic_scan_roots(app.layout.as_ref());
        roots.extend(arguments.scan_roots);
        roots.extend(arguments.packages);
        app.start_scan(discover_component_packs(&roots));
        app
    }

    fn start_scan(&mut self, paths: Vec<PathBuf>) {
        let known = self
            .candidates
            .iter()
            .filter_map(|item| item.path.canonicalize().ok())
            .collect::<BTreeSet<_>>();
        let mut unique = BTreeSet::new();
        let paths = paths
            .into_iter()
            .filter(|path| is_component_pack(path) && path.is_file())
            .filter(|path| {
                let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                !known.contains(&normalized) && unique.insert(normalized)
            })
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return;
        }
        if self.scan_busy {
            self.queued_paths.extend(paths);
            return;
        }
        self.scan_busy = true;
        let sender = self.worker_tx.clone();
        thread::spawn(move || {
            for path in paths {
                let archive_bytes = fs::metadata(&path).map(|item| item.len()).unwrap_or(0);
                let result = verify_component_pack(&path).map_err(|error| error.to_string());
                if sender
                    .send(WorkerMessage::Scanned {
                        path,
                        archive_bytes,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            let _ = sender.send(WorkerMessage::ScanFinished);
        });
    }

    fn begin_install(&mut self) {
        let Some(layout) = self.layout.clone() else {
            return;
        };
        let mut packages = self
            .candidates
            .iter()
            .filter(|item| item.selected && matches!(item.status, PackageStatus::Ready))
            .map(|item| (component_priority(&item.manifest.id), item.path.clone()))
            .collect::<Vec<_>>();
        packages.sort_by_key(|(priority, _)| *priority);
        if packages.is_empty() {
            return;
        }
        self.step = WizardStep::Installing;
        self.install_progress = 0.0;
        self.install_summary.clear();
        self.cancel.store(false, Ordering::Relaxed);
        let sender = self.worker_tx.clone();
        let cancel = Arc::clone(&self.cancel);
        thread::spawn(move || {
            let total = packages.len();
            let mut installed = 0;
            let mut failed = 0;
            for (index, (_, path)) in packages.into_iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if sender
                    .send(WorkerMessage::Installing {
                        path: path.clone(),
                        current: index,
                        total,
                    })
                    .is_err()
                {
                    return;
                }
                match install_component(&layout, &path) {
                    Ok(_) => {
                        installed += 1;
                        if sender.send(WorkerMessage::Installed { path }).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        failed += 1;
                        if sender
                            .send(WorkerMessage::InstallFailed {
                                path,
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let _ = sender.send(WorkerMessage::InstallFinished {
                installed,
                failed,
                cancelled: cancel.load(Ordering::Relaxed),
            });
        });
    }

    fn skip_install(&mut self) {
        self.install_summary = if self.all_components_satisfied() {
            "现有组件与系统能力均已就绪，已跳过重复安装".to_owned()
        } else {
            "已按你的选择跳过组件安装".to_owned()
        };
        self.step = WizardStep::Complete;
    }

    fn start_repair(&mut self, component_id: String) {
        if self.management_busy.is_some() {
            return;
        }
        let Some(layout) = self.layout.clone() else {
            self.management_notice = Some((false, "没有可用的 DRPA Core 安装".to_owned()));
            return;
        };
        let Some(selection) = self.active.get(&component_id) else {
            self.management_notice = Some((false, "组件已经不在活动列表中".to_owned()));
            return;
        };
        let repair_pack = self
            .candidates
            .iter()
            .find(|item| {
                item.manifest.id == component_id && item.manifest.version == selection.version
            })
            .map(|item| item.path.clone());
        self.management_busy = Some((component_id.clone(), ManagementAction::Repair));
        self.management_notice = None;
        let sender = self.worker_tx.clone();
        thread::spawn(move || {
            let result = if let Some(pack) = repair_pack {
                install_component(&layout, &pack)
                    .map(|manifest| {
                        format!(
                            "已使用 {} 重新安装 {} {}",
                            pack.display(),
                            manifest.display_name,
                            manifest.version
                        )
                    })
                    .map_err(|error| {
                        if component_id == DESKTOP_COMPONENT_ID {
                            format!(
                                "{error}。若 DRPA Next 主界面仍在运行，请先退出后再修复桌面组件"
                            )
                        } else {
                            error.to_string()
                        }
                    })
            } else {
                layout
                    .verify_component(&component_id)
                    .map(|manifest| {
                        format!(
                            "{} {} 校验通过；如需强制重装，请先添加同版本 .drpac",
                            manifest.display_name, manifest.version
                        )
                    })
                    .map_err(|error| format!("组件校验失败且未找到同版本 .drpac：{error}"))
            };
            let _ = sender.send(WorkerMessage::ManagementFinished {
                component_id,
                action: ManagementAction::Repair,
                result,
            });
        });
    }

    fn start_uninstall(&mut self, component_id: String) {
        if self.management_busy.is_some() {
            return;
        }
        let Some(layout) = self.layout.clone() else {
            self.management_notice = Some((false, "没有可用的 DRPA Core 安装".to_owned()));
            return;
        };
        self.management_busy = Some((component_id.clone(), ManagementAction::Uninstall));
        self.management_notice = None;
        let sender = self.worker_tx.clone();
        thread::spawn(move || {
            let result = remove_component(&layout, &component_id, true)
                .map(|_| format!("已卸载 {component_id} 的全部组件版本"))
                .map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::ManagementFinished {
                component_id,
                action: ManagementAction::Uninstall,
                result,
            });
        });
    }

    fn handle_worker_messages(&mut self, context: &egui::Context) {
        let mut repaint = false;
        while let Ok(message) = self.worker_rx.try_recv() {
            repaint = true;
            match message {
                WorkerMessage::Scanned {
                    path,
                    archive_bytes,
                    result,
                } => match result {
                    Ok(manifest) => self.merge_candidate(path, archive_bytes, manifest),
                    Err(error) => self
                        .scan_errors
                        .push(format!("{}：{error}", path.display())),
                },
                WorkerMessage::ScanFinished => {
                    self.scan_busy = false;
                    if !self.queued_paths.is_empty() {
                        let queued = std::mem::take(&mut self.queued_paths);
                        self.start_scan(queued);
                    }
                }
                WorkerMessage::Installing {
                    path,
                    current,
                    total,
                } => {
                    self.install_progress = current as f32 / total.max(1) as f32;
                    if let Some(item) = self.candidate_mut(&path) {
                        item.status = PackageStatus::Installing;
                    }
                }
                WorkerMessage::Installed { path } => {
                    if let Some(item) = self.candidate_mut(&path) {
                        item.status = PackageStatus::Installed;
                        item.selected = false;
                    }
                }
                WorkerMessage::InstallFailed { path, error } => {
                    if let Some(item) = self.candidate_mut(&path) {
                        item.status = PackageStatus::Failed(error);
                    }
                }
                WorkerMessage::InstallFinished {
                    installed,
                    failed,
                    cancelled,
                } => {
                    self.install_progress = 1.0;
                    self.reload_active_state();
                    self.install_summary = if cancelled {
                        format!("已停止后续安装：成功 {installed} 个，失败 {failed} 个")
                    } else {
                        format!("组件处理完成：成功 {installed} 个，失败 {failed} 个")
                    };
                    self.step = WizardStep::Complete;
                }
                WorkerMessage::ManagementFinished {
                    component_id,
                    action,
                    result,
                } => {
                    self.management_busy = None;
                    self.reload_active_state();
                    self.management_notice = Some(match result {
                        Ok(message) => (true, message),
                        Err(error) => (
                            false,
                            format!(
                                "{}组件 {} 失败：{}",
                                action.display_name(),
                                component_id,
                                error
                            ),
                        ),
                    });
                }
            }
        }
        if repaint
            || self.scan_busy
            || self.step == WizardStep::Installing
            || self.management_busy.is_some()
        {
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn merge_candidate(&mut self, path: PathBuf, archive_bytes: u64, manifest: ComponentManifest) {
        if self.candidates.iter().any(|item| {
            item.manifest.id == manifest.id
                && item.manifest.version == manifest.version
                && same_path(&item.path, &path)
        }) {
            return;
        }
        let installed = self
            .active
            .get(&manifest.id)
            .is_some_and(|selection| selection.version == manifest.version);
        let compatible = manifest.platform == current_platform();
        let system_satisfied = component_satisfied_by_system(
            &manifest.id,
            self.system_webview2.is_some(),
            self.system_browser.is_some(),
        );
        let component_id = manifest.id.clone();
        self.candidates.push(Candidate {
            path,
            manifest,
            archive_bytes,
            selected: compatible && !installed && !system_satisfied,
            system_satisfied,
            status: if !compatible {
                PackageStatus::Incompatible
            } else if installed {
                PackageStatus::AlreadyInstalled
            } else {
                PackageStatus::Ready
            },
        });
        self.candidates.sort_by(|left, right| {
            component_priority(&left.manifest.id)
                .cmp(&component_priority(&right.manifest.id))
                .then_with(|| left.manifest.display_name.cmp(&right.manifest.display_name))
                .then_with(|| right.manifest.version.cmp(&left.manifest.version))
        });
        self.keep_only_latest_default(&component_id);
    }

    fn keep_only_latest_default(&mut self, component_id: &str) {
        let selected = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.manifest.id == component_id
                    && item.selected
                    && matches!(item.status, PackageStatus::Ready)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if selected.len() < 2 {
            return;
        }
        let latest = selected
            .iter()
            .copied()
            .max_by(|left, right| {
                compare_versions(
                    &self.candidates[*left].manifest.version,
                    &self.candidates[*right].manifest.version,
                )
            })
            .expect("at least two selected versions");
        for index in selected {
            self.candidates[index].selected = index == latest;
        }
    }

    fn candidate_mut(&mut self, path: &Path) -> Option<&mut Candidate> {
        self.candidates
            .iter_mut()
            .find(|candidate| same_path(&candidate.path, path))
    }

    fn reload_active_state(&mut self) {
        self.active = self
            .layout
            .as_ref()
            .and_then(|layout| layout.read_state().ok())
            .unwrap_or_else(ComponentState::default)
            .active;
        for item in &mut self.candidates {
            let installed = self
                .active
                .get(&item.manifest.id)
                .is_some_and(|selection| selection.version == item.manifest.version);
            if installed {
                item.status = PackageStatus::AlreadyInstalled;
                item.selected = false;
            } else if item.manifest.platform == current_platform() {
                item.status = PackageStatus::Ready;
                item.selected = false;
            }
        }
    }

    fn selected_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|item| item.selected && matches!(item.status, PackageStatus::Ready))
            .count()
    }

    fn selected_bytes(&self) -> u64 {
        self.candidates
            .iter()
            .filter(|item| item.selected && matches!(item.status, PackageStatus::Ready))
            .map(|item| item.archive_bytes)
            .sum()
    }

    fn all_components_satisfied(&self) -> bool {
        if self.candidates.is_empty() {
            return self.desktop_ready() && self.webview_ready();
        }
        self.candidates.iter().all(|item| match &item.status {
            PackageStatus::AlreadyInstalled | PackageStatus::Installed => true,
            PackageStatus::Ready => item.system_satisfied,
            PackageStatus::Installing | PackageStatus::Incompatible | PackageStatus::Failed(_) => {
                false
            }
        })
    }

    fn desktop_ready(&self) -> bool {
        self.active.contains_key(DESKTOP_COMPONENT_ID)
    }

    fn webview_ready(&self) -> bool {
        !cfg!(windows)
            || self.system_webview2.is_some()
            || self.active.contains_key(WEBVIEW_COMPONENT_ID)
    }

    fn launch_desktop(&self, context: &egui::Context) {
        let Some(layout) = &self.layout else {
            return;
        };
        let Some(launcher) = stable_launcher_executable(layout.root()) else {
            show_error("无法定位 DRPA Next 稳定启动器");
            return;
        };
        if let Err(error) = Command::new(&launcher)
            .current_dir(layout.root())
            .env("DRPA_INSTALL_ROOT", layout.root())
            .spawn()
        {
            show_error(&format!("无法启动 {}：{error}", launcher.display()));
            return;
        }
        context.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn draw_step_rail(&self, context: &egui::Context) {
        egui::SidePanel::left("steps")
            .exact_width(220.0)
            .frame(
                egui::Frame::new()
                    .fill(INK)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(context, |ui| {
                ui.add_space(10.0);
                draw_mark(ui);
                ui.add_space(30.0);
                for (index, (title, subtitle)) in [
                    ("开始", "了解安装方式"),
                    ("环境检测", "检查 Core 与组件"),
                    ("选择组件", "添加 .drpac 文件"),
                    ("安全安装", "校验并激活版本"),
                    ("完成", "启动或继续配置"),
                ]
                .into_iter()
                .enumerate()
                {
                    let active = index == self.step.index();
                    let done = index < self.step.index();
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            13.0,
                            if active || done {
                                CYAN
                            } else {
                                Color32::from_rgb(54, 72, 107)
                            },
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            if done {
                                "✓".to_owned()
                            } else {
                                (index + 1).to_string()
                            },
                            egui::FontId::proportional(13.0),
                            Color32::WHITE,
                        );
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).size(15.0).strong().color(if active {
                                Color32::WHITE
                            } else {
                                Color32::from_rgb(191, 203, 226)
                            }));
                            ui.label(
                                RichText::new(subtitle)
                                    .size(11.0)
                                    .color(Color32::from_rgb(132, 151, 186)),
                            );
                        });
                    });
                    ui.add_space(20.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("Data · Runtime · Process · AI")
                            .size(11.0)
                            .color(Color32::from_rgb(116, 139, 180)),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Core {} · {}",
                            env!("CARGO_PKG_VERSION"),
                            current_platform()
                        ))
                        .size(11.0)
                        .color(Color32::from_rgb(116, 139, 180)),
                    );
                });
            });
    }

    fn draw_welcome(&mut self, ui: &mut egui::Ui) {
        page_heading(
            ui,
            "欢迎使用 DRPA 组件安装向导",
            "Core 已与大型运行组件分离。按需下载并安装 .drpac，保持基础安装轻量、可恢复。",
        );
        ui.add_space(22.0);
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(14)
            .inner_margin(egui::Margin::same(22))
            .show(ui, |ui| {
                ui.columns(3, |columns| {
                    feature_card(
                        &mut columns[0],
                        "01",
                        "轻量 Core",
                        "命令行、组件管理和此向导始终可运行，不依赖网页渲染内核。",
                        BLUE,
                    );
                    feature_card(
                        &mut columns[1],
                        "02",
                        "离线组件",
                        "桌面、Python、浏览器等均为独立包，可分开下载与升级。",
                        CYAN,
                    );
                    feature_card(
                        &mut columns[2],
                        "03",
                        "事务安装",
                        "安装前验证全部文件，成功后才切换活动版本，并保留回滚点。",
                        Color32::from_rgb(123, 92, 224),
                    );
                });
            });
        ui.add_space(18.0);
        notice(
            ui,
            "组件包尚未加入数字签名，请只使用 DRPA 官方发布或你自己构建的 .drpac 文件。",
            WARNING,
        );
    }

    fn draw_environment(&mut self, ui: &mut egui::Ui) {
        page_heading(
            ui,
            "检测安装环境",
            if cfg!(windows) {
                "通过 Core 安装标记、WebView2 官方 Loader 和实际文件判断当前状态。"
            } else {
                "通过 Core 安装标记、UOS 图形环境和实际文件判断当前状态。系统级组件安装时会请求授权。"
            },
        );
        ui.add_space(18.0);
        let root = self
            .layout
            .as_ref()
            .map(|layout| layout.root().display().to_string())
            .unwrap_or_else(|| "未找到".to_owned());
        environment_row(ui, "DRPA Core", self.layout.is_some(), &root);
        if cfg!(windows) {
            environment_row(
                ui,
                "系统 WebView2",
                self.system_webview2.is_some(),
                self.system_webview2
                    .as_deref()
                    .map(|version| format!("Evergreen {version}（将使用系统默认运行时）"))
                    .as_deref()
                    .unwrap_or("未检测到；启动桌面前必须安装固定版 WebView2 组件"),
            );
        } else {
            environment_row(
                ui,
                "UOS 图形环境",
                true,
                "向导使用原生 egui；桌面组件自带已验证的 WebKitGTK 运行时",
            );
        }
        environment_row(
            ui,
            "系统 Chromium 浏览器",
            self.system_browser.is_some(),
            self.system_browser
                .as_ref()
                .map(|browser| {
                    format!(
                        "{} · {}",
                        browser.source.display_name(),
                        browser.executable.display()
                    )
                })
                .as_deref()
                .unwrap_or("未检测到；浏览器自动化需要 Chromium 组件"),
        );
        environment_row(
            ui,
            "已安装组件",
            !self.active.is_empty(),
            &format!("{} 个活动组件", self.active.len()),
        );
        environment_row(
            ui,
            "发现的组件包",
            !self.candidates.is_empty(),
            &if self.scan_busy {
                format!("正在校验，已发现 {} 个", self.candidates.len())
            } else {
                format!("{} 个有效包", self.candidates.len())
            },
        );
        if let Some(error) = &self.install_error {
            ui.add_space(12.0);
            notice(
                ui,
                &format!("没有可用的 Core 安装：{error}。请先运行轻量 Setup。"),
                Color32::from_rgb(205, 67, 77),
            );
        }
        if let Some(required) = &self.required_component {
            ui.add_space(12.0);
            notice(
                ui,
                &format!(
                    "DRPA Next 缺少启动所需能力：{}。请添加并安装对应组件。",
                    component_description(required)
                ),
                WARNING,
            );
        }
        if self.scan_busy {
            ui.add_space(16.0);
            ui.add(
                egui::ProgressBar::new(0.5)
                    .animate(true)
                    .text("正在逐文件校验 .drpac…"),
            );
        }
    }

    fn draw_installed_components(&mut self, ui: &mut egui::Ui) {
        if self.active.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("已安装组件 · {}", self.active.len()))
                    .size(16.0)
                    .strong()
                    .color(INK),
            );
            if let Some((component_id, action)) = &self.management_busy {
                ui.spinner();
                ui.label(
                    RichText::new(format!(
                        "正在{} {}",
                        action.display_name(),
                        component_display_name(component_id)
                    ))
                    .size(11.5)
                    .color(MUTED),
                );
            }
        });
        ui.add_space(8.0);

        let installed = self
            .active
            .iter()
            .map(|(id, selection)| (id.clone(), selection.clone()))
            .collect::<Vec<_>>();
        let mut repair = None;
        let mut uninstall = None;
        for (component_id, selection) in installed {
            let has_repair_pack = self.candidates.iter().any(|item| {
                item.manifest.id == component_id && item.manifest.version == selection.version
            });
            egui::Frame::new()
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(15, 11))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(component_display_name(&component_id))
                                        .strong()
                                        .color(INK),
                                );
                                ui.label(
                                    RichText::new(format!("v{}", selection.version))
                                        .size(10.5)
                                        .color(BLUE),
                                );
                            });
                            ui.label(RichText::new(&component_id).size(10.5).color(MUTED));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let enabled = self.management_busy.is_none();
                            if ui
                                .add_enabled(
                                    enabled,
                                    egui::Button::new(
                                        RichText::new("卸载").color(Color32::from_rgb(190, 63, 72)),
                                    ),
                                )
                                .clicked()
                            {
                                uninstall = Some(component_id.clone());
                            }
                            let repair_response = ui.add_enabled(enabled, primary("修复"));
                            let repair_response = if has_repair_pack {
                                repair_response.on_hover_text("使用同版本 .drpac 重新校验并安装")
                            } else {
                                repair_response
                                    .on_hover_text("当前未发现同版本 .drpac；将先校验现有文件")
                            };
                            if repair_response.clicked() {
                                repair = Some(component_id.clone());
                            }
                        });
                    });
                });
            ui.add_space(7.0);
        }
        if let Some(component_id) = repair {
            self.start_repair(component_id);
        }
        if let Some(component_id) = uninstall {
            self.pending_uninstall = Some(component_id);
        }
        ui.separator();
        ui.add_space(12.0);
    }

    fn draw_components(&mut self, ui: &mut egui::Ui) {
        page_heading(
            ui,
            "选择离线组件",
            "拖入 .drpac、使用内置文件浏览器选择，或让向导扫描下载目录。",
        );
        ui.add_space(12.0);
        self.draw_installed_components(ui);
        if let Some((ok, message)) = &self.management_notice {
            notice(ui, message, if *ok { SUCCESS } else { WARNING });
            ui.add_space(12.0);
        }
        ui.horizontal(|ui| {
            if ui.button("＋ 选择 .drpac 文件").clicked() {
                self.file_browser.open();
            }
            if ui.button("重新扫描常用目录").clicked() {
                self.start_scan(discover_component_packs(&automatic_scan_roots(
                    self.layout.as_ref(),
                )));
            }
            if self.scan_busy {
                ui.spinner();
                ui.label(RichText::new("正在校验").color(MUTED));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "已选 {} · {}",
                        self.selected_count(),
                        human_bytes(self.selected_bytes())
                    ))
                    .strong()
                    .color(INK),
                );
            });
        });
        ui.add_space(10.0);
        if self.candidates.is_empty() {
            egui::Frame::new()
                .fill(SURFACE)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(12)
                .inner_margin(egui::Margin::same(28))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(18.0);
                        ui.label(
                            RichText::new("把 .drpac 文件拖到这里")
                                .size(20.0)
                                .strong()
                                .color(INK),
                        );
                        ui.label(
                            RichText::new("也可以点击上方按钮，从任意磁盘选择组件包").color(MUTED),
                        );
                        ui.add_space(18.0);
                    });
                });
        } else {
            let mut selected_component: Option<(usize, String)> = None;
            for (index, item) in self.candidates.iter_mut().enumerate() {
                component_card(ui, item, &mut selected_component, index);
                ui.add_space(8.0);
            }
            if let Some((selected_index, id)) = selected_component {
                for (index, item) in self.candidates.iter_mut().enumerate() {
                    if index != selected_index && item.manifest.id == id {
                        item.selected = false;
                    }
                }
            }
        }
        if !self.scan_errors.is_empty() {
            ui.add_space(8.0);
            egui::CollapsingHeader::new(format!("有 {} 个文件未通过校验", self.scan_errors.len()))
                .show(ui, |ui| {
                    for error in &self.scan_errors {
                        ui.label(
                            RichText::new(error)
                                .size(11.0)
                                .color(Color32::from_rgb(190, 63, 72)),
                        );
                    }
                });
        }
        if self.desktop_selection_needs_webview() {
            ui.add_space(8.0);
            notice(
                ui,
                "已选择桌面工作台，但未检测到系统或固定版 WebView2。桌面界面可能无法启动。",
                WARNING,
            );
        } else if !self.scan_busy && self.selected_count() == 0 {
            ui.add_space(8.0);
            notice(
                ui,
                if self.all_components_satisfied() {
                    "当前组件与系统能力均已满足，无需重复安装，可以直接跳过。"
                } else {
                    "当前没有选择待安装组件；你仍可跳过安装并继续使用现有 Core。"
                },
                SUCCESS,
            );
        }
    }

    fn desktop_selection_needs_webview(&self) -> bool {
        let desktop_selected = self
            .candidates
            .iter()
            .any(|item| item.manifest.id == DESKTOP_COMPONENT_ID && item.selected);
        let webview_selected = self
            .candidates
            .iter()
            .any(|item| item.manifest.id == WEBVIEW_COMPONENT_ID && item.selected);
        desktop_selected && !webview_selected && !self.webview_ready()
    }

    fn draw_installing(&mut self, ui: &mut egui::Ui) {
        page_heading(
            ui,
            "正在安全安装组件",
            "每个组件都会再次验证并写入暂存目录，完成后原子切换活动版本。",
        );
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.add_space(18.0);
            ui.label(
                RichText::new("请保持向导运行")
                    .size(24.0)
                    .strong()
                    .color(INK),
            );
            ui.label(
                RichText::new("大组件解压时可能需要几分钟，现有组件不会被提前删除。").color(MUTED),
            );
            ui.add_space(24.0);
            ui.add_sized(
                [520.0, 22.0],
                egui::ProgressBar::new(self.install_progress).animate(true),
            );
            ui.add_space(16.0);
            for item in &self.candidates {
                if matches!(item.status, PackageStatus::Installing) {
                    ui.label(
                        RichText::new(format!(
                            "正在安装：{} {}",
                            item.manifest.display_name, item.manifest.version
                        ))
                        .strong()
                        .color(BLUE),
                    );
                }
            }
        });
    }

    fn draw_complete(&mut self, ui: &mut egui::Ui) {
        page_heading(
            ui,
            "组件配置完成",
            "活动组件状态已经保存，Core 会从版本化目录解析入口。",
        );
        ui.add_space(32.0);
        egui::Frame::new()
            .fill(Color32::from_rgb(238, 250, 245))
            .stroke(Stroke::new(1.0, Color32::from_rgb(178, 226, 204)))
            .corner_radius(16)
            .inner_margin(egui::Margin::same(28))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✓").size(38.0).color(SUCCESS));
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&self.install_summary)
                                .size(20.0)
                                .strong()
                                .color(INK),
                        );
                        ui.label(
                            RichText::new(format!("当前共有 {} 个活动组件", self.active.len()))
                                .color(MUTED),
                        );
                    });
                });
            });
        ui.add_space(18.0);
        if self.desktop_ready() && !self.webview_ready() {
            notice(
                ui,
                "桌面组件已安装，但仍未检测到 WebView2。你可以返回并添加固定版 WebView2 组件。",
                WARNING,
            );
        } else if !self.desktop_ready() {
            notice(
                ui,
                "Core 可以直接通过命令行使用；若需要图形工作台，请继续安装桌面 UI 组件。",
                BLUE,
            );
        }
    }

    fn draw_navigation(&mut self, context: &egui::Context, ui: &mut egui::Ui) {
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            match self.step {
                WizardStep::Welcome => {
                    if ui.button("退出").clicked() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                WizardStep::Environment | WizardStep::Components => {
                    if ui.button("上一步").clicked() {
                        self.step = if self.step == WizardStep::Environment {
                            WizardStep::Welcome
                        } else {
                            WizardStep::Environment
                        };
                    }
                }
                WizardStep::Installing => {
                    if ui.button("停止后续组件").clicked() {
                        self.cancel.store(true, Ordering::Relaxed);
                    }
                }
                WizardStep::Complete => {
                    if ui.button("继续添加组件").clicked() {
                        self.step = WizardStep::Components;
                    }
                }
            }
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match self.step {
                    WizardStep::Welcome => {
                        if primary_button(ui, "开始检测").clicked() {
                            self.step = WizardStep::Environment;
                        }
                    }
                    WizardStep::Environment => {
                        if ui
                            .add_enabled(
                                self.layout.is_some() && !self.scan_busy,
                                primary("下一步：选择组件"),
                            )
                            .clicked()
                        {
                            self.step = WizardStep::Components;
                        }
                    }
                    WizardStep::Components => {
                        match component_next_action(self.scan_busy, self.selected_count()) {
                            ComponentNextAction::Wait => {
                                ui.add_enabled(false, primary("正在检查组件"));
                            }
                            ComponentNextAction::Install => {
                                if primary_button(ui, "安装所选组件").clicked() {
                                    self.begin_install();
                                }
                            }
                            ComponentNextAction::Skip => {
                                let label = if self.all_components_satisfied() {
                                    "全部就绪，跳过安装"
                                } else {
                                    "跳过组件安装"
                                };
                                if primary_button(ui, label).clicked() {
                                    self.skip_install();
                                }
                            }
                        }
                    }
                    WizardStep::Installing => {}
                    WizardStep::Complete => {
                        if self.desktop_ready() && self.webview_ready() {
                            if primary_button(ui, "启动 DRPA Next").clicked() {
                                self.launch_desktop(context);
                            }
                        } else if ui.button("完成").clicked() {
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                },
            );
        });
    }

    fn draw_uninstall_confirmation(&mut self, context: &egui::Context) {
        let Some(component_id) = self.pending_uninstall.clone() else {
            return;
        };
        let mut cancel = false;
        let mut confirm = false;
        egui::Window::new("确认卸载组件")
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.set_min_width(430.0);
                ui.label(
                    RichText::new(format!(
                        "确定卸载 {}？",
                        component_display_name(&component_id)
                    ))
                    .size(17.0)
                    .strong()
                    .color(INK),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "将删除 {} 的全部已安装版本，但不会删除工作区、项目或用户数据。",
                        component_id
                    ))
                    .color(MUTED),
                );
                if component_id == DESKTOP_COMPONENT_ID {
                    ui.add_space(8.0);
                    notice(
                        ui,
                        "若 DRPA Next 主界面仍在运行，请先退出，否则 Windows 可能拒绝删除正在使用的桌面组件。",
                        WARNING,
                    );
                }
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(
                            RichText::new("确认卸载").color(Color32::from_rgb(190, 63, 72)),
                        )
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.pending_uninstall = None;
        } else if confirm {
            self.pending_uninstall = None;
            self.start_uninstall(component_id);
        }
    }
}

fn component_satisfied_by_system(
    component_id: &str,
    webview2_available: bool,
    browser_available: bool,
) -> bool {
    match component_id {
        WEBVIEW_COMPONENT_ID => !cfg!(windows) || webview2_available,
        BROWSER_COMPONENT_ID => browser_available,
        _ => false,
    }
}

fn install_component(layout: &InstallLayout, archive: &Path) -> Result<ComponentManifest, String> {
    let expected = verify_component_pack(archive).map_err(|error| error.to_string())?;
    if layout_is_writable(layout) {
        return layout
            .install_component(archive)
            .map_err(|error| error.to_string());
    }
    run_elevated_component_command(
        layout,
        &["component", "install", &archive.display().to_string()],
    )?;
    let installed = layout
        .verify_component(&expected.id)
        .map_err(|error| format!("授权安装结束后组件校验失败：{error}"))?;
    if installed.version != expected.version {
        return Err(format!(
            "授权安装未激活期望版本：需要 {}，实际 {}",
            expected.version, installed.version
        ));
    }
    Ok(installed)
}

fn remove_component(
    layout: &InstallLayout,
    component_id: &str,
    purge_versions: bool,
) -> Result<(), String> {
    if layout_is_writable(layout) {
        return layout
            .remove_component(component_id, purge_versions)
            .map_err(|error| error.to_string());
    }
    let mut args = vec!["component", "remove", component_id];
    if purge_versions {
        args.push("--purge");
    }
    run_elevated_component_command(layout, &args).map(|_| ())
}

fn layout_is_writable(layout: &InstallLayout) -> bool {
    let probe = layout
        .root()
        .join("state")
        .join(format!(".write-probe-{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

fn run_elevated_component_command(
    layout: &InstallLayout,
    arguments: &[&str],
) -> Result<String, String> {
    #[cfg(windows)]
    {
        let _ = (layout, arguments);
        return Err("当前安装目录不可写，请使用管理员权限重新打开组件向导".to_owned());
    }
    #[cfg(not(windows))]
    {
        let core = core_cli_executable();
        let mut command = Command::new("pkexec");
        command
            .arg(&core)
            .arg("--install-root")
            .arg(layout.root())
            .args(arguments);
        let output = command
            .output()
            .map_err(|error| format!("无法请求系统安装授权（pkexec）：{error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if output.status.success() {
            Ok(stdout)
        } else if stderr.is_empty() {
            Err(format!("系统安装授权被取消或执行失败：{}", output.status))
        } else {
            Err(format!("系统安装授权失败：{stderr}"))
        }
    }
}

#[cfg(not(windows))]
fn core_cli_executable() -> PathBuf {
    if let Some(path) = env::var_os("DRPA_CORE_CLI") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path;
        }
    }
    #[cfg(target_os = "linux")]
    {
        let installed = PathBuf::from("/usr/lib/drpa-next/drpa");
        if installed.is_file() {
            return installed;
        }
    }
    if let Ok(executable) = env::current_exe()
        && let Some(parent) = executable.parent()
    {
        let sibling = parent.join(if cfg!(windows) { "drpa.exe" } else { "drpa" });
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from(if cfg!(windows) { "drpa.exe" } else { "drpa" })
}

fn stable_launcher_executable(install_root: &Path) -> Option<PathBuf> {
    if let Some(path) = env::var_os("DRPA_LAUNCHER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    #[allow(unused_mut)]
    let mut candidates = vec![
        install_root.join("DRPA Next.exe"),
        install_root.join("drpa-launcher"),
    ];
    #[cfg(target_os = "linux")]
    candidates.extend([
        PathBuf::from("/usr/bin/drpa-next"),
        PathBuf::from("/usr/lib/drpa-next/drpa-launcher"),
    ]);
    candidates.into_iter().find(|path| path.is_file())
}

fn component_next_action(scan_busy: bool, selected_count: usize) -> ComponentNextAction {
    if scan_busy {
        ComponentNextAction::Wait
    } else if selected_count > 0 {
        ComponentNextAction::Install
    } else {
        ComponentNextAction::Skip
    }
}

fn component_display_name(component_id: &str) -> &str {
    match component_id {
        DESKTOP_COMPONENT_ID => "DRPA Desktop UI",
        "org.drpa.python-runtime" => "Python / RPAZ 运行环境",
        BROWSER_COMPONENT_ID => "Chromium 浏览器",
        WEBVIEW_COMPONENT_ID => "WebView2 固定版",
        "org.drpa.jcode" => "JCode",
        _ => component_id,
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_worker_messages(context);
        let dropped = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|item| item.path.clone())
                .collect::<Vec<_>>()
        });
        if !dropped.is_empty() {
            self.start_scan(discover_component_packs(&dropped));
            self.step = WizardStep::Components;
        }
        self.draw_step_rail(context);
        egui::TopBottomPanel::bottom("wizard_navigation")
            .exact_height(NAVIGATION_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(30, 14)),
            )
            .show(context, |ui| self.draw_navigation(context, ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::same(30)),
            )
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("wizard_page_content")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        match self.step {
                            WizardStep::Welcome => self.draw_welcome(ui),
                            WizardStep::Environment => self.draw_environment(ui),
                            WizardStep::Components => self.draw_components(ui),
                            WizardStep::Installing => self.draw_installing(ui),
                            WizardStep::Complete => self.draw_complete(ui),
                        }
                        ui.add_space(12.0);
                    });
            });
        if let Some(path) = self.file_browser.show(context) {
            self.start_scan(vec![path]);
        }
        self.draw_uninstall_confirmation(context);
    }
}

fn configure_look_and_feel(context: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some(path) = system_font_path()
        && let Ok(bytes) = fs::read(path)
    {
        fonts
            .font_data
            .insert("drpa-cjk".to_owned(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "drpa-cjk".to_owned());
        }
    }
    context.set_fonts(fonts);
    let mut style = (*context.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = SURFACE;
    style.visuals.widgets.inactive.corner_radius = 8.into();
    style.visuals.widgets.hovered.corner_radius = 8.into();
    style.visuals.widgets.active.corner_radius = 8.into();
    style.spacing.button_padding = egui::vec2(15.0, 9.0);
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    context.set_style(style);
}

fn draw_mark(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(42.0, 42.0), egui::Sense::hover());
        for (offset, color) in [
            (egui::vec2(0.0, -10.0), BLUE),
            (egui::vec2(-10.0, 0.0), Color32::from_rgb(255, 119, 103)),
            (egui::vec2(10.0, 0.0), CYAN),
            (egui::vec2(0.0, 10.0), Color32::from_rgb(136, 157, 244)),
        ] {
            let center = rect.center() + offset;
            let square = egui::Rect::from_center_size(center, egui::vec2(13.0, 13.0));
            ui.painter().rect_filled(square, 3, color);
        }
        ui.vertical(|ui| {
            ui.label(
                RichText::new("DRPA")
                    .size(20.0)
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.label(
                RichText::new("COMPONENT SETUP")
                    .size(9.0)
                    .color(Color32::from_rgb(136, 157, 195)),
            );
        });
    });
}

fn page_heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(28.0).strong().color(INK));
    ui.add_space(4.0);
    ui.label(RichText::new(subtitle).size(14.0).color(MUTED));
}

fn feature_card(ui: &mut egui::Ui, number: &str, title: &str, body: &str, color: Color32) {
    ui.set_min_height(150.0);
    ui.label(RichText::new(number).size(14.0).strong().color(color));
    ui.add_space(8.0);
    ui.label(RichText::new(title).size(18.0).strong().color(INK));
    ui.add_space(6.0);
    ui.label(RichText::new(body).size(13.0).color(MUTED));
}

fn notice(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.08))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.5).color(color));
        });
}

fn environment_row(ui: &mut egui::Ui, label: &str, ok: bool, detail: &str) {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(16, 13))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if ok { "●" } else { "○" })
                        .size(17.0)
                        .color(if ok { SUCCESS } else { WARNING }),
                );
                ui.label(RichText::new(label).strong().color(INK));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(detail).size(12.0).color(MUTED));
                });
            });
        });
    ui.add_space(8.0);
}

fn component_card(
    ui: &mut egui::Ui,
    item: &mut Candidate,
    selected_component: &mut Option<(usize, String)>,
    index: usize,
) {
    let enabled = matches!(item.status, PackageStatus::Ready);
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(
            if item.selected { 1.5 } else { 1.0 },
            if item.selected { BLUE } else { BORDER },
        ))
        .corner_radius(11)
        .inner_margin(egui::Margin::symmetric(15, 12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let response =
                    ui.add_enabled(enabled, egui::Checkbox::without_text(&mut item.selected));
                if response.changed() && item.selected {
                    *selected_component = Some((index, item.manifest.id.clone()));
                }
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&item.manifest.display_name)
                                .size(15.0)
                                .strong()
                                .color(INK),
                        );
                        ui.label(
                            RichText::new(format!("v{}", item.manifest.version))
                                .size(11.0)
                                .color(BLUE),
                        );
                        status_badge(ui, &item.status, item.system_satisfied);
                    });
                    ui.label(
                        RichText::new(component_description(&item.manifest.id))
                            .size(12.0)
                            .color(MUTED),
                    );
                    if item.system_satisfied && matches!(item.status, PackageStatus::Ready) {
                        ui.label(
                            RichText::new("系统已有兼容能力，因此默认不勾选；仍可安装固定组件以获得隔离和可复现版本。")
                                .size(10.5)
                                .color(SUCCESS),
                        );
                    }
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            human_bytes(item.archive_bytes),
                            item.path.display()
                        ))
                        .size(10.5)
                        .color(Color32::from_rgb(130, 143, 166)),
                    );
                    if let PackageStatus::Failed(error) = &item.status {
                        ui.label(
                            RichText::new(error)
                                .size(11.0)
                                .color(Color32::from_rgb(190, 63, 72)),
                        );
                    }
                });
            });
        });
}

fn status_badge(ui: &mut egui::Ui, status: &PackageStatus, system_satisfied: bool) {
    let (label, color) = match status {
        PackageStatus::Ready if system_satisfied => ("系统可用 · 可选", SUCCESS),
        PackageStatus::Ready => ("可安装", BLUE),
        PackageStatus::AlreadyInstalled => ("已安装", SUCCESS),
        PackageStatus::Installing => ("安装中", CYAN),
        PackageStatus::Installed => ("安装完成", SUCCESS),
        PackageStatus::Incompatible => ("平台不兼容", WARNING),
        PackageStatus::Failed(_) => ("失败", Color32::from_rgb(190, 63, 72)),
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.1))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(10.0).strong().color(color));
        });
}

fn primary(text: &'static str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
        .fill(BLUE)
        .stroke(Stroke::NONE)
        .corner_radius(8)
}

fn primary_button(ui: &mut egui::Ui, text: &'static str) -> egui::Response {
    ui.add(primary(text))
}

fn component_priority(id: &str) -> u8 {
    match id {
        "org.drpa.python-runtime" => 10,
        "org.drpa.browser.chromium" => 20,
        WEBVIEW_COMPONENT_ID => 30,
        DESKTOP_COMPONENT_ID => 40,
        "org.drpa.jcode" => 50,
        _ => 100,
    }
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn component_description(id: &str) -> &'static str {
    match id {
        "org.drpa.python-runtime" => "Python、Jupyter、文档处理与 RPAZ 执行环境",
        "org.drpa.browser.chromium" => "Agent 与 RPA 使用的隔离 Chromium 浏览器",
        WEBVIEW_COMPONENT_ID => "不依赖系统安装的固定版 WebView2 桌面内核",
        DESKTOP_COMPONENT_ID => "DRPA Next 图形工作台与完整交互界面",
        "org.drpa.jcode" => "可独立运行的 JCode 开发 Agent",
        _ => "DRPA 扩展组件",
    }
}

fn automatic_scan_roots(layout: Option<&InstallLayout>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = env::current_exe()
        && let Some(parent) = executable.parent()
    {
        roots.push(parent.to_path_buf());
    }
    if let Ok(current) = env::current_dir() {
        roots.push(current);
    }
    if let Some(layout) = layout {
        roots.push(layout.root().to_path_buf());
    }
    if let Some(profile) = env::var_os("USERPROFILE") {
        let profile = PathBuf::from(profile);
        roots.push(profile.join("Downloads"));
        roots.push(profile.join("Desktop"));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join("Downloads"));
        roots.push(home.join("Desktop"));
        roots.push(home.join("下载"));
        roots.push(home.join("桌面"));
    }
    roots
}

fn discover_component_packs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut packs = BTreeSet::new();
    for root in roots {
        if root.is_file() {
            if is_component_pack(root) {
                packs.insert(root.to_path_buf());
            }
            continue;
        }
        for directory in [
            root.to_path_buf(),
            root.join("components"),
            root.join("component-packs"),
        ] {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_component_pack(&path) {
                    packs.insert(path);
                }
            }
        }
    }
    packs.into_iter().collect()
}

fn is_component_pack(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("drpac"))
}

fn preferred_start_directory(layout: Option<&InstallLayout>) -> PathBuf {
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|path| path.join("Downloads"))
        .filter(|path| path.is_dir())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join("Downloads"))
                .filter(|path| path.is_dir())
        })
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join("下载"))
                .filter(|path| path.is_dir())
        })
        .or_else(|| layout.map(|layout| layout.root().to_path_buf()))
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn system_font_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(windows) = env::var_os("WINDIR") {
        let fonts = PathBuf::from(windows).join("Fonts");
        for name in ["msyh.ttc", "msyh.ttf", "simhei.ttf", "simsun.ttc"] {
            candidates.push(fonts.join(name));
        }
    }
    for path in [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ] {
        candidates.push(PathBuf::from(path));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn human_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * MIB;
    if bytes as f64 >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB)
    } else if bytes as f64 >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[derive(Clone)]
struct FileEntry {
    path: PathBuf,
    is_directory: bool,
}

struct FileBrowser {
    visible: bool,
    current: PathBuf,
    path_text: String,
    entries: Vec<FileEntry>,
    selected: Option<PathBuf>,
    error: Option<String>,
}

impl FileBrowser {
    fn new(current: PathBuf) -> Self {
        let mut result = Self {
            visible: false,
            path_text: current.display().to_string(),
            current,
            entries: Vec::new(),
            selected: None,
            error: None,
        };
        result.refresh();
        result
    }

    fn open(&mut self) {
        self.visible = true;
        self.refresh();
    }

    fn navigate(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.current = path;
            self.path_text = self.current.display().to_string();
            self.selected = None;
            self.refresh();
        } else {
            self.error = Some(format!("目录不存在：{}", path.display()));
        }
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;
        match fs::read_dir(&self.current) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() || is_component_pack(&path) {
                        self.entries.push(FileEntry {
                            is_directory: path.is_dir(),
                            path,
                        });
                    }
                }
                self.entries.sort_by(|left, right| {
                    right
                        .is_directory
                        .cmp(&left.is_directory)
                        .then_with(|| left.path.file_name().cmp(&right.path.file_name()))
                });
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn show(&mut self, context: &egui::Context) -> Option<PathBuf> {
        if !self.visible {
            return None;
        }
        let mut chosen = None;
        let mut visible = self.visible;
        egui::Window::new("选择 DRPA 组件包")
            .open(&mut visible)
            .collapsible(false)
            .resizable(true)
            .default_size([700.0, 480.0])
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("← 上一级").clicked()
                        && let Some(parent) = self.current.parent()
                    {
                        self.navigate(parent.to_path_buf());
                    }
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.path_text)
                            .desired_width(f32::INFINITY),
                    );
                    if (response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                        || ui.button("转到").clicked()
                    {
                        self.navigate(PathBuf::from(self.path_text.trim()));
                    }
                });
                #[cfg(windows)]
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("磁盘：").size(11.0).color(MUTED));
                    for letter in b'A'..=b'Z' {
                        let root = PathBuf::from(format!("{}:\\", letter as char));
                        if root.is_dir()
                            && ui.small_button(format!("{}:", letter as char)).clicked()
                        {
                            self.navigate(root);
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(ui, |ui| {
                        for entry in self.entries.clone() {
                            let name = entry
                                .path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| entry.path.display().to_string());
                            let selected = self
                                .selected
                                .as_ref()
                                .is_some_and(|path| same_path(path, &entry.path));
                            let response = ui.selectable_label(
                                selected,
                                if entry.is_directory {
                                    format!("📁 {name}")
                                } else {
                                    format!("◈ {name}")
                                },
                            );
                            if response.clicked() {
                                self.selected = Some(entry.path.clone());
                            }
                            if response.double_clicked() {
                                if entry.is_directory {
                                    self.navigate(entry.path);
                                } else {
                                    chosen = Some(entry.path);
                                }
                            }
                        }
                    });
                if let Some(error) = &self.error {
                    ui.label(
                        RichText::new(error)
                            .size(11.0)
                            .color(Color32::from_rgb(190, 63, 72)),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("只显示文件夹和 .drpac 文件")
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_add = self
                            .selected
                            .as_ref()
                            .is_some_and(|path| is_component_pack(path) && path.is_file());
                        if ui.add_enabled(can_add, primary("添加组件包")).clicked() {
                            chosen = self.selected.clone();
                        }
                    });
                });
            });
        self.visible = visible && chosen.is_none();
        chosen
    }
}

#[cfg(windows)]
fn show_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let title = wide("DRPA 组件安装向导");
    let message = wide(message);
    // SAFETY: both UTF-16 buffers are NUL-terminated and remain alive for the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_error(message: &str) {
    eprintln!("DRPA 组件安装向导：{message}");
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discovers_only_immediate_component_packages_and_known_subdirectories() {
        let temporary = TempDir::new().unwrap();
        fs::write(temporary.path().join("desktop.drpac"), b"pack").unwrap();
        fs::write(temporary.path().join("ignore.zip"), b"zip").unwrap();
        fs::create_dir_all(temporary.path().join("components")).unwrap();
        fs::write(temporary.path().join("components/python.drpac"), b"pack").unwrap();
        fs::create_dir_all(temporary.path().join("nested/deep")).unwrap();
        fs::write(temporary.path().join("nested/deep/hidden.drpac"), b"pack").unwrap();

        let packs = discover_component_packs(&[temporary.path().to_path_buf()]);

        assert_eq!(packs.len(), 2);
        assert!(packs.iter().all(|path| is_component_pack(path)));
    }

    #[test]
    fn parses_setup_source_and_explicit_packages() {
        let arguments = Arguments::parse([
            "--install-root".into(),
            "D:/DRPA Next".into(),
            "--scan".into(),
            "D:/Downloads".into(),
            "--required".into(),
            "org.drpa.webview2-fixed".into(),
            "D:/desktop.drpac".into(),
        ]);

        assert_eq!(arguments.install_root, Some(PathBuf::from("D:/DRPA Next")));
        assert_eq!(arguments.scan_roots, vec![PathBuf::from("D:/Downloads")]);
        assert_eq!(arguments.packages, vec![PathBuf::from("D:/desktop.drpac")]);
        assert_eq!(
            arguments.required_component.as_deref(),
            Some("org.drpa.webview2-fixed")
        );
    }

    #[test]
    fn component_versions_use_semver_ordering() {
        assert_eq!(
            compare_versions("2.10.0", "2.9.0"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_versions("2.1.1-beta.1", "2.1.1"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn system_capabilities_make_matching_component_optional() {
        assert!(component_satisfied_by_system(
            WEBVIEW_COMPONENT_ID,
            true,
            false
        ));
        assert!(component_satisfied_by_system(
            BROWSER_COMPONENT_ID,
            false,
            true
        ));
        assert!(!component_satisfied_by_system(
            "org.drpa.desktop-ui",
            true,
            true
        ));
    }

    #[test]
    fn component_navigation_can_skip_when_nothing_needs_installing() {
        assert_eq!(component_next_action(false, 0), ComponentNextAction::Skip);
        assert_eq!(
            component_next_action(false, 2),
            ComponentNextAction::Install
        );
        assert_eq!(component_next_action(true, 0), ComponentNextAction::Wait);
    }
}
