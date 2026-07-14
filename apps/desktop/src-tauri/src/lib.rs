use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use drpa_host::{HostState, RunLaunch};
use drpa_package::{PackageManifest, safe_relative_path, validate_package_id};
use drpa_protocol::{
    PackageSummary, RUNTIME_PROTOCOL_VERSION, RuntimeEvent, WindowsUpdatePhase,
    WindowsUpdateSession, WindowsUpdateStatus, WorkspaceSnapshot,
};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

mod agent;

const WINDOWS_UPDATE_SCHEMA: u32 = 2;
const WINDOWS_UPDATE_HOST_PROTOCOL: u32 = 2;
const WINDOWS_UPDATE_WORKER_PROTOCOL: u32 = 2;

#[derive(Clone)]
struct AppPaths {
    workspace_root: PathBuf,
}

#[derive(Clone)]
struct StudioKernelManager {
    sessions: Arc<Mutex<HashMap<String, StudioKernel>>>,
}

struct StudioKernel {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Drop for StudioKernel {
    fn drop(&mut self) {
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
struct CurrentUser {
    display_name: String,
    account_name: String,
    initials: String,
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

#[tauri::command]
fn install_package(
    archive_path: String,
    state: State<'_, HostState>,
) -> Result<PackageSummary, String> {
    state
        .install_package(Path::new(&archive_path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
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
) -> Result<String, String> {
    let launch = state
        .prepare_run(&package_id, &profile_id, &parameters)
        .map_err(|error| error.to_string())?;
    let run_id = launch.run_id.clone();
    if let Err(error) = execute_python_run(&state, &paths, &launch, &parameters) {
        state.fail_run(&run_id, error.clone());
        return Err(error);
    }
    Ok(run_id)
}

#[tauri::command]
fn cancel_run(run_id: String, state: State<'_, HostState>) -> Result<(), String> {
    state.cancel_run(&run_id).map_err(|error| error.to_string())
}

#[tauri::command]
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

#[tauri::command]
fn create_studio_project(
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<StudioProject, String> {
    if name.trim().is_empty() {
        return Err("项目名称不能为空".to_owned());
    }
    let (project_id, package_id) = generated_project_ids(name.trim());
    let root = paths.workspace_root.join("projects").join(&project_id);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let yaml_name = serde_json::to_string(name.trim()).map_err(|error| error.to_string())?;
    let manifest = format!(
        "schema: 2\nid: {package_id}\nname: {yaml_name}\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\nruntime:\n  python: \"3.11.*\"\ncapabilities:\n  network:\n    allow: []\n  filesystem:\n    read: []\n    write: [\"$outputs\"]\nparameters: []\n"
    );
    fs::write(root.join("manifest.yaml"), manifest).map_err(|error| error.to_string())?;
    fs::write(
        root.join("main.py"),
        "def main(ctx):\n    ctx.log.info(\"任务开始\")\n    ctx.progress(100, \"任务完成\")\n",
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
    Ok(StudioProject {
        id: project_id,
        name: name.trim().to_owned(),
        files: vec![
            "main.py".to_owned(),
            "manifest.yaml".to_owned(),
            "notebook.ipynb".to_owned(),
        ],
    })
}

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
fn build_studio_project(project_id: String, paths: State<'_, AppPaths>) -> Result<String, String> {
    validate_project_id(&project_id)?;
    let root = paths.workspace_root.join("projects").join(&project_id);
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
    Ok(output.to_string_lossy().into_owned())
}

#[tauri::command]
async fn run_studio_project(
    project_id: String,
    parameters: serde_json::Value,
    state: State<'_, HostState>,
    paths: State<'_, AppPaths>,
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
    if let Err(error) = execute_python_run(&state, &paths, &launch, &parameters) {
        state.fail_run(&run_id, error.clone());
        return Err(error);
    }
    Ok(run_id)
}

#[tauri::command]
fn open_installed_package(
    package_id: String,
    paths: State<'_, AppPaths>,
) -> Result<StudioProject, String> {
    validate_package_id(&package_id).map_err(|error| error.to_string())?;
    let package_root = paths.workspace_root.join("packages").join(&package_id);
    let mut versions: Vec<PathBuf> = fs::read_dir(&package_root)
        .map_err(|_| format!("找不到已安装脚本包：{package_id}"))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("manifest.yaml").is_file())
        .collect();
    versions.sort();
    let source = versions
        .pop()
        .ok_or_else(|| format!("脚本包 {package_id} 没有可编辑版本"))?;
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
async fn run_agent_turn(
    request: agent::AgentTurnRequest,
    paths: State<'_, AppPaths>,
) -> Result<agent::AgentTurnResult, String> {
    let paths = paths.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = locate_runtime(&paths)?;
        agent::run_agent_turn(request, paths.workspace_root.clone(), runtime.python)
    })
    .await
    .map_err(|error| format!("Agent 后台任务失败：{error}"))?
}

#[tauri::command]
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

#[tauri::command]
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
    let session_root = paths
        .workspace_root
        .join("updates/sessions")
        .join(&session_id);
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

#[tauri::command]
fn get_windows_update_status(
    session_id: String,
    paths: State<'_, AppPaths>,
) -> Result<WindowsUpdateStatus, String> {
    validate_update_session_id(&session_id)?;
    let source = fs::read_to_string(
        paths
            .workspace_root
            .join("updates/sessions")
            .join(&session_id)
            .join("status.json"),
    )
    .map_err(|error| format!("读取更新进度失败：{error}"))?;
    serde_json::from_str(&source).map_err(|error| format!("更新进度数据无效：{error}"))
}

#[tauri::command]
fn get_latest_windows_update_status(
    paths: State<'_, AppPaths>,
) -> Result<Option<WindowsUpdateStatus>, String> {
    let sessions = paths.workspace_root.join("updates/sessions");
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

#[tauri::command]
fn restart_for_windows_update(
    session_id: String,
    app: tauri::AppHandle,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_update_session_id(&session_id)?;
    let session_root = paths
        .workspace_root
        .join("updates/sessions")
        .join(&session_id);
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

fn update_session_root(workspace_root: &Path, session_id: &str) -> Result<PathBuf, String> {
    validate_update_session_id(session_id)?;
    Ok(workspace_root.join("updates/sessions").join(session_id))
}

fn acknowledge_windows_update_startup(workspace_root: &Path) -> std::io::Result<()> {
    let Some(session_id) = std::env::var_os("DRPA_UPDATE_SESSION_ID") else {
        return Ok(());
    };
    let session_id = session_id.to_string_lossy();
    let Ok(session_root) = update_session_root(workspace_root, &session_id) else {
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

#[tauri::command]
fn get_runtime_status(paths: State<'_, AppPaths>) -> Result<RuntimeStatus, String> {
    inspect_runtime_status(&paths)
}

#[tauri::command]
fn initialize_runtime(paths: State<'_, AppPaths>) -> Result<RuntimeStatus, String> {
    let runtime = locate_runtime(&paths)?;
    verify_runtime_imports(&runtime)?;
    inspect_runtime_status(&paths)
}

#[tauri::command]
fn repair_runtime(
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<RuntimeStatus, String> {
    kernels
        .sessions
        .lock()
        .map_err(|_| "无法停止 Studio Kernel".to_owned())?
        .clear();
    let generated = paths.workspace_root.join("runtime-environment");
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
    } = locate_runtime(paths)?;
    let project_root = paths.workspace_root.join("projects").join(project_id);
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
        .env("PYTHONIOENCODING", "utf-8");
    if let Some(python_path) = python_path {
        command.env("PYTHONPATH", python_path);
    }
    if let Some(browser) = browser {
        command.env("DRPA_BROWSER_PATH", browser);
    }
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
    launch: &RunLaunch,
    parameters: &serde_json::Value,
) -> Result<(), String> {
    let runtime = locate_runtime(paths)?;
    fs::create_dir_all(&launch.output_dir).map_err(|error| error.to_string())?;
    let run_root = launch
        .output_dir
        .parent()
        .ok_or_else(|| "无效的运行输出目录".to_owned())?;
    fs::create_dir_all(run_root).map_err(|error| error.to_string())?;
    let request_path = run_root.join("request.json");
    let request = serde_json::json!({
        "protocol": RUNTIME_PROTOCOL_VERSION,
        "run_id": launch.run_id,
        "package_id": launch.package_id,
        "package_dir": launch.package_dir,
        "output_dir": launch.output_dir,
        "entrypoint": launch.entrypoint,
        "callable": launch.callable,
        "parameters": parameters,
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
        .env("PYTHONIOENCODING", "utf-8");
    if let Some(python_path) = runtime.python_path {
        command.env("PYTHONPATH", python_path);
    }
    if let Some(browser) = runtime.browser {
        command.env("DRPA_BROWSER_PATH", browser);
    }
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动封装 Python：{error}"))?;

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
            Ok(event) => state.record_runtime_event(&launch.run_id, event),
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
        });
    }

    for root in runtime_roots()? {
        if !root.is_dir() {
            continue;
        }
        let environment = paths.workspace_root.join("runtime-environment");
        let python = prepare_sealed_runtime(&root, &environment)?;
        let manifest = read_offline_runtime_manifest(&root)?;
        return Ok(RuntimeEnvironment {
            python,
            python_path: None,
            browser: Some(resolve_runtime_manifest_path(
                &root,
                &manifest.browser_executable,
            )?),
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
            });
        }
    }

    Err(format!(
        "未找到封装 Python 运行时。请把平台 runtime 放到应用同目录的 runtime 文件夹；工作区：{}",
        paths.workspace_root.display()
    ))
}

fn runtime_roots() -> Result<Vec<PathBuf>, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut roots = Vec::new();
    if let Some(root) = std::env::var_os("DRPA_RUNTIME_ROOT") {
        roots.push(PathBuf::from(root));
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
    let root = runtime_roots()?
        .into_iter()
        .find(|candidate| candidate.is_dir())
        .ok_or_else(|| "未找到随安装包提供的 Windows 运行时".to_owned())?;
    let manifest = read_offline_runtime_manifest(&root)?;
    let browser = resolve_runtime_manifest_path(&root, &manifest.browser_executable)?;
    let environment_root = paths.workspace_root.join("runtime-environment/environment");
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
            "import drpa_runner, DrissionPage, ipykernel, jupyter_client; print('DRPA_RUNTIME_OK')",
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    let mut context = tauri::generate_context!();
    #[cfg(not(windows))]
    let context = tauri::generate_context!();
    #[cfg(windows)]
    let main_window_config = context
        .config_mut()
        .app
        .windows
        .pop()
        .expect("Windows main window configuration is missing");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let workspace_root = if let Some(path) = std::env::var_os("DRPA_DATA_DIR") {
                PathBuf::from(path)
            } else {
                #[cfg(windows)]
                {
                    std::env::current_exe()?
                        .parent()
                        .ok_or_else(|| std::io::Error::other("无法定位应用安装目录"))?
                        .join("data")
                }
                #[cfg(not(windows))]
                {
                    app.path().app_local_data_dir()?.join("workspace")
                }
            };
            fs::create_dir_all(&workspace_root)?;

            #[cfg(windows)]
            {
                let webview_data = workspace_root.join("webview2-user-data");
                fs::create_dir_all(&webview_data)?;
                tauri::WebviewWindowBuilder::from_config(app, &main_window_config)?
                    .data_directory(webview_data)
                    .build()?;
            }

            app.manage(HostState::new(workspace_root.clone()));
            app.manage(AppPaths {
                workspace_root: workspace_root.clone(),
            });
            app.manage(StudioKernelManager {
                sessions: Arc::new(Mutex::new(HashMap::new())),
            });
            acknowledge_windows_update_startup(&workspace_root)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_workspace_snapshot,
            install_package,
            uninstall_package,
            start_run,
            cancel_run,
            list_studio_projects,
            create_studio_project,
            open_installed_package,
            read_project_file,
            write_project_file,
            create_project_directory,
            rename_project_entry,
            delete_project_entry,
            delete_studio_project,
            import_project_file,
            build_studio_project,
            run_studio_project,
            prepare_studio_kernel,
            execute_studio_cell,
            restart_studio_kernel,
            run_agent_turn,
            get_runtime_status,
            initialize_runtime,
            repair_runtime,
            apply_windows_update,
            get_windows_update_status,
            get_latest_windows_update_status,
            restart_for_windows_update,
            get_data_directory,
            get_current_user
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
