use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering as AtomicOrdering},
};

use drpa_host::{HostState, RunLaunch};
use drpa_package::{Entrypoint, PackageManifest, safe_relative_path, validate_package_id};
use drpa_protocol::{
    PackageSummary, RUNTIME_PROTOCOL_VERSION, RuntimeEvent, WindowsUpdatePhase,
    WindowsUpdateSession, WindowsUpdateStatus, WorkspaceSnapshot,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use uuid::Uuid;
use zip::{ZipArchive, write::SimpleFileOptions};

mod agent;
mod agent_browser;
mod agent_config;
mod agent_context;
mod agent_documents;
mod agent_extensions;
mod agent_runtime;
mod agent_sessions;
mod agent_tools;
mod automations;
mod credential_vault;
mod dashboard;
mod database;
mod jcode;
mod knowledge;
mod knowledge_base;
mod local_dify;
mod local_dify_workflow;
#[cfg(windows)]
mod native_splash;
mod plugins;
mod provider;
mod python_flow;
mod system_metrics;
mod workspaces;

const WINDOWS_UPDATE_SCHEMA: u32 = 2;
const WINDOWS_UPDATE_HOST_PROTOCOL: u32 = 2;
const WINDOWS_UPDATE_WORKER_PROTOCOL: u32 = 2;

#[derive(Clone)]
pub(crate) struct AppPaths {
    pub(crate) data_root: PathBuf,
    pub(crate) workspace_root: PathBuf,
    resource_dir: Option<PathBuf>,
}

#[derive(Clone)]
struct StudioKernelManager {
    sessions: Arc<Mutex<HashMap<String, StudioKernel>>>,
}

#[derive(Clone, Default)]
struct RunProcessManager {
    #[cfg(target_os = "linux")]
    process_groups: Arc<Mutex<HashMap<String, LinuxRunProcessGroup>>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupProgress {
    progress: u8,
    label: String,
    current_file: String,
    phase: String,
    revision: u64,
}

struct StartupState {
    started_at: std::time::Instant,
    reveal_requested: Arc<AtomicBool>,
    progress: Mutex<StartupProgress>,
}

impl StartupState {
    fn update(&self, progress: u8, label: &str, current_file: &str, phase: &str) {
        if let Ok(mut status) = self.progress.lock() {
            status.progress = progress.min(100);
            status.label = label.to_owned();
            status.current_file = current_file.to_owned();
            status.phase = phase.to_owned();
            status.revision = status.revision.saturating_add(1);
        }
    }

    fn snapshot(&self) -> StartupProgress {
        self.progress
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| StartupProgress {
                progress: 0,
                label: "正在恢复启动状态".to_owned(),
                current_file: "desktop://startup-state".to_owned(),
                phase: "loading".to_owned(),
                revision: 0,
            })
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct LinuxRunProcessGroup {
    id: u32,
    cancelling: bool,
}

impl RunProcessManager {
    fn register(&self, run_id: &str, process_group: u32) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        self.process_groups
            .lock()
            .map_err(|_| "任务进程状态已损坏".to_owned())?
            .insert(
                run_id.to_owned(),
                LinuxRunProcessGroup {
                    id: process_group,
                    cancelling: false,
                },
            );
        #[cfg(not(target_os = "linux"))]
        let _ = (run_id, process_group);
        Ok(())
    }

    fn unregister(&self, run_id: &str, process_group: u32) {
        #[cfg(target_os = "linux")]
        if let Ok(mut groups) = self.process_groups.lock()
            && groups
                .get(run_id)
                .is_some_and(|group| group.id == process_group && !group.cancelling)
        {
            groups.remove(run_id);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (run_id, process_group);
    }

    fn cancel(&self, run_id: &str) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            let process_group = {
                let mut groups = self
                    .process_groups
                    .lock()
                    .map_err(|_| "任务进程状态已损坏".to_owned())?;
                groups.get_mut(run_id).map(|group| {
                    group.cancelling = true;
                    group.id
                })
            };
            if let Some(process_group) = process_group {
                signal_linux_process_group(process_group, libc::SIGTERM)?;
                let groups = Arc::clone(&self.process_groups);
                let run_id = run_id.to_owned();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let still_running = groups.lock().ok().and_then(|groups| {
                        groups
                            .get(&run_id)
                            .filter(|group| group.id == process_group && group.cancelling)
                            .map(|group| group.id)
                    });
                    if still_running.is_some() {
                        let _ = signal_linux_process_group(process_group, libc::SIGKILL);
                    }
                    if let Ok(mut groups) = groups.lock()
                        && groups
                            .get(&run_id)
                            .is_some_and(|group| group.id == process_group)
                    {
                        groups.remove(&run_id);
                    }
                });
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = run_id;
        Ok(())
    }
}

struct StudioKernel {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Drop for StudioKernel {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        let _ = signal_linux_process_group(self.child.id(), libc::SIGKILL);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioProject {
    id: String,
    name: String,
    files: Vec<String>,
}

#[derive(Deserialize)]
struct StudioKernelResponse {
    request_id: String,
    execution_count: u64,
    stdout: String,
    stderr: String,
    result: Option<String>,
    error: Option<String>,
    traceback: Vec<String>,
    outputs: Vec<serde_json::Value>,
    variables: Vec<StudioVariable>,
    duration_ms: u64,
}

#[derive(Deserialize)]
struct StudioKernelCompletionResponse {
    request_id: String,
    matches: Vec<String>,
    cursor_start: usize,
    cursor_end: usize,
    metadata: serde_json::Value,
    status: String,
}

#[derive(Deserialize)]
struct StudioKernelInspectResponse {
    request_id: String,
    found: bool,
    data: serde_json::Value,
    metadata: serde_json::Value,
    status: String,
}

#[derive(Deserialize, Serialize)]
struct StudioVariable {
    name: String,
    #[serde(rename(deserialize = "type_name", serialize = "typeName"))]
    type_name: String,
    preview: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioCellResult {
    execution_count: u64,
    stdout: String,
    stderr: String,
    result: Option<String>,
    error: Option<String>,
    traceback: Vec<String>,
    outputs: Vec<serde_json::Value>,
    variables: Vec<StudioVariable>,
    duration_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioCompletionResult {
    matches: Vec<String>,
    cursor_start: usize,
    cursor_end: usize,
    metadata: serde_json::Value,
    status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioInspectResult {
    found: bool,
    data: serde_json::Value,
    metadata: serde_json::Value,
    status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentUser {
    display_name: String,
    account_name: String,
    initials: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDataTransferResult {
    path: String,
    file_count: usize,
    total_bytes: u64,
    workspace_name: String,
    restart_required: bool,
}

struct TemporaryFileCleanup(PathBuf);

impl Drop for TemporaryFileCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct TemporaryDirectoryCleanup(PathBuf);

impl Drop for TemporaryDirectoryCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tauri::command]
fn get_current_user() -> CurrentUser {
    let account_name = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Local User".to_owned());
    let display_name = account_name.clone();
    let parts = display_name.split_whitespace().collect::<Vec<_>>();
    let initials = if parts.len() > 1 {
        parts
            .iter()
            .filter_map(|part| part.chars().next())
            .take(2)
            .collect()
    } else {
        display_name.chars().take(2).collect()
    };
    CurrentUser {
        display_name,
        account_name,
        initials,
    }
}

#[tauri::command]
fn get_workspace_snapshot(state: State<'_, HostState>) -> WorkspaceSnapshot {
    state.snapshot()
}

fn write_ui_test_marker(
    environment_variable: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    if let Some(marker) = std::env::var_os(environment_variable) {
        let target = PathBuf::from(marker);
        if !target.is_absolute() {
            return Err(format!("{environment_variable} 必须是绝对路径"));
        }
        let parent = target
            .parent()
            .ok_or_else(|| "无法定位 UI 就绪标记目录".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| format!("创建 UI 就绪目录失败：{error}"))?;
        let temporary = parent.join(format!(".drpa-ui-ready-{}.tmp", Uuid::new_v4().simple()));
        let payload = serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?;
        fs::write(&temporary, payload).map_err(|error| format!("写入 UI 就绪标记失败：{error}"))?;
        fs::rename(&temporary, &target)
            .map_err(|error| format!("提交 UI 就绪标记失败：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
fn report_ui_ready() -> Result<(), String> {
    write_ui_test_marker(
        "DRPA_UI_READY_FILE",
        serde_json::json!({
            "schemaVersion": 1,
            "reactMounted": true,
            "ipcRoundTrip": true,
            "pid": std::process::id(),
        }),
    )
}

#[tauri::command]
fn get_startup_status(startup: State<'_, StartupState>) -> StartupProgress {
    startup.snapshot()
}

#[tauri::command]
fn report_startup_frontend_error(message: String, app: tauri::AppHandle) {
    let message = message.chars().take(700).collect::<String>();
    set_startup_error_handle(&app, "界面加载失败", &message);
}

#[tauri::command]
fn complete_startup(app: tauri::AppHandle, startup: State<'_, StartupState>) -> Result<(), String> {
    if startup.reveal_requested.swap(true, AtomicOrdering::AcqRel) {
        return Ok(());
    }
    let remaining = std::time::Duration::from_secs(2).saturating_sub(startup.started_at.elapsed());
    schedule_main_window_reveal(app, remaining);
    Ok(())
}

#[cfg(windows)]
fn complete_startup_from_handle(app: &tauri::AppHandle) {
    let Some(startup) = app.try_state::<StartupState>() else {
        return;
    };
    if startup.snapshot().phase == "error"
        || startup.reveal_requested.swap(true, AtomicOrdering::AcqRel)
    {
        return;
    }
    let remaining = std::time::Duration::from_secs(2).saturating_sub(startup.started_at.elapsed());
    schedule_main_window_reveal(app.clone(), remaining);
}

fn schedule_main_window_reveal(app: tauri::AppHandle, remaining: std::time::Duration) {
    std::thread::spawn(move || {
        if !remaining.is_zero() {
            std::thread::sleep(remaining);
        }
        set_startup_progress_handle(&app, 100, "启动完成", "ui://ready");
        std::thread::sleep(std::time::Duration::from_millis(120));
        reveal_main_window(&app);
    });
}

fn reveal_main_window(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    #[cfg(windows)]
    if let Some(splash) = app.try_state::<native_splash::NativeSplash>() {
        splash.close();
    }
    if let Some(splash) = app.get_webview_window("splashscreen") {
        let _ = splash.close();
    }
}

fn set_startup_progress_handle(
    app: &tauri::AppHandle,
    progress: u8,
    label: &str,
    current_file: &str,
) {
    let progress = app
        .try_state::<StartupState>()
        .map(|startup| {
            let progress = progress.max(startup.snapshot().progress).min(100);
            startup.update(
                progress,
                label,
                current_file,
                if progress >= 100 { "ready" } else { "loading" },
            );
            progress
        })
        .unwrap_or_else(|| progress.min(100));
    #[cfg(windows)]
    if let Some(splash) = app.try_state::<native_splash::NativeSplash>() {
        if progress >= 100 {
            splash.ready(label, current_file);
        } else {
            splash.update(progress, label, current_file);
        }
    }
    let status = serde_json::json!({
        "progress": progress.min(100),
        "label": label,
        "currentFile": current_file,
        "phase": if progress >= 100 { "ready" } else { "loading" },
    });
    let Ok(status) = serde_json::to_string(&status) else {
        return;
    };
    if let Some(splash) = app.get_webview_window("splashscreen") {
        let _ = splash.eval(format!("window.__DRPA_STARTUP_STATUS__ = {status};"));
    }
}

fn set_startup_error_handle(app: &tauri::AppHandle, label: &str, detail: &str) {
    let progress = app
        .try_state::<StartupState>()
        .map(|startup| {
            let progress = startup.snapshot().progress;
            startup.update(progress, label, detail, "error");
            progress
        })
        .unwrap_or(0);
    #[cfg(windows)]
    if let Some(splash) = app.try_state::<native_splash::NativeSplash>() {
        splash.fail(label, detail);
    }
    let status = serde_json::json!({
        "progress": progress,
        "label": label,
        "currentFile": detail,
        "phase": "error",
    });
    if let Ok(status) = serde_json::to_string(&status)
        && let Some(splash) = app.get_webview_window("splashscreen")
    {
        let _ = splash.eval(format!("window.__DRPA_STARTUP_STATUS__ = {status};"));
    }
}

#[tauri::command]
fn report_ui_input_ready() -> Result<(), String> {
    write_ui_test_marker(
        "DRPA_UI_INPUT_READY_FILE",
        serde_json::json!({
            "schemaVersion": 1,
            "nativeInputTyped": true,
            "postInputClick": true,
            "ipcRoundTrip": true,
            "pid": std::process::id(),
        }),
    )
}

#[tauri::command(async)]
fn install_package(
    archive_path: String,
    state: State<'_, HostState>,
) -> Result<PackageSummary, String> {
    state
        .install_package(Path::new(&archive_path))
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn uninstall_package(package_id: String, state: State<'_, HostState>) -> Result<(), String> {
    state
        .uninstall_package(&package_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_run(
    package_id: String,
    profile_id: String,
    parameters: serde_json::Value,
    state: State<'_, HostState>,
    paths: State<'_, AppPaths>,
    processes: State<'_, RunProcessManager>,
) -> Result<String, String> {
    dispatch_run_background(
        state.inner().clone(),
        paths.inner().clone(),
        processes.inner().clone(),
        &package_id,
        &profile_id,
        parameters,
    )
}

pub(crate) fn dispatch_run_background(
    state: HostState,
    paths: AppPaths,
    processes: RunProcessManager,
    package_id: &str,
    profile_id: &str,
    parameters: serde_json::Value,
) -> Result<String, String> {
    let launch = state
        .prepare_run(package_id, profile_id, &parameters)
        .map_err(|error| error.to_string())?;
    let run_id = launch.run_id.clone();
    let background_state = state;
    let background_paths = paths;
    let background_processes = processes;
    let background_run_id = run_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = execute_python_run(
            &background_state,
            &background_paths,
            &background_processes,
            &launch,
            &parameters,
        ) && !background_state.run_is_cancelled(&background_run_id)
        {
            background_state.fail_run(&background_run_id, error);
        }
    });
    Ok(run_id)
}

pub(crate) fn dispatch_automation_run(
    state: &HostState,
    paths: &AppPaths,
    processes: &RunProcessManager,
    plan: &automations::AutomationPlan,
) -> Result<serde_json::Value, String> {
    if plan.action.kind != "rpaz-package" {
        return Err(format!("暂不支持的自动化动作：{}", plan.action.kind));
    }
    let profile_id = if plan.action.entrypoint.trim().is_empty() {
        "default"
    } else {
        plan.action.entrypoint.trim()
    };
    let mut parameters = plan.action.parameters.clone();
    if !parameters.is_object() {
        parameters = serde_json::json!({"value": parameters});
    }
    if let Some(values) = parameters.as_object_mut() {
        values.insert(
            "_drpa_automation".to_owned(),
            serde_json::json!({
                "planId": plan.id,
                "planName": plan.name,
                "deliveryTargets": plan.delivery_targets,
            }),
        );
    }
    let launch = state
        .prepare_run(&plan.action.package_id, profile_id, &parameters)
        .map_err(|error| error.to_string())?;
    let run_id = launch.run_id.clone();
    if let Err(error) = execute_python_run(state, paths, processes, &launch, &parameters) {
        if !state.run_is_cancelled(&run_id) {
            state.fail_run(&run_id, error.clone());
        }
        return Err(error);
    }
    let detail = state
        .get_run_detail(&run_id)
        .map_err(|error| error.to_string())?;
    if !matches!(detail.summary.status, drpa_protocol::RunStatus::Success) {
        return Err(detail
            .error_message
            .unwrap_or_else(|| format!("RPAZ 自动化运行未成功：{run_id}")));
    }
    Ok(serde_json::json!({
        "runId": run_id,
        "status": "success",
        "outputDirectory": detail.output_dir,
        "artifacts": detail.artifacts,
    }))
}

#[tauri::command]
fn cancel_run(
    run_id: String,
    state: State<'_, HostState>,
    processes: State<'_, RunProcessManager>,
) -> Result<(), String> {
    state
        .cancel_run(&run_id)
        .map_err(|error| error.to_string())?;
    processes.cancel(&run_id)
}

#[tauri::command(async)]
fn get_run_detail(
    run_id: String,
    state: State<'_, HostState>,
) -> Result<drpa_protocol::RunDetail, String> {
    state
        .get_run_detail(&run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn open_run_output_directory(
    run_id: String,
    state: State<'_, HostState>,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let detail = state
        .get_run_detail(&run_id)
        .map_err(|error| error.to_string())?;
    let output = PathBuf::from(detail.output_dir);
    let runs_root = paths.workspace_root.join("runs");
    fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    fs::create_dir_all(&runs_root).map_err(|error| error.to_string())?;
    let output = output
        .canonicalize()
        .map_err(|error| format!("定位运行输出目录失败：{error}"))?;
    let runs_root = runs_root
        .canonicalize()
        .map_err(|error| format!("定位运行记录目录失败：{error}"))?;
    if !output.starts_with(&runs_root) {
        return Err("运行输出目录超出工作区".to_owned());
    }
    open_directory_in_file_explorer(&output)
}

#[tauri::command(async)]
fn list_studio_projects(paths: State<'_, AppPaths>) -> Result<Vec<StudioProject>, String> {
    let projects_root = paths.workspace_root.join("projects");
    fs::create_dir_all(&projects_root).map_err(|error| error.to_string())?;
    let mut projects = Vec::new();
    for entry in fs::read_dir(projects_root)
        .map_err(|error| error.to_string())?
        .flatten()
        .filter(|entry| entry.path().is_dir())
    {
        let id = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = entry.path().join("manifest.yaml");
        let name = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|source| PackageManifest::from_yaml(&source).ok())
            .map_or_else(|| id.clone(), |manifest| manifest.name);
        let mut files = Vec::new();
        collect_project_entries(&entry.path(), &entry.path(), &mut files)
            .map_err(|error| error.to_string())?;
        files.sort();
        projects.push(StudioProject { id, name, files });
    }
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(projects)
}

#[tauri::command(async)]
fn create_studio_project(
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<StudioProject, String> {
    create_studio_project_in_workspace(&name, &paths.workspace_root)
}

fn create_studio_project_in_workspace(
    name: &str,
    workspace_root: &Path,
) -> Result<StudioProject, String> {
    if name.trim().is_empty() {
        return Err("项目名称不能为空".to_owned());
    }
    let (project_id, package_id) = generated_project_ids(name.trim());
    let root = workspace_root.join("projects").join(&project_id);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let yaml_name = serde_json::to_string(name.trim()).map_err(|error| error.to_string())?;
    let manifest = format!(
        "schema: 2\nid: {package_id}\nname: {yaml_name}\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\nruntime:\n  python: \"3.11.*\"\ncapabilities:\n  network:\n    allow: []\n  filesystem:\n    read: []\n    write: [\"$outputs\"]\nparameters: []\n"
    );
    fs::write(root.join("manifest.yaml"), manifest).map_err(|error| error.to_string())?;
    fs::write(
        root.join("main.py"),
        "from drpa_runner import RuntimeContext\n\n\ndef main(ctx: RuntimeContext) -> None:\n    ctx.log.info(\"任务开始\")\n    ctx.progress(100, \"任务完成\")\n",
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        root.join("notebook.ipynb"),
        r##"{
  "cells": [
    {
      "cell_type": "code",
      "execution_count": null,
      "metadata": {},
      "outputs": [],
      "source": ["# 使用内置 sealed Python Kernel 交互开发\n", "message = '你好，DRPA Notebook'\n", "message"]
    }
  ],
  "metadata": {
    "kernelspec": {"display_name": "DRPA Python 3.11", "language": "python", "name": "drpa-python"},
    "language_info": {"name": "python", "version": "3.11"}
  },
  "nbformat": 4,
  "nbformat_minor": 5
}
"##,
    )
    .map_err(|error| error.to_string())?;
    ensure_studio_project_readme(&root, name.trim())?;
    Ok(StudioProject {
        id: project_id,
        name: name.trim().to_owned(),
        files: vec![
            "README.md".to_owned(),
            "main.py".to_owned(),
            "manifest.yaml".to_owned(),
            "notebook.ipynb".to_owned(),
        ],
    })
}

fn studio_project_readme(project_name: &str) -> String {
    let project_name = escape_markdown_text(project_name);
    format!(
        r#"# {project_name}

> DRPA Studio 为本项目生成的 AI 开发入口。代码、清单与本文档共同构成项目上下文。

## AI 开发入口

开始分析或修改前，按顺序阅读：`manifest.yaml` → `main.py` → `README.md`。先确认包契约和入口，再规划实现；不要只根据文件名猜测行为。

## RPAZ schema 2 约定

- `manifest.yaml` 必须保留 `schema: 2`，并维护稳定的 `id`、`version`、Python `entrypoint`、`capabilities` 与 `parameters`。
- `main.py` 必须提供清单声明的 callable，默认签名为 `main(ctx)`；模块导入阶段避免执行网络、浏览器、数据库或文件写入。
- 参数来自 manifest，代码通过 `ctx.params` 读取；新增能力时同步收紧或补充 capabilities。

## ctx 默认能力

- `ctx.params`：读取任务配置参数。
- `ctx.log`：输出结构化运行日志。
- `ctx.progress(...)`：实时报告 0–100 的进度和当前阶段。
- `ctx.output_file(...)`：在隔离输出目录创建并登记产物。
- `ctx.open_output_directory()`：按任务参数决定是否展示输出目录。
- `ctx.sql`：访问工作区 SQLite，使用参数化 SQL 和事务。
- `ctx.browser(...)`：连接 DRPA 管理的可复用 DrissionPage 浏览器会话。
- `ctx.invoke(...)`：按包 ID 调用已安装 RPAZ 包；保持参数和返回值可序列化。

## 离线依赖

- 目标用户环境默认离线。代码只使用 DRPA sealed runtime 中已经锁定并随平台交付的依赖，不在运行代码中调用 pip，也不依赖系统 Python。
- RPA for Python 已由平台运行时提供，自动化代码使用 `import rpa as r` 引入；项目自身不执行在线安装。
- 新增平台级 Python 依赖时，更新 DRPA 源码仓库中的 `offline/requirements/runtime.txt` 与对应运行时构建锁，然后重新构建、验证并发布全量 sealed runtime。
- Windows 与 UOS/Linux 均需验证 Python 3.11 ABI，代码应避免写死解释器、浏览器及工作区绝对路径。

## 测试与导出

1. 在 Studio 使用“直接运行”验证当前工作副本，修复 manifest、入口和参数问题。
2. 运行最小参数、边界参数与失败路径，检查实时日志、进度、产物和工作区 SQLite 变更。
3. 对纯函数和外部接口适配层补充测试；外部服务使用可替换夹具，确保离线测试可重复。
4. 点击“导出 RPAZ”，安装生成包；再到运行工作台创建任务配置，执行“预检并保存”和试运行。

## Python Flow 协作约定

- Python 源码是业务逻辑和执行行为的唯一事实源（SSOT）；Python Flow 只承载可视化编排、节点位置和源码映射元数据，不维护第二套隐式业务逻辑。
- 把可视化步骤拆成命名稳定、输入输出显式的函数，由 `main(ctx)` 负责薄编排；避免隐藏全局状态、动态 `exec` 和导入期副作用。
- AI 修改函数名、参数或返回结构时，同步更新对应节点映射；可视化编辑回写代码后，重新格式化、预检并运行测试。
- 手写代码与生成代码都应保留清晰边界。遇到无法映射的 Python 语义时保留为“代码节点”，不要静默丢弃行为。
"#
    )
}

fn ensure_studio_project_readme(root: &Path, project_name: &str) -> Result<(), String> {
    let readme = root.join("README.md");
    if !readme.is_file() {
        fs::write(readme, studio_project_readme(project_name))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn escape_markdown_text(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut escaped = String::with_capacity(normalized.len());
    for character in normalized.chars() {
        if matches!(
            character,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '<'
                | '>'
                | '('
                | ')'
                | '#'
                | '+'
                | '-'
                | '.'
                | '!'
                | '|'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[tauri::command(async)]
fn rename_studio_project(
    project_id: String,
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<StudioProject, String> {
    validate_project_id(&project_id)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名称不能为空".to_owned());
    }
    if name.chars().count() > 120 {
        return Err("项目名称不能超过 120 个字符".to_owned());
    }
    let root = paths.workspace_root.join("projects").join(&project_id);
    if !root.is_dir() {
        return Err("开发项目不存在".to_owned());
    }
    let manifest_path = root.join("manifest.yaml");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("读取 manifest.yaml 失败：{error}"))?;
    PackageManifest::from_yaml(&source).map_err(|error| error.to_string())?;
    let quoted_name = serde_json::to_string(name).map_err(|error| error.to_string())?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let trailing_newline = source.ends_with('\n');
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in source.lines() {
        if !replaced
            && !line.chars().next().is_some_and(char::is_whitespace)
            && line
                .split_once(':')
                .is_some_and(|(key, _)| key.trim() == "name")
        {
            lines.push(format!("name: {quoted_name}"));
            replaced = true;
        } else {
            lines.push(line.to_owned());
        }
    }
    if !replaced {
        return Err("manifest.yaml 缺少顶层 name 字段".to_owned());
    }
    let mut updated = lines.join(newline);
    if trailing_newline {
        updated.push_str(newline);
    }
    let manifest = PackageManifest::from_yaml(&updated).map_err(|error| error.to_string())?;
    fs::write(&manifest_path, updated).map_err(|error| format!("保存项目名称失败：{error}"))?;
    let mut files = Vec::new();
    collect_project_entries(&root, &root, &mut files).map_err(|error| error.to_string())?;
    files.sort();
    Ok(StudioProject {
        id: project_id,
        name: manifest.name,
        files,
    })
}

#[tauri::command(async)]
fn read_project_file(
    project_id: String,
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    validate_project_id(&project_id)?;
    let relative = safe_relative_path(&relative_path).map_err(|error| error.to_string())?;
    fs::read_to_string(
        paths
            .workspace_root
            .join("projects")
            .join(project_id)
            .join(relative),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn write_project_file(
    project_id: String,
    relative_path: String,
    content: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_project_id(&project_id)?;
    let relative = safe_relative_path(&relative_path).map_err(|error| error.to_string())?;
    let target = paths
        .workspace_root
        .join("projects")
        .join(project_id)
        .join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(target, content).map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn create_project_directory(
    project_id: String,
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_project_id(&project_id)?;
    let relative = safe_relative_path(&relative_path).map_err(|error| error.to_string())?;
    fs::create_dir_all(
        paths
            .workspace_root
            .join("projects")
            .join(project_id)
            .join(relative),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn rename_project_entry(
    project_id: String,
    relative_path: String,
    target_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_project_id(&project_id)?;
    let source_relative = safe_relative_path(relative_path.trim_end_matches('/'))
        .map_err(|error| error.to_string())?;
    let target_relative =
        safe_relative_path(target_path.trim_end_matches('/')).map_err(|error| error.to_string())?;
    let project_root = paths.workspace_root.join("projects").join(project_id);
    let source = project_root.join(source_relative);
    let target = project_root.join(target_relative);
    if !source.exists() {
        return Err("要重命名的文件或文件夹不存在".to_owned());
    }
    if target.exists() {
        return Err("目标名称已存在".to_owned());
    }
    let parent = target.parent().ok_or_else(|| "目标路径无效".to_owned())?;
    if !parent.is_dir() {
        return Err("目标文件夹不存在".to_owned());
    }
    fs::rename(source, target).map_err(|error| format!("重命名失败：{error}"))
}

#[tauri::command(async)]
fn delete_project_entry(
    project_id: String,
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_project_id(&project_id)?;
    let relative = safe_relative_path(relative_path.trim_end_matches('/'))
        .map_err(|error| error.to_string())?;
    let target = paths
        .workspace_root
        .join("projects")
        .join(project_id)
        .join(relative);
    if target.is_dir() {
        fs::remove_dir_all(target).map_err(|error| format!("删除文件夹失败：{error}"))
    } else if target.is_file() {
        fs::remove_file(target).map_err(|error| format!("删除文件失败：{error}"))
    } else {
        Err("要删除的文件或文件夹不存在".to_owned())
    }
}

#[tauri::command(async)]
fn delete_studio_project(
    project_id: String,
    kernels: State<'_, StudioKernelManager>,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_project_id(&project_id)?;
    kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?
        .remove(&project_id);
    let project_root = paths.workspace_root.join("projects").join(project_id);
    if !project_root.is_dir() {
        return Err("开发项目不存在".to_owned());
    }
    fs::remove_dir_all(project_root).map_err(|error| format!("删除开发项目失败：{error}"))
}

#[tauri::command(async)]
fn import_project_file(
    project_id: String,
    source_path: String,
    target_directory: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    validate_project_id(&project_id)?;
    let source = Path::new(&source_path);
    if !source.is_file() {
        return Err("只能导入单个文件".to_owned());
    }
    let target_relative = if target_directory.trim().is_empty() {
        PathBuf::new()
    } else {
        safe_relative_path(&target_directory).map_err(|error| error.to_string())?
    };
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无法读取导入文件名".to_owned())?;
    let file_name = safe_relative_path(file_name).map_err(|error| error.to_string())?;
    let target = paths
        .workspace_root
        .join("projects")
        .join(project_id)
        .join(&target_relative)
        .join(&file_name);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::copy(source, &target).map_err(|error| error.to_string())?;
    let relative_output = target_relative.join(file_name);
    Ok(relative_output.to_string_lossy().replace('\\', "/"))
}

#[tauri::command(async)]
fn build_studio_project(project_id: String, paths: State<'_, AppPaths>) -> Result<String, String> {
    build_studio_project_inner(&project_id, &paths).map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command(async)]
fn install_studio_project(
    project_id: String,
    paths: State<'_, AppPaths>,
    state: State<'_, HostState>,
) -> Result<PackageSummary, String> {
    let archive = build_studio_project_inner(&project_id, &paths)?;
    state
        .install_package(&archive)
        .map_err(|error| format!("保存到 RPAZ 包库失败：{error}"))
}

fn build_studio_project_inner(project_id: &str, paths: &AppPaths) -> Result<PathBuf, String> {
    validate_project_id(project_id)?;
    let root = paths.workspace_root.join("projects").join(project_id);
    let manifest_source = fs::read_to_string(root.join("manifest.yaml"))
        .map_err(|error| format!("无法读取 manifest.yaml：{error}"))?;
    let manifest =
        PackageManifest::from_yaml(&manifest_source).map_err(|error| error.to_string())?;
    let output_root = paths.workspace_root.join("build");
    fs::create_dir_all(&output_root).map_err(|error| error.to_string())?;
    let output = output_root.join(format!("{}-{}.rpaz", manifest.id, manifest.version));
    let file = File::create(&output).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files).map_err(|error| error.to_string())?;
    files.sort();
    for relative in files {
        let source = root.join(&relative);
        archive
            .start_file(relative.replace('\\', "/"), options)
            .map_err(|error| error.to_string())?;
        let mut content = Vec::new();
        File::open(source)
            .and_then(|mut file| file.read_to_end(&mut content))
            .map_err(|error| error.to_string())?;
        archive
            .write_all(&content)
            .map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    Ok(output)
}

#[tauri::command]
async fn run_studio_project(
    project_id: String,
    parameters: serde_json::Value,
    state: State<'_, HostState>,
    paths: State<'_, AppPaths>,
    processes: State<'_, RunProcessManager>,
) -> Result<String, String> {
    validate_project_id(&project_id)?;
    let root = paths.workspace_root.join("projects").join(project_id);
    let manifest = PackageManifest::from_yaml(
        &fs::read_to_string(root.join("manifest.yaml"))
            .map_err(|error| format!("无法读取 manifest.yaml：{error}"))?,
    )
    .map_err(|error| error.to_string())?;
    let launch = state
        .prepare_development_run(&root, &manifest, &parameters)
        .map_err(|error| error.to_string())?;
    let run_id = launch.run_id.clone();
    let background_state = state.inner().clone();
    let background_paths = paths.inner().clone();
    let background_processes = processes.inner().clone();
    let background_run_id = run_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = execute_python_run(
            &background_state,
            &background_paths,
            &background_processes,
            &launch,
            &parameters,
        ) && !background_state.run_is_cancelled(&background_run_id)
        {
            background_state.fail_run(&background_run_id, error);
        }
    });
    Ok(run_id)
}

#[tauri::command(async)]
fn open_installed_package(
    package_id: String,
    paths: State<'_, AppPaths>,
) -> Result<StudioProject, String> {
    validate_package_id(&package_id).map_err(|error| error.to_string())?;
    let package_root = paths.workspace_root.join("packages").join(&package_id);
    let mut versions: Vec<PathBuf> = fs::read_dir(&package_root)
        .map_err(|_| format!("找不到已安装 RPAZ 包：{package_id}"))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("manifest.yaml").is_file())
        .collect();
    versions.sort();
    let source = versions
        .pop()
        .ok_or_else(|| format!("RPAZ 包 {package_id} 没有可编辑版本"))?;
    let manifest = PackageManifest::from_yaml(
        &fs::read_to_string(source.join("manifest.yaml")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let (project_id, _) = generated_project_ids(&manifest.name);
    let target = paths.workspace_root.join("projects").join(&project_id);
    copy_directory(&source, &target).map_err(|error| error.to_string())?;
    if !target.join("notebook.ipynb").is_file() {
        fs::write(
            target.join("notebook.ipynb"),
            "{\n  \"cells\": [],\n  \"metadata\": {\"kernelspec\": {\"display_name\": \"DRPA Python 3.11\", \"language\": \"python\", \"name\": \"drpa-python\"}},\n  \"nbformat\": 4,\n  \"nbformat_minor\": 5\n}\n",
        )
        .map_err(|error| error.to_string())?;
    }
    ensure_studio_project_readme(&target, &manifest.name)?;
    let mut files = Vec::new();
    collect_project_entries(&target, &target, &mut files).map_err(|error| error.to_string())?;
    files.sort();
    Ok(StudioProject {
        id: project_id,
        name: manifest.name,
        files,
    })
}

#[tauri::command]
async fn prepare_studio_kernel(
    project_id: String,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<(), String> {
    let paths = paths.inner().clone();
    let kernels = kernels.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        validate_project_id(&project_id)?;
        let mut sessions = kernels
            .sessions
            .lock()
            .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?;
        if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(project_id.clone())
        {
            let kernel = spawn_studio_kernel(&project_id, &paths)?;
            entry.insert(kernel);
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("准备 Studio Kernel 任务失败：{error}"))?
}

#[tauri::command]
async fn execute_studio_cell(
    project_id: String,
    code: String,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<StudioCellResult, String> {
    let paths = paths.inner().clone();
    let kernels = kernels.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        execute_studio_cell_blocking(project_id, code, &paths, &kernels)
    })
    .await
    .map_err(|error| format!("执行 Notebook 单元任务失败：{error}"))?
}

#[tauri::command]
async fn complete_studio_python(
    project_id: String,
    code: String,
    cursor_pos: usize,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<StudioCompletionResult, String> {
    let paths = paths.inner().clone();
    let kernels = kernels.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        complete_studio_python_blocking(project_id, code, cursor_pos, &paths, &kernels)
    })
    .await
    .map_err(|error| format!("执行 Python 补全任务失败：{error}"))?
}

#[tauri::command]
async fn inspect_studio_python(
    project_id: String,
    code: String,
    cursor_pos: usize,
    detail_level: u8,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<StudioInspectResult, String> {
    let paths = paths.inner().clone();
    let kernels = kernels.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inspect_studio_python_blocking(project_id, code, cursor_pos, detail_level, &paths, &kernels)
    })
    .await
    .map_err(|error| format!("执行 Python 符号检查任务失败：{error}"))?
}

fn complete_studio_python_blocking(
    project_id: String,
    code: String,
    cursor_pos: usize,
    paths: &AppPaths,
    kernels: &StudioKernelManager,
) -> Result<StudioCompletionResult, String> {
    validate_project_id(&project_id)?;
    let mut sessions = kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?;
    if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(project_id.clone()) {
        entry.insert(spawn_studio_kernel(&project_id, paths)?);
    }
    let request_id = Uuid::new_v4().simple().to_string();
    let result = (|| {
        let session = sessions
            .get_mut(&project_id)
            .ok_or_else(|| "创建 Studio Kernel 失败".to_owned())?;
        serde_json::to_writer(
            &mut session.stdin,
            &serde_json::json!({
                "type": "complete",
                "request_id": request_id.clone(),
                "code": code,
                "cursor_pos": cursor_pos,
            }),
        )
        .map_err(|error| error.to_string())?;
        session
            .stdin
            .write_all(b"\n")
            .and_then(|_| session.stdin.flush())
            .map_err(|error| format!("向 Kernel 发送补全请求失败：{error}"))?;
        let mut line = String::new();
        if session
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("读取 Kernel 补全响应失败：{error}"))?
            == 0
        {
            return Err("Studio Kernel 已意外退出".to_owned());
        }
        let response: StudioKernelCompletionResponse = serde_json::from_str(&line)
            .map_err(|error| format!("Kernel 返回无效补全响应：{error}"))?;
        if response.request_id != request_id {
            return Err("Kernel 补全响应与当前请求不匹配".to_owned());
        }
        Ok(StudioCompletionResult {
            matches: response.matches,
            cursor_start: response.cursor_start,
            cursor_end: response.cursor_end,
            metadata: response.metadata,
            status: response.status,
        })
    })();
    if result.is_err() {
        sessions.remove(&project_id);
    }
    result
}

fn inspect_studio_python_blocking(
    project_id: String,
    code: String,
    cursor_pos: usize,
    detail_level: u8,
    paths: &AppPaths,
    kernels: &StudioKernelManager,
) -> Result<StudioInspectResult, String> {
    validate_project_id(&project_id)?;
    let mut sessions = kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?;
    if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(project_id.clone()) {
        entry.insert(spawn_studio_kernel(&project_id, paths)?);
    }
    let request_id = Uuid::new_v4().simple().to_string();
    let result = (|| {
        let session = sessions
            .get_mut(&project_id)
            .ok_or_else(|| "创建 Studio Kernel 失败".to_owned())?;
        serde_json::to_writer(
            &mut session.stdin,
            &serde_json::json!({
                "type": "inspect",
                "request_id": request_id.clone(),
                "code": code,
                "cursor_pos": cursor_pos,
                "detail_level": detail_level.min(1),
            }),
        )
        .map_err(|error| error.to_string())?;
        session
            .stdin
            .write_all(b"\n")
            .and_then(|_| session.stdin.flush())
            .map_err(|error| format!("向 Kernel 发送符号检查请求失败：{error}"))?;
        let mut line = String::new();
        if session
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("读取 Kernel 符号检查响应失败：{error}"))?
            == 0
        {
            return Err("Studio Kernel 已意外退出".to_owned());
        }
        let response: StudioKernelInspectResponse = serde_json::from_str(&line)
            .map_err(|error| format!("Kernel 返回无效符号检查响应：{error}"))?;
        if response.request_id != request_id {
            return Err("Kernel 符号检查响应与当前请求不匹配".to_owned());
        }
        Ok(StudioInspectResult {
            found: response.found,
            data: response.data,
            metadata: response.metadata,
            status: response.status,
        })
    })();
    if result.is_err() {
        sessions.remove(&project_id);
    }
    result
}

fn execute_studio_cell_blocking(
    project_id: String,
    code: String,
    paths: &AppPaths,
    kernels: &StudioKernelManager,
) -> Result<StudioCellResult, String> {
    validate_project_id(&project_id)?;
    let mut sessions = kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?;
    if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(project_id.clone()) {
        entry.insert(spawn_studio_kernel(&project_id, paths)?);
    }
    let request_id = Uuid::new_v4().simple().to_string();
    let result = (|| {
        let session = sessions
            .get_mut(&project_id)
            .ok_or_else(|| "无法创建 Studio Kernel".to_owned())?;
        serde_json::to_writer(
            &mut session.stdin,
            &serde_json::json!({
                "type": "execute",
                "request_id": request_id.clone(),
                "code": code,
            }),
        )
        .map_err(|error| error.to_string())?;
        session
            .stdin
            .write_all(b"\n")
            .and_then(|_| session.stdin.flush())
            .map_err(|error| format!("无法向 Kernel 发送代码：{error}"))?;
        let mut line = String::new();
        if session
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("无法读取 Kernel 输出：{error}"))?
            == 0
        {
            return Err("Studio Kernel 已意外退出".to_owned());
        }
        let response: StudioKernelResponse =
            serde_json::from_str(&line).map_err(|error| format!("Kernel 返回无效响应：{error}"))?;
        if response.request_id != request_id {
            return Err("Kernel 响应与当前单元格不匹配".to_owned());
        }
        Ok(StudioCellResult {
            execution_count: response.execution_count,
            stdout: response.stdout,
            stderr: response.stderr,
            result: response.result,
            error: response.error,
            traceback: response.traceback,
            outputs: response.outputs,
            variables: response.variables,
            duration_ms: response.duration_ms,
        })
    })();
    if result.is_err() {
        sessions.remove(&project_id);
    }
    result
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn run_agent_turn(
    request: agent::AgentTurnRequest,
    app: tauri::AppHandle,
    paths: State<'_, AppPaths>,
    state: State<'_, HostState>,
    processes: State<'_, RunProcessManager>,
    vault: State<'_, credential_vault::CredentialVaultManager>,
    runs: State<'_, agent_runtime::AgentRunManager>,
    browsers: State<'_, agent_browser::AgentBrowserManager>,
) -> Result<agent::AgentTurnResult, String> {
    let event_name = agent::agent_stream_event_name(&request.request_id)?;
    let request_id = request.request_id.clone();
    let session_id = request.session_id.clone();
    let max_wall_time =
        std::time::Duration::from_secs(request.max_wall_time_seconds.clamp(10, 86_400));
    let run_manager = runs.inner().clone();
    let browser_manager = browsers.inner().clone();
    let control = run_manager.begin(&request_id, &session_id, max_wall_time)?;
    let started_event = agent::AgentStreamEvent::Started {
        run_id: request_id.clone(),
        session_id,
    };
    run_manager.record(&request_id, started_event.clone());
    let _ = app.emit(&event_name, started_event);
    let paths = paths.inner().clone();
    let host = agent::AgentHostContext::new(
        state.inner().clone(),
        paths.clone(),
        processes.inner().clone(),
        vault.inner().clone(),
    );
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let runtime = if request.mode == "sql" {
                None
            } else {
                Some(locate_runtime(&paths)?)
            };
            let python = runtime
                .as_ref()
                .map(|runtime| runtime.python.clone())
                .unwrap_or_default();
            let browser = runtime.and_then(|runtime| runtime.browser);
            let browser_session_id = if request.session_id.trim().is_empty() {
                request_id.as_str()
            } else {
                request.session_id.trim()
            };
            let browser_session = if request.mode == "sql" {
                None
            } else {
                Some(browser_manager.session(browser_session_id)?)
            };
            agent::run_agent_turn(
                request,
                paths.workspace_root.clone(),
                paths.resource_dir.clone(),
                python,
                browser,
                browser_session,
                host,
                control.clone(),
                |event| {
                    run_manager.record(&request_id, event.clone());
                    let _ = app.emit(&event_name, event);
                },
            )
        })();
        match &result {
            Ok(completed) => {
                run_manager.complete(&control, completed);
                let terminal = agent::AgentStreamEvent::Completed {
                    run_id: request_id.clone(),
                    usage: completed.usage.clone(),
                    duration_ms: completed.duration_ms,
                    stop_reason: completed.stop_reason.clone(),
                };
                run_manager.record(&request_id, terminal.clone());
                let _ = app.emit(&event_name, terminal);
            }
            Err(error) => {
                run_manager.fail(&control, error);
                let terminal = if control.is_cancelled() || error.contains("运行已取消") {
                    agent::AgentStreamEvent::Cancelled {
                        run_id: request_id.clone(),
                    }
                } else {
                    agent::AgentStreamEvent::Failed {
                        run_id: request_id.clone(),
                        error: error.clone(),
                    }
                };
                run_manager.record(&request_id, terminal.clone());
                let _ = app.emit(&event_name, terminal);
            }
        }
        result
    })
    .await
    .map_err(|error| format!("Agent 后台任务失败：{error}"))?
}

#[tauri::command]
fn cancel_agent_run(
    request_id: String,
    runs: State<'_, agent_runtime::AgentRunManager>,
) -> Result<agent_runtime::AgentRunSnapshot, String> {
    agent::agent_stream_event_name(&request_id)?;
    runs.cancel(&request_id)
}

#[tauri::command]
fn get_agent_run(
    request_id: String,
    runs: State<'_, agent_runtime::AgentRunManager>,
) -> Result<agent_runtime::AgentRunSnapshot, String> {
    agent::agent_stream_event_name(&request_id)?;
    runs.snapshot(&request_id)
}

#[tauri::command(async)]
fn list_agent_extensions(
    paths: State<'_, AppPaths>,
) -> Result<Vec<agent_extensions::AgentExtensionSummary>, String> {
    agent_extensions::list_extensions(&paths.workspace_root)
}

#[tauri::command(async)]
fn install_agent_extension(
    package_path: String,
    paths: State<'_, AppPaths>,
) -> Result<agent_extensions::AgentExtensionSummary, String> {
    agent_extensions::install_extension(&paths.workspace_root, &package_path)
}

#[tauri::command(async)]
fn set_agent_extension_enabled(
    extension_id: String,
    enabled: bool,
    paths: State<'_, AppPaths>,
) -> Result<agent_extensions::AgentExtensionSummary, String> {
    agent_extensions::set_extension_enabled(&paths.workspace_root, &extension_id, enabled)
}

#[tauri::command(async)]
fn remove_agent_extension(extension_id: String, paths: State<'_, AppPaths>) -> Result<(), String> {
    agent_extensions::remove_extension(&paths.workspace_root, &extension_id)
}

#[tauri::command]
async fn start_plugin(
    plugin_id: String,
    paths: State<'_, AppPaths>,
    manager: State<'_, plugins::PluginManager>,
) -> Result<(), String> {
    let workspace_root = paths.workspace_root.clone();
    let python = if plugins::plugin_services_require_bundled_python(&workspace_root, &plugin_id)? {
        Some(locate_runtime(&paths)?.python)
    } else {
        None
    };
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        plugins::start_plugin_inner(&workspace_root, &plugin_id, python.as_deref(), &manager)
            .map_err(|error| plugins::redact_plugin_error(&workspace_root, &plugin_id, error))
    })
    .await
    .map_err(|error| format!("插件后台启动任务失败：{error}"))?
}

#[tauri::command]
async fn list_plugin_tools(
    plugin_id: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<plugins::PluginToolDescriptor>, String> {
    let workspace_root = paths.workspace_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        plugins::list_plugin_tools_for_workbench(&workspace_root, &plugin_id)
    })
    .await
    .map_err(|error| format!("插件工具扫描后台任务失败：{error}"))?
}

#[tauri::command]
async fn invoke_plugin_tool(
    plugin_id: String,
    tool_name: String,
    input: serde_json::Value,
    paths: State<'_, AppPaths>,
) -> Result<plugins::PluginToolWorkbenchResult, String> {
    let paths = paths.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let python = locate_runtime(&paths)?.python;
        plugins::invoke_plugin_tool_for_workbench(
            &paths.workspace_root,
            &python,
            &plugin_id,
            &tool_name,
            &input,
        )
    })
    .await
    .map_err(|error| format!("插件工具后台任务失败：{error}"))?
}

#[tauri::command(async)]
fn restart_studio_kernel(
    project_id: String,
    kernels: State<'_, StudioKernelManager>,
) -> Result<(), String> {
    validate_project_id(&project_id)?;
    kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?
        .remove(&project_id);
    Ok(())
}

#[tauri::command]
fn get_data_directory(paths: State<'_, AppPaths>) -> String {
    paths.workspace_root.to_string_lossy().into_owned()
}

#[tauri::command(async)]
fn open_workspace_data_directory(paths: State<'_, AppPaths>) -> Result<(), String> {
    fs::create_dir_all(&paths.workspace_root).map_err(|error| error.to_string())?;
    open_directory_in_file_explorer(&paths.workspace_root)
}

const USER_DATA_DIRECTORIES: &[&str] = &[
    "agent",
    "automations",
    "build",
    "databases",
    "knowledge",
    "knowledge-bases",
    "local-dify",
    "packages",
    "plugin-projects",
    "plugins",
    "projects",
    "runs",
    "system",
];

#[tauri::command(async)]
fn export_user_data(
    target_path: String,
    paths: State<'_, AppPaths>,
) -> Result<UserDataTransferResult, String> {
    let target = PathBuf::from(target_path);
    if !target.is_absolute() {
        return Err("导出文件必须使用绝对路径".to_owned());
    }
    let parent = target.parent().ok_or_else(|| "导出路径无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{error}"))?;
    let canonical_workspace = fs::canonicalize(&paths.workspace_root)
        .map_err(|error| format!("检查当前工作区失败：{error}"))?;
    let canonical_parent =
        fs::canonicalize(parent).map_err(|error| format!("检查导出目录失败：{error}"))?;
    if canonical_parent.starts_with(&canonical_workspace) {
        return Err("导出文件不能保存在当前工作区内部，请选择其他目录".to_owned());
    }

    let export_stage =
        std::env::temp_dir().join(format!("drpa-export-{}", Uuid::new_v4().simple()));
    fs::create_dir(&export_stage).map_err(|error| format!("创建导出暂存目录失败：{error}"))?;
    let _export_stage_cleanup = TemporaryDirectoryCleanup(export_stage.clone());
    let session_snapshot = export_stage.join("session.db");
    let has_session_snapshot =
        agent_sessions::create_export_snapshot(&paths.workspace_root, &session_snapshot)?;
    let temporary = export_stage.join("user-data.tmp");
    let file = File::create(&temporary).map_err(|error| format!("创建导出文件失败：{error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let workspace_name = workspaces::active_workspace_name(&paths.data_root)
        .unwrap_or_else(|_| "当前工作区".to_owned());
    let metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "kind": "drpa-user-data",
        "workspaceName": workspace_name,
        "exportedAt": chrono::Utc::now().timestamp_millis(),
    }))
    .map_err(|error| error.to_string())?;
    archive
        .start_file("user-data.json", options)
        .map_err(|error| format!("写入导出清单失败：{error}"))?;
    archive
        .write_all(&metadata)
        .map_err(|error| format!("写入导出清单失败：{error}"))?;

    let mut file_count = 0usize;
    let mut total_bytes = 0u64;
    for directory in USER_DATA_DIRECTORIES {
        let root = paths.workspace_root.join(directory);
        if !root.is_dir() {
            continue;
        }
        if *directory == "agent" && has_session_snapshot {
            let size = session_snapshot
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            archive
                .start_file("agent/session.db", options)
                .map_err(|error| format!("写入 Agent 会话快照失败：{error}"))?;
            let mut input = File::open(&session_snapshot)
                .map_err(|error| format!("读取 Agent 会话快照失败：{error}"))?;
            std::io::copy(&mut input, &mut archive)
                .map_err(|error| format!("压缩 Agent 会话快照失败：{error}"))?;
            file_count += 1;
            total_bytes = total_bytes.saturating_add(size);
        }
        let mut files = Vec::new();
        collect_files(&root, &root, &mut files)
            .map_err(|error| format!("扫描 {directory} 失败：{error}"))?;
        files.sort();
        for relative in files {
            let normalized_relative = relative.replace('\\', "/");
            if *directory == "agent"
                && matches!(
                    normalized_relative.as_str(),
                    "session.db" | "session.db-wal" | "session.db-shm"
                )
            {
                continue;
            }
            let source = root.join(&relative);
            let size = source
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            let archive_path = format!("{directory}/{normalized_relative}");
            archive
                .start_file(archive_path, options)
                .map_err(|error| format!("写入用户数据失败：{error}"))?;
            let mut input = File::open(&source)
                .map_err(|error| format!("读取 {} 失败：{error}", source.display()))?;
            std::io::copy(&mut input, &mut archive)
                .map_err(|error| format!("压缩 {} 失败：{error}", source.display()))?;
            file_count += 1;
            total_bytes = total_bytes.saturating_add(size);
        }
    }
    archive
        .finish()
        .map_err(|error| format!("完成用户数据导出失败：{error}"))?;
    let target_staging = parent.join(format!(".drpa-export-{}.tmp", Uuid::new_v4().simple()));
    let _target_staging_cleanup = TemporaryFileCleanup(target_staging.clone());
    fs::copy(&temporary, &target_staging)
        .map_err(|error| format!("提交用户数据导出失败：{error}"))?;
    if target.exists() {
        fs::remove_file(&target).map_err(|error| format!("替换已有导出文件失败：{error}"))?;
    }
    fs::rename(&target_staging, &target)
        .map_err(|error| format!("提交用户数据导出失败：{error}"))?;
    Ok(UserDataTransferResult {
        path: target.to_string_lossy().into_owned(),
        file_count,
        total_bytes,
        workspace_name,
        restart_required: false,
    })
}

#[tauri::command(async)]
fn import_user_data(
    source_path: String,
    app: tauri::AppHandle,
    paths: State<'_, AppPaths>,
    host: State<'_, HostState>,
) -> Result<UserDataTransferResult, String> {
    if host.snapshot().stats.active_runs > 0 {
        return Err("存在正在运行的任务，请等待任务结束后再导入用户数据".to_owned());
    }
    let source = PathBuf::from(source_path);
    if !source.is_absolute() || !source.is_file() {
        return Err("请选择有效的 DRPA 用户数据文件".to_owned());
    }
    let file = File::open(&source).map_err(|error| format!("打开用户数据文件失败：{error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("用户数据文件不是有效 ZIP：{error}"))?;
    if archive.len() > 100_000 {
        return Err("用户数据文件条目过多".to_owned());
    }
    let stage = paths
        .data_root
        .join(".drpa")
        .join(format!("import-stage-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&stage).map_err(|error| format!("创建导入暂存目录失败：{error}"))?;
    let result = (|| {
        let mut workspace_name = "导入的用户数据".to_owned();
        let mut file_count = 0usize;
        let mut total_bytes = 0u64;
        let mut manifest_found = false;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
            let enclosed = entry
                .enclosed_name()
                .ok_or_else(|| format!("用户数据包含不安全路径：{}", entry.name()))?
                .to_owned();
            let first = enclosed
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str())
                .unwrap_or_default();
            if first == "user-data.json" {
                let mut metadata = String::new();
                entry
                    .read_to_string(&mut metadata)
                    .map_err(|error| format!("读取导入清单失败：{error}"))?;
                let value: serde_json::Value = serde_json::from_str(&metadata)
                    .map_err(|error| format!("导入清单无效：{error}"))?;
                if value.get("kind").and_then(serde_json::Value::as_str) != Some("drpa-user-data") {
                    return Err("该文件不是 DRPA 用户数据导出包".to_owned());
                }
                manifest_found = true;
                if let Some(name) = value
                    .get("workspaceName")
                    .and_then(serde_json::Value::as_str)
                {
                    workspace_name =
                        format!("导入 · {}", name.chars().take(48).collect::<String>());
                }
                continue;
            }
            if !USER_DATA_DIRECTORIES.contains(&first) {
                return Err(format!("用户数据包含不支持的目录：{first}"));
            }
            if entry.is_dir() {
                fs::create_dir_all(stage.join(&enclosed))
                    .map_err(|error| format!("创建导入目录失败：{error}"))?;
                continue;
            }
            total_bytes = total_bytes.saturating_add(entry.size());
            if total_bytes > 4 * 1024 * 1024 * 1024 {
                return Err("用户数据解压后超过 4 GiB 限制".to_owned());
            }
            let target = stage.join(&enclosed);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| format!("创建导入目录失败：{error}"))?;
            }
            let mut output =
                File::create(&target).map_err(|error| format!("创建导入文件失败：{error}"))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("解压用户数据失败：{error}"))?;
            file_count += 1;
        }
        if !manifest_found {
            return Err("用户数据文件缺少 DRPA 导出清单".to_owned());
        }
        let imported = workspaces::create_import_workspace(&paths.data_root, &workspace_name)?;
        let target = PathBuf::from(&imported.path);
        copy_directory(&stage, &target).map_err(|error| format!("提交导入工作区失败：{error}"))?;
        workspaces::activate_workspace(&paths.data_root, &imported.id)?;
        knowledge::seed_default_knowledge(&target)
            .map_err(|error| format!("初始化导入知识库失败：{error}"))?;
        Ok(UserDataTransferResult {
            path: target.to_string_lossy().into_owned(),
            file_count,
            total_bytes,
            workspace_name: imported.name,
            restart_required: true,
        })
    })();
    let _ = fs::remove_dir_all(&stage);
    if result.is_ok() {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            app.request_restart();
        });
    }
    result
}

#[tauri::command(async)]
fn open_build_output_directory(paths: State<'_, AppPaths>) -> Result<(), String> {
    let build_root = paths.workspace_root.join("build");
    fs::create_dir_all(&build_root).map_err(|error| error.to_string())?;
    open_directory_in_file_explorer(&build_root)
}

fn open_runtime_output_directory(launch: &RunLaunch, requested: &str) -> Result<(), String> {
    let output = launch
        .output_dir
        .canonicalize()
        .map_err(|error| format!("定位运行输出目录失败：{error}"))?;
    let requested = Path::new(requested)
        .canonicalize()
        .map_err(|error| format!("定位请求目录失败：{error}"))?;
    if requested != output {
        return Err("运行时只允许打开当前任务的输出目录".to_owned());
    }
    open_directory_in_file_explorer(&output)
}

fn open_directory_in_file_explorer(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("目录不存在：{}", path.display()));
    }
    #[cfg(windows)]
    let mut command = Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_child_window(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开目录失败：{error}"))
}

fn generated_project_ids(name: &str) -> (String, String) {
    let salt = Uuid::new_v4();
    let mut first = DefaultHasher::new();
    name.hash(&mut first);
    salt.as_bytes().hash(&mut first);
    let first = first.finish();

    let mut second = DefaultHasher::new();
    salt.as_bytes().hash(&mut second);
    first.hash(&mut second);
    name.len().hash(&mut second);
    let hash = format!("{first:016x}{:08x}", second.finish() as u32);
    (format!("project-{hash}"), format!("local.{hash}"))
}

fn validate_project_id(project_id: &str) -> Result<(), String> {
    let is_generated = project_id
        .strip_prefix("project-")
        .is_some_and(|hash| hash.len() == 24 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    // Preview 4 and earlier used the package id as the Studio directory id.
    // Continue accepting those safe, separator-free ids so existing projects open.
    if is_generated || validate_package_id(project_id).is_ok() {
        Ok(())
    } else {
        Err("无效的内部项目标识".to_owned())
    }
}

fn copy_directory(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)?.flatten() {
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &target_path)?;
        } else if source_path.is_file() {
            fs::copy(source_path, target_path)?;
        }
    }
    Ok(())
}

