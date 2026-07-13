use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use drpa_host::{HostState, RunLaunch};
use drpa_package::{PackageManifest, safe_relative_path, validate_package_id};
use drpa_protocol::{PackageSummary, RUNTIME_PROTOCOL_VERSION, RuntimeEvent, WorkspaceSnapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Manager, State};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

struct AppPaths {
    workspace_root: PathBuf,
}

struct StudioKernelManager {
    sessions: Mutex<HashMap<String, StudioKernel>>,
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
        collect_files(&entry.path(), &entry.path(), &mut files)
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
    collect_files(&target, &target, &mut files).map_err(|error| error.to_string())?;
    files.sort();
    Ok(StudioProject {
        id: project_id,
        name: manifest.name,
        files,
    })
}

#[tauri::command]
fn execute_studio_cell(
    project_id: String,
    code: String,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<StudioCellResult, String> {
    validate_project_id(&project_id)?;
    let mut sessions = kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?;
    if !sessions.contains_key(&project_id) {
        sessions.insert(
            project_id.clone(),
            spawn_studio_kernel(&project_id, &paths)?,
        );
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
            collect_files(root, &path, output)?;
        } else if path.is_file() {
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
    version: String,
    target: String,
    files: Vec<WindowsUpdateFile>,
}

#[derive(Deserialize)]
struct WindowsUpdateFile {
    path: String,
    bytes: u64,
    sha256: String,
}

#[tauri::command]
fn apply_windows_update(
    package_path: String,
    app: tauri::AppHandle,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("文件级热更新当前只对 Windows 开放".to_owned());
    }
    let package = Path::new(&package_path);
    let file = File::open(package).map_err(|error| format!("无法打开更新包：{error}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("更新包无效：{error}"))?;
    let manifest_source = {
        let mut entry = archive
            .by_name("update-manifest.json")
            .map_err(|_| "更新包缺少 update-manifest.json".to_owned())?;
        let mut source = String::new();
        entry
            .read_to_string(&mut source)
            .map_err(|error| format!("无法读取更新清单：{error}"))?;
        source
    };
    let manifest: WindowsUpdateManifest = serde_json::from_str(&manifest_source)
        .map_err(|error| format!("更新清单无效：{error}"))?;
    if manifest.schema != 1 || manifest.target != "windows-x86_64" {
        return Err("更新包格式或目标平台不匹配".to_owned());
    }
    if !is_safe_update_version(&manifest.version) {
        return Err("更新包版本标识无效".to_owned());
    }
    if manifest.files.is_empty() || manifest.files.len() > 50_000 {
        return Err("更新包文件数量异常".to_owned());
    }

    let stage = paths
        .workspace_root
        .join("updates/staged")
        .join(format!("{}-{}", manifest.version, Uuid::new_v4().simple()));
    fs::create_dir_all(stage.join("files"))
        .map_err(|error| format!("无法创建更新暂存目录：{error}"))?;
    let extraction = (|| {
        let mut total = 0_u64;
        let mut paths = HashSet::new();
        for item in &manifest.files {
            let relative = safe_relative_path(&item.path)
                .map_err(|error| format!("更新清单包含不安全路径：{error}"))?;
            let normalized = relative.to_string_lossy().replace('\\', "/");
            if !paths.insert(normalized.clone()) {
                return Err(format!("更新清单包含重复路径：{normalized}"));
            }
            if item.sha256.len() != 64
                || !item
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!("更新文件 SHA-256 格式无效：{normalized}"));
            }
            if normalized.starts_with("data/")
                || normalized.eq_ignore_ascii_case("drpa-updater.exe")
            {
                return Err(format!("更新包不得覆盖受保护路径：{normalized}"));
            }
            total = total
                .checked_add(item.bytes)
                .ok_or_else(|| "更新包体积溢出".to_owned())?;
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
            let mut digest = Sha256::new();
            let mut buffer = [0_u8; 1024 * 1024];
            loop {
                let read = entry.read(&mut buffer).map_err(|error| error.to_string())?;
                if read == 0 {
                    break;
                }
                digest.update(&buffer[..read]);
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| error.to_string())?;
            }
            let actual = format!("{:x}", digest.finalize());
            if !actual.eq_ignore_ascii_case(&item.sha256) {
                return Err(format!("更新文件 SHA-256 不匹配：{normalized}"));
            }
        }
        fs::write(stage.join("update-manifest.json"), &manifest_source)
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = extraction {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let install = executable
        .parent()
        .ok_or_else(|| "无法定位安装目录".to_owned())?;
    let updater = install.join("drpa-updater.exe");
    if !updater.is_file() {
        return Err(
            "安装目录缺少 drpa-updater.exe，当前版本不能执行热更新".to_owned(),
        );
    }
    let launch = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无法读取主程序文件名".to_owned())?;
    let mut command = Command::new(updater);
    command
        .arg("--stage")
        .arg(&stage)
        .arg("--install")
        .arg(install)
        .arg("--launch")
        .arg(launch)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_child_window(&mut command);
    command
        .spawn()
        .map_err(|error| format!("无法启动无界面更新器：{error}"))?;
    let version = manifest.version;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(450));
        app.exit(0);
    });
    Ok(format!("更新 {version} 已验证，应用即将重启"))
}

fn is_safe_update_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 80
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
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
        .env("PYTHONDONTWRITEBYTECODE", "1");
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
        .env("PYTHONDONTWRITEBYTECODE", "1");
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
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("读取运行时事件失败：{error}"))?;
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
            return Ok(RuntimeEnvironment {
                python: PathBuf::from(if cfg!(windows) { "python" } else { "python3" }),
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
        ("notInitialized", "运行环境尚未初始化；首次初始化完全离线完成")
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
    let source = fs::read_to_string(&path).map_err(|error| {
        format!(
            "无法读取封装运行时清单 {}：{error}",
            path.display()
        )
    })?;
    let manifest: OfflineRuntimeManifest = serde_json::from_str(&source)
        .map_err(|error| format!("封装运行时清单无效：{error}"))?;
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
            app.manage(AppPaths { workspace_root });
            app.manage(StudioKernelManager {
                sessions: Mutex::new(HashMap::new()),
            });
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
            build_studio_project,
            run_studio_project,
            execute_studio_cell,
            restart_studio_kernel,
            get_runtime_status,
            initialize_runtime,
            repair_runtime,
            apply_windows_update,
            get_data_directory
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
        let template = root.join(
            "python/cpython-3.11.9-windows-x86_64-none/Lib/venv/scripts/nt/python.exe",
        );
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
}