fn collect_files(root: &Path, current: &Path, output: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(current)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "__pycache__") {
                continue;
            }
            collect_files(root, &path, output)?;
        } else if path.is_file() {
            if path
                .extension()
                .is_some_and(|extension| extension == "pyc" || extension == "pyo")
            {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(root) {
                output.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

struct RuntimeEnvironment {
    python: PathBuf,
    python_path: Option<PathBuf>,
    browser: Option<PathBuf>,
    rpa_bundle: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfflineRuntimeManifest {
    bundle_version: String,
    platform: String,
    python_version: String,
    python_executable: String,
    browser_executable: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeStatus {
    state: &'static str,
    bundle_version: String,
    python_version: String,
    runtime_root: String,
    environment_root: String,
    browser_executable: String,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformCapabilities {
    os: &'static str,
    display_name: &'static str,
    runtime_target: &'static str,
    supports_windows_updates: bool,
    file_manager_name: &'static str,
    data_directory_policy: &'static str,
    reduced_visual_effects: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsUpdateManifest {
    schema: u32,
    host_protocol: u32,
    worker_protocol: u32,
    package_kind: String,
    minimum_host_version: String,
    version: String,
    base_version: Option<String>,
    target: String,
    files: Vec<WindowsUpdateFile>,
    #[serde(default)]
    remove: Vec<String>,
    worker: WindowsUpdateWorker,
}

#[derive(Deserialize)]
struct WindowsUpdateFile {
    path: String,
    bytes: u64,
}

#[derive(Deserialize)]
struct WindowsUpdateWorker {
    bytes: u64,
}

#[derive(Deserialize)]
struct WindowsInstallCatalog {
    schema: u32,
    version: String,
    target: String,
    update_protocol: u32,
}

#[tauri::command(async)]
fn apply_windows_update(
    package_path: String,
    host: State<'_, HostState>,
    kernels: State<'_, StudioKernelManager>,
    paths: State<'_, AppPaths>,
) -> Result<WindowsUpdateSession, String> {
    if !cfg!(windows) {
        return Err("文件热更新当前只支持 Windows 平台".to_owned());
    }
    if host.snapshot().stats.active_runs > 0 {
        return Err("存在正在运行的任务，请等待任务结束后再更新".to_owned());
    }
    let package = Path::new(&package_path);
    let file = File::open(package).map_err(|error| format!("打开更新包失败：{error}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("更新包无效：{error}"))?;
    let manifest_source = {
        let mut entry = archive
            .by_name("update-manifest.json")
            .map_err(|_| "更新包缺少 update-manifest.json".to_owned())?;
        let mut source = String::new();
        entry
            .read_to_string(&mut source)
            .map_err(|error| format!("读取更新清单失败：{error}"))?;
        source
    };
    let manifest: WindowsUpdateManifest =
        serde_json::from_str(&manifest_source).map_err(|error| format!("更新清单无效：{error}"))?;
    if manifest.schema != WINDOWS_UPDATE_SCHEMA {
        return Err(format!(
            "更新协议不兼容：当前支持 schema {}，更新包为 schema {}。请先安装对应版本的全量安装包",
            WINDOWS_UPDATE_SCHEMA, manifest.schema
        ));
    }
    if manifest.host_protocol > WINDOWS_UPDATE_HOST_PROTOCOL
        || manifest.worker_protocol != WINDOWS_UPDATE_WORKER_PROTOCOL
    {
        return Err(format!(
            "更新协议版本不兼容：Host {}/{}，Worker {}/{}。请先安装对应版本的全量安装包",
            WINDOWS_UPDATE_HOST_PROTOCOL,
            manifest.host_protocol,
            WINDOWS_UPDATE_WORKER_PROTOCOL,
            manifest.worker_protocol
        ));
    }
    if manifest.target != "windows-x86_64" {
        return Err("更新包目标平台不是 windows-x86_64".to_owned());
    }
    if !version_is_at_least(env!("CARGO_PKG_VERSION"), &manifest.minimum_host_version) {
        return Err(format!(
            "当前客户端版本 {} 低于更新包要求的 {}，请先安装全量安装包",
            env!("CARGO_PKG_VERSION"),
            manifest.minimum_host_version
        ));
    }
    if manifest.package_kind != "delta" {
        return Err("客户端只接受 delta 文件更新；完整版本请运行全量安装包".to_owned());
    }
    if !is_safe_update_version(&manifest.version) {
        return Err("更新包版本标识无效".to_owned());
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let install = executable
        .parent()
        .ok_or_else(|| "定位安装目录失败".to_owned())?;
    let base_version = manifest
        .base_version
        .as_ref()
        .ok_or_else(|| "delta 更新包缺少 baseVersion".to_owned())?;
    if !is_safe_update_version(base_version) {
        return Err("更新包基线版本标识无效".to_owned());
    }
    let catalog_source =
        fs::read_to_string(install.join("install-manifest.json")).map_err(|_| {
            "当前安装不是受支持的更新基线（缺少 install-manifest.json），请安装 0.3.0 全量安装包"
                .to_owned()
        })?;
    let catalog: WindowsInstallCatalog = serde_json::from_str(&catalog_source)
        .map_err(|error| format!("当前安装文件清单无效：{error}"))?;
    if catalog.schema != WINDOWS_UPDATE_SCHEMA
        || catalog.update_protocol != WINDOWS_UPDATE_HOST_PROTOCOL
        || catalog.target != "windows-x86_64"
    {
        return Err("当前安装清单属于旧更新协议，请安装 0.3.0 全量安装包".to_owned());
    }
    if catalog.version != *base_version {
        return Err(format!(
            "更新基线不匹配：当前为 {}，更新包要求 {}",
            catalog.version, base_version
        ));
    }
    if manifest.files.is_empty()
        || manifest.files.len().saturating_add(manifest.remove.len()) > 50_000
    {
        return Err("更新包文件数量异常".to_owned());
    }

    let session_id = Uuid::new_v4().simple().to_string();
    let session_root = paths.data_root.join("updates/sessions").join(&session_id);
    let stage = session_root.join("stage");
    let status_path = session_root.join("status.json");
    let restart_request = session_root.join("restart-requested");
    fs::create_dir_all(stage.join("files"))
        .map_err(|error| format!("创建更新暂存目录失败：{error}"))?;
    let total_bytes = manifest.files.iter().try_fold(0_u64, |total, item| {
        total.checked_add(item.bytes).ok_or("更新包大小溢出")
    })?;
    let total_files = u32::try_from(manifest.files.len() + manifest.remove.len())
        .map_err(|_| "更新包文件数量溢出".to_owned())?;
    write_windows_update_status(
        &status_path,
        &WindowsUpdateStatus {
            session_id: session_id.clone(),
            version: manifest.version.clone(),
            phase: WindowsUpdatePhase::Verifying,
            progress: 1,
            completed_files: 0,
            total_files,
            current_file: None,
            message: "正在读取更新包清单与文件结构".to_owned(),
        },
    )?;

    let extraction = (|| {
        let mut total = 0_u64;
        let mut package_paths = HashSet::new();
        for item in &manifest.files {
            let relative = safe_relative_path(&item.path)
                .map_err(|error| format!("更新清单包含不安全路径：{error}"))?;
            let normalized = relative.to_string_lossy().replace('\\', "/");
            let folded = normalized.to_ascii_lowercase();
            if !package_paths.insert(folded.clone()) {
                return Err(format!("更新清单包含重复路径：{normalized}"));
            }
            if folded.starts_with("data/") || folded.starts_with("webview2/") {
                return Err(format!("更新包试图覆盖受保护路径：{normalized}"));
            }
            total = total
                .checked_add(item.bytes)
                .ok_or_else(|| "更新包过大".to_owned())?;
            if total > 2 * 1024 * 1024 * 1024 {
                return Err("更新包解压后超过 2 GiB 限制".to_owned());
            }
            let archive_name = format!("files/{normalized}");
            let mut entry = archive
                .by_name(&archive_name)
                .map_err(|_| format!("更新包缺少文件：{normalized}"))?;
            if entry.is_dir() || entry.size() != item.bytes {
                return Err(format!("更新文件大小不匹配：{normalized}"));
            }
            if entry.compressed_size() > 0 && entry.size() / entry.compressed_size().max(1) > 200 {
                return Err(format!("更新文件压缩率异常：{normalized}"));
            }
            let target = stage.join("files").join(&relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut output = File::create(&target).map_err(|error| error.to_string())?;
            let written =
                std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
            if written != item.bytes {
                return Err(format!("更新文件写入大小不匹配：{normalized}"));
            }
        }
        let mut remove_paths = HashSet::new();
        for item in &manifest.remove {
            let relative = safe_relative_path(item)
                .map_err(|error| format!("删除清单包含不安全路径：{error}"))?;
            let normalized = relative.to_string_lossy().replace('\\', "/");
            let folded = normalized.to_ascii_lowercase();
            if !remove_paths.insert(folded.clone()) || package_paths.contains(&folded) {
                return Err(format!("删除清单包含重复或冲突路径：{normalized}"));
            }
            if folded.starts_with("data/") || folded.starts_with("webview2/") {
                return Err(format!("删除清单包含受保护路径：{normalized}"));
            }
        }
        fs::write(stage.join("update-manifest.json"), &manifest_source)
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = extraction {
        let _ = fs::remove_dir_all(&session_root);
        return Err(error);
    }

    let updater = session_root.join("update-worker.exe");
    let worker = &manifest.worker;
    let mut entry = archive
        .by_name("worker/drpa-updater.exe")
        .map_err(|_| "更新包缺少 worker/drpa-updater.exe".to_owned())?;
    if entry.is_dir() || entry.size() != worker.bytes {
        return Err("更新 Worker 大小不匹配".to_owned());
    }
    let mut output = File::create(&updater).map_err(|error| error.to_string())?;
    let written = std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
    if written != worker.bytes {
        return Err("更新 Worker 写入大小不匹配".to_owned());
    }
    kernels
        .sessions
        .lock()
        .map_err(|_| "停止 Studio Kernel 失败".to_owned())?
        .clear();
    let launch = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "读取主程序文件名失败".to_owned())?;
    if let Err(error) = spawn_windows_update_worker(
        &updater,
        &stage,
        install,
        launch,
        &status_path,
        &restart_request,
        &session_id,
    ) {
        let message = format!("启动无界面更新 Worker 失败：{error}");
        let _ = write_windows_update_status(
            &status_path,
            &WindowsUpdateStatus {
                session_id: session_id.clone(),
                version: manifest.version.clone(),
                phase: WindowsUpdatePhase::Failed,
                progress: 0,
                completed_files: 0,
                total_files,
                current_file: None,
                message: message.clone(),
            },
        );
        return Err(message);
    }
    Ok(WindowsUpdateSession {
        id: session_id,
        version: manifest.version,
        total_files,
        total_bytes,
    })
}

#[tauri::command(async)]
fn get_windows_update_status(
    session_id: String,
    paths: State<'_, AppPaths>,
) -> Result<WindowsUpdateStatus, String> {
    validate_update_session_id(&session_id)?;
    let source = fs::read_to_string(
        paths
            .data_root
            .join("updates/sessions")
            .join(&session_id)
            .join("status.json"),
    )
    .map_err(|error| format!("读取更新进度失败：{error}"))?;
    serde_json::from_str(&source).map_err(|error| format!("更新进度数据无效：{error}"))
}

#[tauri::command(async)]
fn get_latest_windows_update_status(
    paths: State<'_, AppPaths>,
) -> Result<Option<WindowsUpdateStatus>, String> {
    let sessions = paths.data_root.join("updates/sessions");
    if !sessions.is_dir() {
        return Ok(None);
    }
    let mut latest = None;
    for entry in fs::read_dir(&sessions)
        .map_err(|error| format!("读取更新会话目录失败：{error}"))?
        .flatten()
    {
        let status_path = entry.path().join("status.json");
        let Ok(metadata) = fs::metadata(&status_path) else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if latest
            .as_ref()
            .is_none_or(|(latest_modified, _)| modified > *latest_modified)
        {
            latest = Some((modified, status_path));
        }
    }
    let Some((_, status_path)) = latest else {
        return Ok(None);
    };
    let source = fs::read_to_string(status_path)
        .map_err(|error| format!("读取最近更新状态失败：{error}"))?;
    serde_json::from_str(&source)
        .map(Some)
        .map_err(|error| format!("最近更新状态无效：{error}"))
}

#[tauri::command(async)]
fn restart_for_windows_update(
    session_id: String,
    app: tauri::AppHandle,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_update_session_id(&session_id)?;
    let session_root = paths.data_root.join("updates/sessions").join(&session_id);
    let source = fs::read_to_string(session_root.join("status.json"))
        .map_err(|error| format!("读取更新进度失败：{error}"))?;
    let status: WindowsUpdateStatus =
        serde_json::from_str(&source).map_err(|error| format!("更新进度数据无效：{error}"))?;
    if status.phase != WindowsUpdatePhase::WaitingForRestart {
        return Err("更新尚未进入重启阶段".to_owned());
    }
    if !session_root.join("worker-ready").is_file() {
        return Err("更新 Worker 尚未就绪，应用保持运行".to_owned());
    }
    fs::write(session_root.join("restart-requested"), b"restart\n")
        .map_err(|error| format!("创建重启请求失败：{error}"))?;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(350));
        app.exit(0);
    });
    Ok(())
}

fn validate_update_session_id(session_id: &str) -> Result<(), String> {
    if session_id.len() == 32 && session_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("更新会话标识无效".to_owned())
    }
}

fn update_session_root(data_root: &Path, session_id: &str) -> Result<PathBuf, String> {
    validate_update_session_id(session_id)?;
    Ok(data_root.join("updates/sessions").join(session_id))
}

fn acknowledge_windows_update_startup(data_root: &Path) -> std::io::Result<()> {
    let Some(session_id) = std::env::var_os("DRPA_UPDATE_SESSION_ID") else {
        return Ok(());
    };
    let session_id = session_id.to_string_lossy();
    let Ok(session_root) = update_session_root(data_root, &session_id) else {
        return Ok(());
    };
    if !session_root.join("status.json").is_file() {
        return Ok(());
    }
    let target = session_root.join("startup-ack");
    let temporary = session_root.join("startup-ack.tmp");
    fs::write(&temporary, format!("pid={}\n", std::process::id()))?;
    if target.exists() {
        fs::remove_file(&target)?;
    }
    fs::rename(temporary, target)
}

fn write_windows_update_status(target: &Path, status: &WindowsUpdateStatus) -> Result<(), String> {
    let source = serde_json::to_vec_pretty(status).map_err(|error| error.to_string())?;
    let temporary = target.with_extension("json.tmp");
    fs::write(&temporary, source).map_err(|error| format!("写入更新进度失败：{error}"))?;
    if target.exists() {
        fs::remove_file(target).map_err(|error| format!("替换更新进度失败：{error}"))?;
    }
    fs::rename(temporary, target).map_err(|error| format!("提交更新进度失败：{error}"))
}

fn is_safe_update_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 80
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn numeric_version(value: &str) -> Option<[u64; 3]> {
    let core = value.split_once('-').map_or(value, |(core, _)| core);
    let mut parts = core.split('.');
    let version = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    if parts.next().is_some() {
        return None;
    }
    Some(version)
}

fn version_is_at_least(current: &str, minimum: &str) -> bool {
    numeric_version(current)
        .zip(numeric_version(minimum))
        .is_some_and(|(current, minimum)| current >= minimum)
}

#[tauri::command(async)]
fn get_runtime_status(paths: State<'_, AppPaths>) -> Result<RuntimeStatus, String> {
    inspect_runtime_status(&paths)
}

#[tauri::command]
fn get_platform_capabilities() -> PlatformCapabilities {
    let reduced_visual_effects = std::env::var("DRPA_UI_REDUCED_EFFECTS")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false);
    if cfg!(windows) {
        PlatformCapabilities {
            os: "windows",
            display_name: "Windows x64",
            runtime_target: "windows-x86_64",
            supports_windows_updates: false,
            file_manager_name: "资源管理器",
            data_directory_policy: "安装目录 data",
            reduced_visual_effects,
        }
    } else if cfg!(target_os = "linux") {
        PlatformCapabilities {
            os: "linux",
            display_name: "Linux x86_64",
            runtime_target: "linux-x86_64",
            supports_windows_updates: false,
            file_manager_name: "文件管理器",
            data_directory_policy: "XDG 本地数据目录",
            reduced_visual_effects,
        }
    } else {
        PlatformCapabilities {
            os: "macos",
            display_name: "macOS",
            runtime_target: if cfg!(target_arch = "aarch64") {
                "macos-arm64"
            } else {
                "macos-x86_64"
            },
            supports_windows_updates: false,
            file_manager_name: "Finder",
            data_directory_policy: "应用本地数据目录",
            reduced_visual_effects,
        }
    }
}

#[tauri::command(async)]
fn initialize_runtime(paths: State<'_, AppPaths>) -> Result<RuntimeStatus, String> {
    let runtime = locate_runtime(&paths)?;
    verify_runtime_imports(&runtime)?;
    inspect_runtime_status(&paths)
}

#[tauri::command(async)]
fn repair_runtime(
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<RuntimeStatus, String> {
    kernels
        .sessions
        .lock()
        .map_err(|_| "无法停止 Studio Kernel".to_owned())?
        .clear();
    let generated = paths.data_root.join("runtime-environment");
    if generated.is_dir() {
        fs::remove_dir_all(&generated)
            .map_err(|error| format!("无法清理损坏的运行环境：{error}"))?;
    }
    let runtime = locate_runtime(&paths)?;
    verify_runtime_imports(&runtime)?;
    inspect_runtime_status(&paths)
}

fn spawn_studio_kernel(project_id: &str, paths: &AppPaths) -> Result<StudioKernel, String> {
    let RuntimeEnvironment {
        python,
        python_path,
        browser,
        rpa_bundle,
    } = locate_runtime(paths)?;
    let project_root = paths.workspace_root.join("projects").join(project_id);
    fs::create_dir_all(paths.workspace_root.join("rpa-python"))
        .map_err(|error| format!("准备 RPA for Python 工作目录失败：{error}"))?;
    let mut command = Command::new(python);
    command
        .args(["-m", "drpa_runner.kernel"])
        .current_dir(project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env(
            "DRPA_BROWSER_PROFILE_ROOT",
            paths.workspace_root.join("browser").join("drissionpage"),
        )
        .env("DRPA_RPA_HOME", paths.workspace_root.join("rpa-python"));
    if let Some(python_path) = python_path {
        command.env("PYTHONPATH", python_path);
    }
    if let Some(browser) = browser {
        command.env("DRPA_BROWSER_PATH", browser);
    }
    if let Some(rpa_bundle) = rpa_bundle {
        command.env("DRPA_RPA_BUNDLE", rpa_bundle);
    }
    configure_linux_process_group(&mut command);
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动 Studio Kernel：{error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法连接 Studio Kernel 输入".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法连接 Studio Kernel 输出".to_owned())?;
    Ok(StudioKernel {
        child,
        stdin: BufWriter::new(stdin),
        stdout: BufReader::new(stdout),
    })
}

fn execute_python_run(
    state: &HostState,
    paths: &AppPaths,
    processes: &RunProcessManager,
    launch: &RunLaunch,
    parameters: &serde_json::Value,
) -> Result<(), String> {
    if state.run_is_cancelled(&launch.run_id) {
        return Ok(());
    }
    let runtime = locate_runtime(paths)?;
    fs::create_dir_all(&launch.output_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(paths.workspace_root.join("rpa-python"))
        .map_err(|error| format!("准备 RPA for Python 工作目录失败：{error}"))?;
    let run_root = launch
        .output_dir
        .parent()
        .ok_or_else(|| "无效的运行输出目录".to_owned())?;
    fs::create_dir_all(run_root).map_err(|error| error.to_string())?;
    let request_path = run_root.join("request.json");
    let database_path = paths
        .workspace_root
        .join("databases")
        .join("workspace.sqlite3");
    if let Some(database_root) = database_path.parent() {
        fs::create_dir_all(database_root).map_err(|error| error.to_string())?;
    }
    let request = serde_json::json!({
        "protocol": RUNTIME_PROTOCOL_VERSION,
        "run_id": launch.run_id,
        "package_id": launch.package_id,
        "package_dir": launch.package_dir,
        "output_dir": launch.output_dir,
        "entrypoint": launch.entrypoint,
        "callable": launch.callable,
        "parameters": parameters,
        "database_path": database_path,
        "package_catalog": installed_package_catalog(paths)?,
    });
    let request_file = File::create(&request_path).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(request_file, &request).map_err(|error| error.to_string())?;

    let mut command = Command::new(&runtime.python);
    command
        .args(["-m", "drpa_runner.cli", "--request"])
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env(
            "DRPA_BROWSER_PROFILE_ROOT",
            paths.workspace_root.join("browser").join("drissionpage"),
        )
        .env("DRPA_RPA_HOME", paths.workspace_root.join("rpa-python"));
    if let Some(python_path) = runtime.python_path {
        command.env("PYTHONPATH", python_path);
    }
    if let Some(browser) = runtime.browser {
        command.env("DRPA_BROWSER_PATH", browser);
    }
    if let Some(rpa_bundle) = runtime.rpa_bundle {
        command.env("DRPA_RPA_BUNDLE", rpa_bundle);
    }
    configure_linux_process_group(&mut command);
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动封装 Python：{error}"))?;
    let process_group = child.id();
    processes.register(&launch.run_id, process_group)?;
    if state.run_is_cancelled(&launch.run_id) {
        processes.cancel(&launch.run_id)?;
    }

    let result = (|| {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "无法读取 Python 标准输出".to_owned())?;
        let mut reader = BufReader::new(stdout);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            let read = reader
                .read_until(b'\n', &mut buffer)
                .map_err(|error| format!("读取运行时事件失败：{error}"))?;
            if read == 0 {
                break;
            }
            let line = decode_runtime_event_line(&buffer);
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<RuntimeEvent>(&line) {
                Ok(event) => {
                    state.record_runtime_event(&launch.run_id, event.clone());
                    if let RuntimeEvent::OpenDirectory { path, sequence } = event
                        && let Err(error) = open_runtime_output_directory(launch, &path)
                    {
                        state.record_runtime_event(
                            &launch.run_id,
                            RuntimeEvent::Warning {
                                sequence,
                                message: error,
                            },
                        );
                    }
                }
                Err(error) => state.fail_run(
                    &launch.run_id,
                    format!("运行时返回了无效事件：{error} · {line}"),
                ),
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("等待 Python 进程失败：{error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if !stderr.is_empty() {
                return Err(format!("Python 进程异常退出：{stderr}"));
            }
        }
        Ok(())
    })();
    processes.unregister(&launch.run_id, process_group);
    result
}

fn installed_package_catalog(paths: &AppPaths) -> Result<serde_json::Value, String> {
    let packages_root = paths.workspace_root.join("packages");
    let mut catalog = serde_json::Map::new();
    if !packages_root.is_dir() {
        return Ok(serde_json::Value::Object(catalog));
    }
    for package_entry in fs::read_dir(&packages_root)
        .map_err(|error| format!("读取 RPAZ 包目录失败：{error}"))?
        .flatten()
        .filter(|entry| entry.path().is_dir())
    {
        let mut versions = fs::read_dir(package_entry.path())
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.join("manifest.yaml").is_file())
            .collect::<Vec<_>>();
        versions.sort();
        let Some(package_dir) = versions.pop() else {
            continue;
        };
        let Ok(source) = fs::read_to_string(package_dir.join("manifest.yaml")) else {
            continue;
        };
        let Ok(manifest) = PackageManifest::from_yaml(&source) else {
            continue;
        };
        let Entrypoint::Python { module, callable } = manifest.entrypoint else {
            continue;
        };
        catalog.insert(
            manifest.id,
            serde_json::json!({
                "package_dir": package_dir,
                "entrypoint": module,
                "callable": callable,
            }),
        );
    }
    Ok(serde_json::Value::Object(catalog))
}

fn collect_project_entries(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(current)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "__pycache__") {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(root) {
                output.push(format!(
                    "{}/",
                    relative.to_string_lossy().replace('\\', "/")
                ));
            }
            collect_project_entries(root, &path, output)?;
        } else if path.is_file() {
            if path
                .extension()
                .is_some_and(|extension| extension == "pyc" || extension == "pyo")
            {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(root) {
                output.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

fn decode_runtime_event_line(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(line) => line.to_owned(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn locate_runtime(paths: &AppPaths) -> Result<RuntimeEnvironment, String> {
    if let Some(python) = std::env::var_os("DRPA_RUNTIME_PYTHON") {
        return Ok(RuntimeEnvironment {
            python: PathBuf::from(python),
            python_path: std::env::var_os("DRPA_RUNTIME_PYTHONPATH").map(PathBuf::from),
            browser: std::env::var_os("DRPA_BROWSER_PATH").map(PathBuf::from),
            rpa_bundle: std::env::var_os("DRPA_RPA_BUNDLE").map(PathBuf::from),
        });
    }

    for root in runtime_roots(paths)? {
        if !root.join("manifest.json").is_file() {
            continue;
        }
        let environment = paths.data_root.join("runtime-environment");
        let python = prepare_sealed_runtime(&root, &environment)?;
        let manifest = read_offline_runtime_manifest(&root)?;
        return Ok(RuntimeEnvironment {
            python,
            python_path: None,
            browser: Some(resolve_runtime_manifest_path(
                &root,
                &manifest.browser_executable,
            )?),
            rpa_bundle: root
                .join("rpa")
                .join("rpa_python.zip")
                .is_file()
                .then(|| root.join("rpa").join("rpa_python.zip")),
        });
    }

    #[cfg(debug_assertions)]
    {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../runtime/python/src");
        if source.is_dir() {
            let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
            let venv_python = environment_python_path(&workspace.join(".venv"));
            return Ok(RuntimeEnvironment {
                python: if venv_python.is_file() {
                    venv_python
                } else {
                    PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
                },
                python_path: Some(source),
                browser: std::env::var_os("DRPA_BROWSER_PATH").map(PathBuf::from),
                rpa_bundle: std::env::var_os("DRPA_RPA_BUNDLE").map(PathBuf::from),
            });
        }
    }

    Err(format!(
        "未找到封装 Python 运行时。请检查发行包内的 runtime 资源，或为源码联调设置 DRPA_RUNTIME_ROOT；工作区：{}",
        paths.workspace_root.display()
    ))
}

pub(crate) fn locate_runtime_python(paths: &AppPaths) -> Result<PathBuf, String> {
    Ok(locate_runtime(paths)?.python)
}

fn runtime_roots(paths: &AppPaths) -> Result<Vec<PathBuf>, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut roots = Vec::new();
    if let Some(root) = std::env::var_os("DRPA_RUNTIME_ROOT") {
        roots.push(PathBuf::from(root));
    }
    if cfg!(target_os = "linux")
        && let Some(resource_dir) = &paths.resource_dir
    {
        roots.push(resource_dir.join("runtime"));
    }
    if let Some(parent) = executable.parent() {
        roots.push(parent.join("runtime"));
        #[cfg(target_os = "macos")]
        if let Some(contents) = parent.parent() {
            roots.push(contents.join("Resources/runtime"));
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(app_image) = std::env::var_os("APPIMAGE")
        && let Some(parent) = Path::new(&app_image).parent()
    {
        roots.push(parent.join("runtime"));
    }
    Ok(roots)
}

fn inspect_runtime_status(paths: &AppPaths) -> Result<RuntimeStatus, String> {
    let root = runtime_roots(paths)?
        .into_iter()
        .find(|candidate| candidate.join("manifest.json").is_file())
        .ok_or_else(|| "未找到与当前平台匹配的封装运行时".to_owned())?;
    let manifest = read_offline_runtime_manifest(&root)?;
    let browser = resolve_runtime_manifest_path(&root, &manifest.browser_executable)?;
    let environment_root = paths.data_root.join("runtime-environment/environment");
    let python = environment_python_path(&environment_root);
    let marker = environment_root.join(".drpa-runtime.json");
    let pyvenv = environment_root.join("pyvenv.cfg");
    let (state, message) = if python.is_file() && marker.is_file() && pyvenv.is_file() {
        ("ready", "Python、Jupyter Kernel 与浏览器自动化依赖已就绪")
    } else if environment_root.exists() {
        (
            "broken",
            "运行环境不完整；请执行修复，应用只会重建 data 内的生成文件",
        )
    } else {
        (
            "notInitialized",
            "运行环境尚未初始化；首次初始化完全离线完成",
        )
    };
    Ok(RuntimeStatus {
        state,
        bundle_version: manifest.bundle_version,
        python_version: manifest.python_version,
        runtime_root: root.display().to_string(),
        environment_root: environment_root.display().to_string(),
        browser_executable: browser.display().to_string(),
        message: message.to_owned(),
    })
}

fn environment_python_path(environment: &Path) -> PathBuf {
    environment.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    })
}

fn verify_runtime_imports(runtime: &RuntimeEnvironment) -> Result<(), String> {
    let mut command = Command::new(&runtime.python);
    command
        .args([
            "-I",
            "-c",
            "import drpa_runner, DrissionPage, ipykernel, jupyter_client, rpa, tagui; from drpa_runner import agent_mcp, python_flow; from drpa_runner.context import RuntimeContext; assert hasattr(RuntimeContext, 'open_output_directory'); assert agent_mcp.TOOL_DEFINITIONS; assert python_flow.SCHEMA_VERSION == 1; print('DRPA_RUNTIME_OK')",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_child_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法验证运行环境：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "运行环境依赖验证失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn prepare_sealed_runtime(root: &Path, environment: &Path) -> Result<PathBuf, String> {
    let environment_python = environment_python_path(&environment.join("environment"));
    let manifest = read_offline_runtime_manifest(root)?;
    let bundled = resolve_runtime_manifest_path(root, &manifest.python_executable)?;
    let bootstrap = root.join("bootstrap_runtime.py");
    if !bootstrap.is_file() {
        return Err("封装运行时缺少 bootstrap_runtime.py".to_owned());
    }
    let mut command = Command::new(bundled);
    command
        .arg(bootstrap)
        .arg("--environment")
        .arg(environment.join("environment"))
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_child_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法初始化离线 Python：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "离线 Python 初始化失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if environment_python.is_file() {
        Ok(environment_python)
    } else {
        Err("离线 Python 初始化完成但没有生成解释器".to_owned())
    }
}

fn read_offline_runtime_manifest(root: &Path) -> Result<OfflineRuntimeManifest, String> {
    let path = root.join("manifest.json");
    let source = fs::read_to_string(&path)
        .map_err(|error| format!("无法读取封装运行时清单 {}：{error}", path.display()))?;
    let manifest: OfflineRuntimeManifest =
        serde_json::from_str(&source).map_err(|error| format!("封装运行时清单无效：{error}"))?;
    let expected_platform = if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "macos-arm64"
    } else if cfg!(target_os = "macos") {
        "macos-x86_64"
    } else {
        "linux-x86_64"
    };
    if manifest.platform != expected_platform {
        return Err(format!(
            "运行时平台不匹配：需要 {expected_platform}，实际为 {}",
            manifest.platform
        ));
    }
    Ok(manifest)
}

fn resolve_runtime_manifest_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = safe_relative_path(relative)
        .map_err(|error| format!("运行时清单包含不安全路径：{error}"))?;
    let path = root.join(relative);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("封装运行时缺少文件：{}", path.display()))
    }
}

fn update_worker_command(
    updater: &Path,
    stage: &Path,
    install: &Path,
    launch: &str,
    status_path: &Path,
    restart_request: &Path,
    session_id: &str,
) -> Command {
    let mut command = Command::new(updater);
    command
        .arg("--stage")
        .arg(stage)
        .arg("--install")
        .arg(install)
        .arg("--launch")
        .arg(launch)
        .arg("--status")
        .arg(status_path)
        .arg("--restart-request")
        .arg(restart_request)
        .arg("--session")
        .arg(session_id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(windows)]
fn spawn_windows_update_worker(
    updater: &Path,
    stage: &Path,
    install: &Path,
    launch: &str,
    status_path: &Path,
    restart_request: &Path,
    session_id: &str,
) -> std::io::Result<Child> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let detached_flags = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS;

    let mut command = update_worker_command(
        updater,
        stage,
        install,
        launch,
        status_path,
        restart_request,
        session_id,
    );
    command.creation_flags(detached_flags | CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Ok(child) => Ok(child),
        Err(_) => {
            let mut fallback = update_worker_command(
                updater,
                stage,
                install,
                launch,
                status_path,
                restart_request,
                session_id,
            );
            fallback.creation_flags(detached_flags);
            fallback.spawn()
        }
    }
}

#[cfg(not(windows))]
fn spawn_windows_update_worker(
    updater: &Path,
    stage: &Path,
    install: &Path,
    launch: &str,
    status_path: &Path,
    restart_request: &Path,
    session_id: &str,
) -> std::io::Result<Child> {
    update_worker_command(
        updater,
        stage,
        install,
        launch,
        status_path,
        restart_request,
        session_id,
    )
    .spawn()
}

#[cfg(windows)]
fn hide_child_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_window(_command: &mut Command) {}

#[cfg(target_os = "linux")]
fn configure_linux_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_process_group(_command: &mut Command) {}

#[cfg(target_os = "linux")]
fn signal_linux_process_group(process_group: u32, signal: libc::c_int) -> Result<(), String> {
    let process_group =
        i32::try_from(process_group).map_err(|_| format!("进程组标识超出范围：{process_group}"))?;
    // SAFETY: kill receives a negative, validated child process-group id and a
    // constant POSIX signal. No borrowed memory crosses the FFI boundary.
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!("终止 Linux 任务进程组失败：{error}"))
    }
}

fn initialize_desktop(
    app: tauri::AppHandle,
    #[cfg(windows)] main_window_config: tauri::utils::config::WindowConfig,
    reveal_requested: Arc<AtomicBool>,
) -> Result<(), String> {
    set_startup_progress_handle(&app, 8, "正在读取应用配置", "desktop://tauri.conf.json");
    let data_root = if let Some(path) = std::env::var_os("DRPA_DATA_DIR") {
        PathBuf::from(path)
    } else {
        #[cfg(windows)]
        {
            std::env::current_exe()
                .map_err(|error| format!("无法定位当前程序：{error}"))?
                .parent()
                .ok_or_else(|| "无法定位应用安装目录".to_owned())?
                .join("data")
        }
        #[cfg(not(windows))]
        {
            app.path()
                .app_local_data_dir()
                .map_err(|error| format!("无法定位应用数据目录：{error}"))?
                .join("workspace")
        }
    };
    fs::create_dir_all(&data_root).map_err(|error| format!("创建数据目录失败：{error}"))?;
    set_startup_progress_handle(&app, 18, "正在准备本地数据目录", "workspace://data");

    let workspace_root = workspaces::resolve_active_workspace(&data_root)
        .map_err(|error| format!("载入活动工作区失败：{error}"))?;
    set_startup_progress_handle(&app, 30, "正在切换隔离工作区", "workspace://active");

    knowledge::seed_default_knowledge(&workspace_root)
        .map_err(|error| format!("准备知识文档失败：{error}"))?;
    set_startup_progress_handle(&app, 42, "正在加载知识文档", "knowledge://documents");

    let app_paths = AppPaths {
        data_root: data_root.clone(),
        workspace_root: workspace_root.clone(),
        resource_dir: app.path().resource_dir().ok(),
    };
    let plugin_manager = plugins::PluginManager::default();
    set_startup_progress_handle(&app, 52, "正在定位运行环境", "runtime://python");
    let autostart_paths = app_paths.clone();
    let autostart_workspace = workspace_root.clone();
    let autostart_manager = plugin_manager.clone();
    let autostart_gate = Arc::clone(&reveal_requested);
    std::thread::spawn(move || {
        // Avoid competing with first-run WebView2 and knowledge initialization on slow disks.
        // Executable plugins remain independent from sealed Python initialization.
        for _ in 0..600 {
            if autostart_gate.load(AtomicOrdering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(750));
                let _ = plugins::start_autostart_plugins(
                    &autostart_workspace,
                    None,
                    &autostart_manager,
                );
                if let Ok(runtime) = locate_runtime(&autostart_paths) {
                    let _ = plugins::start_autostart_plugins(
                        &autostart_workspace,
                        Some(&runtime.python),
                        &autostart_manager,
                    );
                }
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    set_startup_progress_handle(&app, 64, "正在建立运行索引", "host://workspace.sqlite3");
    let host_state = HostState::try_new(workspace_root.clone())
        .map_err(|error| format!("建立运行索引失败：{error}"))?;
    app.manage(host_state);
    app.manage(app_paths);
    app.manage(StudioKernelManager {
        sessions: Arc::new(Mutex::new(HashMap::new())),
    });
    app.manage(RunProcessManager::default());
    app.manage(system_metrics::SystemMetricsMonitor::default());
    app.manage(plugin_manager);
    app.manage(local_dify::LocalDifyServiceManager::default());
    app.manage(credential_vault::CredentialVaultManager::default());
    app.manage(agent_runtime::AgentRunManager::with_workspace(
        &workspace_root,
    ));
    app.manage(agent_browser::AgentBrowserManager::with_workspace(
        &workspace_root,
    ));

    set_startup_progress_handle(&app, 76, "正在装载本地服务", "services://agent-tools");
    let scheduler = automations::SchedulerManager::default();
    let scheduler_runner = scheduler.clone();
    app.manage(scheduler);
    scheduler_runner.start(app.clone());
    acknowledge_windows_update_startup(&data_root)
        .map_err(|error| format!("确认更新状态失败：{error}"))?;

    #[cfg(windows)]
    {
        set_startup_progress_handle(&app, 86, "正在启动界面引擎", "webview://user-data");
        let webview_data = data_root.join("webview2-user-data");
        fs::create_dir_all(&webview_data)
            .map_err(|error| format!("创建 WebView2 数据目录失败：{error}"))?;
        let page_load_app = app.clone();
        tauri::WebviewWindowBuilder::from_config(&app, &main_window_config)
            .map_err(|error| format!("读取主窗口配置失败：{error}"))?
            .data_directory(webview_data)
            .initialization_script(
                r#"
                (() => {
                  const report = (message) => {
                    const invoke = window.__TAURI_INTERNALS__?.invoke;
                    if (invoke) {
                      invoke("report_startup_frontend_error", {
                        message: String(message).slice(0, 700),
                      }).catch(() => {});
                    }
                  };
                  window.addEventListener("error", (event) => {
                    report(`JavaScript: ${event.message || "unknown error"} · ${event.filename || "unknown"}:${event.lineno || 0}`);
                  });
                  window.addEventListener("unhandledrejection", (event) => {
                    const reason = event.reason instanceof Error
                      ? `${event.reason.name}: ${event.reason.message}`
                      : String(event.reason);
                    report(`Promise: ${reason}`);
                  });
                  window.setTimeout(() => {
                    const root = document.getElementById("root");
                    if (!root || root.childElementCount === 0) {
                      const scripts = Array.from(document.scripts)
                        .map((script) => script.src || "inline")
                        .join(", ");
                      report(`React 未挂载 · readyState=${document.readyState} · scripts=${scripts || "none"}`);
                    }
                  }, 5000);
                })();
                "#,
            )
            .on_page_load(move |_window, payload| match payload.event() {
                tauri::webview::PageLoadEvent::Started => set_startup_progress_handle(
                    &page_load_app,
                    91,
                    "正在读取界面资源",
                    "ui://index.html",
                ),
                tauri::webview::PageLoadEvent::Finished => {
                    set_startup_progress_handle(
                        &page_load_app,
                        96,
                        "正在挂载工作台",
                        "ui://react",
                    );
                    let reveal_app = page_load_app.clone();
                    std::thread::spawn(move || {
                        // Hidden WebView2 windows can suspend animation frames and timers. Once
                        // all page resources have loaded, give React a short commit window and
                        // then reveal unless the startup error hook reported a real failure.
                        std::thread::sleep(std::time::Duration::from_millis(650));
                        complete_startup_from_handle(&reveal_app);
                    });
                }
            })
            .build()
            .map_err(|error| format!("创建主界面失败：{error}"))?;
    }

    set_startup_progress_handle(&app, 94, "正在渲染工作台", "ui://index.html");
    start_startup_watchdog(app, reveal_requested);
    Ok(())
}

fn start_startup_watchdog(app: tauri::AppHandle, reveal_requested: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        // React normally invokes complete_startup itself. The watchdog only repairs a lost IPC
        // call after the root has real content; it never reveals an empty or white WebView.
        for iteration in 0..90 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if reveal_requested.load(AtomicOrdering::Acquire) {
                return;
            }
            if iteration % 3 == 2
                && let Some(main) = app.get_webview_window("main")
            {
                let _ = main.eval(
                    "(() => { const root = document.getElementById('root'); \
                     if (root && root.childElementCount > 0 && window.__TAURI_INTERNALS__) { \
                       window.__TAURI_INTERNALS__.invoke('complete_startup').catch(() => {}); \
                     } })();",
                );
            }
        }
        if !reveal_requested.load(AtomicOrdering::Acquire) {
            set_startup_error_handle(
                &app,
                "工作台响应超时",
                "ui://react-timeout · 启动图仍在运行",
            );
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let startup_started = std::time::Instant::now();
    #[cfg(windows)]
    let mut context = tauri::generate_context!();
    #[cfg(not(windows))]
    let context = tauri::generate_context!();
    #[cfg(windows)]
    let main_window_index = context
        .config()
        .app
        .windows
        .iter()
        .position(|window| window.label == "main")
        .expect("Windows main window configuration is missing");
    #[cfg(windows)]
    let main_window_config = context.config_mut().app.windows.remove(main_window_index);
    #[cfg(windows)]
    context
        .config_mut()
        .app
        .windows
        .retain(|window| window.label != "splashscreen");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let reveal_requested = Arc::new(AtomicBool::new(false));
            app.manage(StartupState {
                started_at: startup_started,
                reveal_requested: Arc::clone(&reveal_requested),
                progress: Mutex::new(StartupProgress {
                    progress: 2,
                    label: "正在启动 DRPA".to_owned(),
                    current_file: "desktop://bootstrap".to_owned(),
                    phase: "loading".to_owned(),
                    revision: 1,
                }),
            });
            #[cfg(windows)]
            app.manage(native_splash::NativeSplash::start().map_err(std::io::Error::other)?);
            if let Some(splash) = app.get_webview_window("splashscreen") {
                let _ = splash.set_always_on_top(false);
            }
            #[cfg(windows)]
            {
                let initialize_app = app.handle().clone();
                std::thread::Builder::new()
                    .name("drpa-startup-initializer".to_owned())
                    .spawn(move || {
                        if let Err(error) = initialize_desktop(
                            initialize_app.clone(),
                            main_window_config,
                            reveal_requested,
                        ) {
                            set_startup_error_handle(&initialize_app, "初始化失败", &error);
                        }
                    })
                    .map_err(std::io::Error::other)?;
            }
            #[cfg(not(windows))]
            initialize_desktop(app.handle().clone(), reveal_requested)
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_workspace_snapshot,
            report_ui_ready,
            report_ui_input_ready,
            get_startup_status,
            report_startup_frontend_error,
            complete_startup,
            install_package,
            uninstall_package,
            start_run,
            cancel_run,
            get_run_detail,
            open_run_output_directory,
            list_studio_projects,
            create_studio_project,
            rename_studio_project,
            open_installed_package,
            read_project_file,
            write_project_file,
            create_project_directory,
            rename_project_entry,
            delete_project_entry,
            delete_studio_project,
            import_project_file,
            build_studio_project,
            install_studio_project,
            run_studio_project,
            prepare_studio_kernel,
            execute_studio_cell,
            complete_studio_python,
            inspect_studio_python,
            python_flow::parse_python_flow,
            python_flow::render_python_flow,
            python_flow::validate_python_flow,
            restart_studio_kernel,
            database::get_workspace_database_info,
            database::list_database_tables,
            database::describe_database_table,
            database::execute_database_sql,
            database::get_database_schema_context,
            database::open_workspace_database_directory,
            database::list_remote_database_profiles,
            database::save_remote_database_profile,
            database::delete_remote_database_profile,
            database::test_remote_database_connection,
            database::list_remote_database_tables,
            database::describe_remote_database_table,
            database::execute_remote_database_sql,
            database::get_remote_database_schema_context,
            database::execute_dashboard_database_query,
            dashboard::get_bi_dashboard,
            dashboard::save_bi_dashboard,
            dashboard::reset_bi_dashboard,
            credential_vault::get_vault_status,
            credential_vault::begin_vault_setup,
            credential_vault::complete_vault_setup,
            credential_vault::unlock_vault,
            credential_vault::unlock_vault_with_recovery,
            credential_vault::lock_vault,
            credential_vault::list_vault_credentials,
            credential_vault::get_vault_credential,
            credential_vault::save_vault_credential,
            credential_vault::delete_vault_credential,
            credential_vault::start_vault_service,
            credential_vault::stop_vault_service,
            credential_vault::export_vault_recovery_code,
            knowledge_base::list_knowledge_bases,
            knowledge_base::create_knowledge_base,
            knowledge_base::delete_knowledge_base,
            knowledge_base::list_knowledge_base_sources,
            knowledge_base::import_knowledge_base_files,
            knowledge_base::import_knowledge_base_directory,
            knowledge_base::add_knowledge_base_text,
            knowledge_base::add_knowledge_base_url,
            knowledge_base::delete_knowledge_base_source,
            knowledge_base::search_knowledge_base,
            agent_documents::import_agent_document,
            agent_documents::list_agent_attachments,
            agent_documents::delete_agent_attachment,
            agent_documents::list_agent_artifacts,
            agent_documents::export_agent_artifact,
            automations::list_automation_plans,
            automations::create_automation_plan,
            automations::update_automation_plan,
            automations::delete_automation_plan,
            automations::set_automation_plan_enabled,
            automations::run_automation_plan_now,
            automations::list_automation_runs,
            local_dify::list_local_dify_apps,
            local_dify::create_local_dify_app,
            local_dify::save_local_dify_app,
            local_dify::validate_local_dify_workflow,
            local_dify::create_local_dify_workflow_node,
            local_dify::delete_local_dify_app,
            local_dify::list_local_dify_providers,
            local_dify::save_local_dify_provider,
            local_dify::delete_local_dify_provider,
            local_dify::test_local_dify_provider,
            local_dify::run_local_dify_app,
            local_dify::list_local_dify_runs,
            local_dify::publish_local_dify_app,
            local_dify::get_local_dify_app_api_token,
            local_dify::check_local_dify_compatibility,
            local_dify::import_local_dify_dsl,
            local_dify::export_local_dify_dsl,
            local_dify::get_local_dify_service_status,
            local_dify::start_local_dify_service,
            local_dify::stop_local_dify_service,
            run_agent_turn,
            cancel_agent_run,
            get_agent_run,
            list_agent_extensions,
            install_agent_extension,
            set_agent_extension_enabled,
            remove_agent_extension,
            agent_config::get_agent_workspace_config,
            agent_config::write_agent_workspace_document,
            agent_config::read_agent_skill,
            agent_config::read_agent_skill_package,
            agent_config::write_agent_skill,
            agent_config::write_agent_skill_package,
            agent_config::read_agent_skill_file,
            agent_config::write_agent_skill_file,
            agent_config::create_agent_skill_directory,
            agent_config::rename_agent_skill_path,
            agent_config::delete_agent_skill_path,
            agent_config::delete_agent_skill,
            agent_sessions::list_agent_projects,
            agent_sessions::create_agent_project,
            agent_sessions::rename_agent_project,
            agent_sessions::list_agent_sessions,
            agent_sessions::create_agent_session,
            agent_sessions::get_agent_session,
            agent_sessions::save_agent_session,
            agent_sessions::rename_agent_session,
            agent_sessions::move_agent_session,
            agent_sessions::delete_agent_session,
            plugins::list_plugins,
            plugins::install_plugin,
            plugins::save_plugin_config,
            plugins::set_plugin_enabled,
            start_plugin,
            plugins::stop_plugin,
            plugins::uninstall_plugin,
            plugins::get_plugin_logs,
            plugins::test_plugin_connection,
            plugins::run_plugin_debugger,
            list_plugin_tools,
            invoke_plugin_tool,
            plugins::list_plugin_projects,
            plugins::create_plugin_project,
            plugins::validate_plugin_project,
            plugins::build_plugin_project,
            get_runtime_status,
            system_metrics::get_system_metrics,
            get_platform_capabilities,
            initialize_runtime,
            repair_runtime,
            apply_windows_update,
            get_windows_update_status,
            get_latest_windows_update_status,
            restart_for_windows_update,
            get_data_directory,
            open_workspace_data_directory,
            export_user_data,
            import_user_data,
            open_build_output_directory,
            get_current_user,
            knowledge::list_knowledge_entries,
            knowledge::read_knowledge_file,
            knowledge::write_knowledge_file,
            knowledge::create_knowledge_entry,
            knowledge::rename_knowledge_entry,
            knowledge::delete_knowledge_entry,
            knowledge::import_knowledge_files,
            knowledge::export_knowledge_file,
            workspaces::list_workspaces,
            workspaces::create_workspace,
            workspaces::switch_workspace
        ])
        .run(context)
        .expect("failed to run DRPA Next desktop host");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_manifest_selects_the_base_python_not_the_venv_template() {
        let root = std::env::temp_dir().join(format!("drpa-runtime-test-{}", Uuid::new_v4()));
        let base = root.join("python/cpython-3.11.9-windows-x86_64-none/python.exe");
        let template =
            root.join("python/cpython-3.11.9-windows-x86_64-none/Lib/venv/scripts/nt/python.exe");
        fs::create_dir_all(base.parent().unwrap()).unwrap();
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(&base, b"base").unwrap();
        fs::write(&template, b"venv launcher requiring pyvenv.cfg").unwrap();

        let resolved = resolve_runtime_manifest_path(
            &root,
            "python/cpython-3.11.9-windows-x86_64-none/python.exe",
        )
        .unwrap();
        assert_eq!(resolved, base);
        assert_ne!(resolved, template);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_manifest_rejects_parent_traversal() {
        let root = std::env::temp_dir().join(format!("drpa-runtime-test-{}", Uuid::new_v4()));
        let error = resolve_runtime_manifest_path(&root, "../python.exe").unwrap_err();
        assert!(error.contains("不安全路径"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_runtime_roots_include_tauri_resource_directory() {
        let resource_dir =
            std::env::temp_dir().join(format!("drpa-resource-test-{}", Uuid::new_v4()));
        let paths = AppPaths {
            data_root: std::env::temp_dir(),
            workspace_root: std::env::temp_dir(),
            resource_dir: Some(resource_dir.clone()),
        };

        let roots = runtime_roots(&paths).unwrap();

        assert!(roots.contains(&resource_dir.join("runtime")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_cancel_terminates_the_worker_process_group() {
        let processes = RunProcessManager::default();
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 30 & wait");
        configure_linux_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        let process_group = child.id();
        processes.register("run-test", process_group).unwrap();

        processes.cancel("run-test").unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = signal_linux_process_group(process_group, libc::SIGKILL);
        let _ = child.wait();
        panic!("Linux worker process group did not terminate after cancellation");
    }

    #[test]
    fn studio_file_collection_ignores_python_bytecode_cache() {
        let root = std::env::temp_dir().join(format!("drpa-project-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("__pycache__")).unwrap();
        fs::write(root.join("main.py"), b"def main(ctx): pass\n").unwrap();
        fs::write(root.join("__pycache__/main.cpython-311.pyc"), b"bytecode").unwrap();
        fs::write(root.join("module.pyo"), b"optimized bytecode").unwrap();

        let mut files = Vec::new();
        collect_files(&root, &root, &mut files).unwrap();
        files.sort();

        assert_eq!(files, vec!["main.py"]);

        let mut entries = Vec::new();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/helper.py"), b"VALUE = 1\n").unwrap();
        collect_project_entries(&root, &root, &mut entries).unwrap();
        entries.sort();
        assert_eq!(entries, vec!["main.py", "src/", "src/helper.py"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn new_studio_project_contains_ai_development_readme() {
        let workspace_root =
            std::env::temp_dir().join(format!("drpa-studio-template-test-{}", Uuid::new_v4()));
        let project_name = "AI # 演示\n`项目`";

        let project = create_studio_project_in_workspace(project_name, &workspace_root).unwrap();
        let project_root = workspace_root.join("projects").join(&project.id);
        let readme = fs::read_to_string(project_root.join("README.md")).unwrap();

        assert!(project.files.contains(&"README.md".to_owned()));
        assert!(readme.starts_with("# AI \\# 演示 \\`项目\\`\n"));
        assert_eq!(readme.matches("# AI \\# 演示 \\`项目\\`").count(), 1);
        for required in [
            "`manifest.yaml` → `main.py` → `README.md`",
            "RPAZ schema 2",
            "`ctx.params`",
            "`ctx.log`",
            "`ctx.progress(...)`",
            "`ctx.output_file(...)`",
            "`ctx.open_output_directory()`",
            "`ctx.sql`",
            "`ctx.browser(...)`",
            "离线依赖",
            "`import rpa as r`",
            "RPA for Python",
            "`offline/requirements/runtime.txt`",
            "全量 sealed runtime",
            "预检",
            "导出 RPAZ",
            "Python Flow",
            "唯一事实源（SSOT）",
        ] {
            assert!(readme.contains(required), "README missing {required}");
        }
        assert!(!readme.contains("manifest 的 `dependencies`"));
        assert!(!readme.contains("wheel 放入 RPAZ 约定目录"));

        fs::write(project_root.join("README.md"), "# 用户文档\n").unwrap();
        ensure_studio_project_readme(&project_root, project_name).unwrap();
        assert_eq!(
            fs::read_to_string(project_root.join("README.md")).unwrap(),
            "# 用户文档\n"
        );

        let manifest = fs::read_to_string(project_root.join("manifest.yaml")).unwrap();
        let parsed = PackageManifest::from_yaml(&manifest).unwrap();
        assert_eq!(parsed.name, project_name);

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn runtime_event_decoder_tolerates_non_utf8_log_lines() {
        let line = decode_runtime_event_line(
            b"{\"type\":\"log\",\"sequence\":1,\"level\":\"info\",\"scope\":\"package\",\"message\":\"bad: \xff\"}\n",
        );
        let event = serde_json::from_str::<RuntimeEvent>(&line).unwrap();
        assert!(matches!(event, RuntimeEvent::Log { .. }));
    }

    #[test]
    fn update_session_id_requires_exactly_32_hex_characters() {
        assert!(validate_update_session_id("0123456789abcdef0123456789ABCDEF").is_ok());
        assert!(validate_update_session_id("../updates/session").is_err());
        assert!(validate_update_session_id("0123456789abcdef").is_err());
    }

    #[test]
    fn update_minimum_version_uses_numeric_semver_core() {
        assert!(version_is_at_least("0.3.0", "0.3.0"));
        assert!(version_is_at_least("0.3.1-preview-2", "0.3.0"));
        assert!(!version_is_at_least("0.2.99", "0.3.0"));
        assert!(!version_is_at_least("preview", "0.3.0"));
    }
}
