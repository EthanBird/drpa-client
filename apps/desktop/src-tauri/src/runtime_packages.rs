use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use drpa_install::{ComponentManifest, build_component_pack, current_platform};
use serde::{Deserialize, Serialize};
use tauri::{State, async_runtime};

use crate::{
    AppPaths, RuntimeEnvironment, StudioKernelManager, hide_child_window,
    installation_root_from_process, locate_runtime, runtime_profile_candidates,
};

const PACKAGE_SCAN_SCRIPT: &str = r#"
import importlib.metadata as metadata
import json
import pathlib
import sys

overlay = pathlib.Path(sys.argv[1]).resolve()
runtime_source = pathlib.Path(sys.argv[2]).resolve()

def normalize(value):
    return value.lower().replace('_', '-').replace('.', '-')

def scan(distributions, source, removable):
    result = {}
    for distribution in distributions:
        name = distribution.metadata.get('Name') or ''
        if not name:
            continue
        try:
            location = str(pathlib.Path(distribution.locate_file('')).resolve())
        except Exception:
            location = ''
        key = normalize(name)
        result[key] = {
            'name': name,
            'version': distribution.version or '',
            'source': source,
            'location': location,
            'removable': removable,
        }
    return result

user = scan(metadata.distributions(path=[str(overlay)]), 'user', True)
runtime = scan(metadata.distributions(), 'runtime', False)
if runtime_source.is_dir():
    runtime.update(scan(metadata.distributions(path=[str(runtime_source)]), 'runtime', False))
for key in user:
    runtime.pop(key, None)
packages = list(user.values()) + list(runtime.values())
packages.sort(key=lambda item: (item['source'] != 'user', item['name'].lower()))
print(json.dumps({'packages': packages}, ensure_ascii=False))
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimePythonPackage {
    name: String,
    version: String,
    source: String,
    location: String,
    removable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimePythonPackageCatalog {
    profile_id: String,
    profile_name: String,
    python_version: String,
    backend: String,
    backend_version: String,
    backend_path: String,
    overlay_root: String,
    packages: Vec<RuntimePythonPackage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeProfileExportResult {
    path: String,
    component_id: String,
    display_name: String,
    description: String,
    package_count: usize,
    file_count: usize,
    bytes: u64,
}

#[derive(Deserialize)]
struct PackageScan {
    packages: Vec<RuntimePythonPackage>,
}

#[derive(Debug, Clone)]
enum PackageBackend {
    Uv {
        executable: PathBuf,
        version: String,
    },
    Pip {
        version: String,
    },
    Unavailable,
}

impl PackageBackend {
    fn kind(&self) -> &'static str {
        match self {
            Self::Uv { .. } => "uv",
            Self::Pip { .. } => "pip",
            Self::Unavailable => "unavailable",
        }
    }

    fn version(&self) -> &str {
        match self {
            Self::Uv { version, .. } | Self::Pip { version } => version,
            Self::Unavailable => "",
        }
    }

    fn path(&self, runtime: &RuntimeEnvironment) -> String {
        match self {
            Self::Uv { executable, .. } => executable.display().to_string(),
            Self::Pip { .. } => format!("{} -m pip", runtime.python.display()),
            Self::Unavailable => String::new(),
        }
    }
}

#[tauri::command(async)]
pub(crate) async fn list_runtime_python_packages(
    paths: State<'_, AppPaths>,
) -> Result<RuntimePythonPackageCatalog, String> {
    let paths = paths.inner().clone();
    async_runtime::spawn_blocking(move || list_packages(&paths))
        .await
        .map_err(|error| format!("Python 包扫描任务异常：{error}"))?
}

#[tauri::command(async)]
pub(crate) async fn install_runtime_python_package(
    requirement: String,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<RuntimePythonPackageCatalog, String> {
    let requirement = validate_requirement(&requirement)?.to_owned();
    let paths = paths.inner().clone();
    let catalog = async_runtime::spawn_blocking(move || {
        let runtime = locate_runtime(&paths)?;
        fs::create_dir_all(&runtime.package_overlay)
            .map_err(|error| format!("创建 Profile 用户包层失败：{error}"))?;
        let uv_cache = paths.data_root.join("cache/uv");
        let pip_cache = paths.data_root.join("cache/pip");
        fs::create_dir_all(&uv_cache).map_err(|error| format!("创建 uv 缓存目录失败：{error}"))?;
        fs::create_dir_all(&pip_cache)
            .map_err(|error| format!("创建 pip 缓存目录失败：{error}"))?;
        let backend = resolve_backend(&runtime, &paths);
        match &backend {
            PackageBackend::Uv { executable, .. } => {
                let mut command = Command::new(executable);
                command
                    .args(["pip", "install", "--python"])
                    .arg(&runtime.python)
                    .arg("--target")
                    .arg(&runtime.package_overlay)
                    .args(["--upgrade", "--no-python-downloads"])
                    .arg(&requirement)
                    .env("UV_CACHE_DIR", &uv_cache);
                run_mutation(&mut command, "uv 安装 Python 包")?;
            }
            PackageBackend::Pip { .. } => {
                let mut command = Command::new(&runtime.python);
                command
                    .args([
                        "-I",
                        "-m",
                        "pip",
                        "install",
                        "--disable-pip-version-check",
                        "--target",
                    ])
                    .arg(&runtime.package_overlay)
                    .arg("--upgrade")
                    .arg(&requirement)
                    .env("PIP_CACHE_DIR", &pip_cache);
                run_mutation(&mut command, "pip 安装 Python 包")?;
            }
            PackageBackend::Unavailable => {
                return Err(
                    "当前 Profile 没有可用的 uv 或 pip；请安装完整 Python Runtime 组件后重试"
                        .to_owned(),
                );
            }
        }
        build_catalog(runtime, backend)
    })
    .await
    .map_err(|error| format!("Python 包安装任务异常：{error}"))??;
    kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?
        .clear();
    Ok(catalog)
}

#[tauri::command(async)]
pub(crate) async fn uninstall_runtime_python_package(
    package_name: String,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<RuntimePythonPackageCatalog, String> {
    uninstall_runtime_python_packages(vec![package_name], paths, kernels).await
}

#[tauri::command(async)]
pub(crate) async fn uninstall_runtime_python_packages(
    package_names: Vec<String>,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
) -> Result<RuntimePythonPackageCatalog, String> {
    let package_names = validate_package_names(package_names)?;
    let paths = paths.inner().clone();
    let catalog = async_runtime::spawn_blocking(move || {
        let runtime = locate_runtime(&paths)?;
        let uv_cache = paths.data_root.join("cache/uv");
        fs::create_dir_all(&uv_cache).map_err(|error| format!("创建 uv 缓存目录失败：{error}"))?;
        let backend = resolve_backend(&runtime, &paths);
        let installed = scan_packages(&runtime)?;
        let installed_user = installed
            .iter()
            .filter(|item| item.removable)
            .map(|item| canonical_package_name(&item.name))
            .collect::<HashSet<_>>();
        let missing = package_names
            .iter()
            .filter(|name| !installed_user.contains(&canonical_package_name(name)))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "当前 Profile 的用户包层中未安装：{}",
                missing.join("、")
            ));
        }
        match &backend {
            PackageBackend::Uv { executable, .. } => {
                let mut command = Command::new(executable);
                command
                    .args(["pip", "uninstall", "--python"])
                    .arg(&runtime.python)
                    .arg("--target")
                    .arg(&runtime.package_overlay)
                    .arg("--no-python-downloads")
                    .args(&package_names)
                    .env("UV_CACHE_DIR", &uv_cache);
                run_mutation(&mut command, "uv 批量卸载 Python 包")?;
            }
            PackageBackend::Pip { .. } => {
                uninstall_with_pip(&runtime, &package_names)?;
            }
            PackageBackend::Unavailable => {
                return Err("当前 Profile 没有可用的 uv 或 pip，无法安全卸载用户包".to_owned());
            }
        }
        build_catalog(runtime, backend)
    })
    .await
    .map_err(|error| format!("Python 包卸载任务异常：{error}"))??;
    kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?
        .clear();
    Ok(catalog)
}

#[tauri::command(async)]
pub(crate) async fn export_runtime_profile_component(
    target_path: String,
    display_name: String,
    description: String,
    paths: State<'_, AppPaths>,
) -> Result<RuntimeProfileExportResult, String> {
    let display_name = validate_export_text(&display_name, "组件名称", 120)?;
    let description = validate_export_text(&description, "组件描述", 1_000)?;
    let target_path = validate_export_path(&target_path)?;
    let paths = paths.inner().clone();
    async_runtime::spawn_blocking(move || {
        export_profile_component(&paths, &target_path, &display_name, &description)
    })
    .await
    .map_err(|error| format!("Python Profile 导出任务异常：{error}"))?
}

fn list_packages(paths: &AppPaths) -> Result<RuntimePythonPackageCatalog, String> {
    let runtime = locate_runtime(paths)?;
    let backend = resolve_backend(&runtime, paths);
    build_catalog(runtime, backend)
}

fn build_catalog(
    runtime: RuntimeEnvironment,
    backend: PackageBackend,
) -> Result<RuntimePythonPackageCatalog, String> {
    let packages = scan_packages(&runtime)?;
    Ok(RuntimePythonPackageCatalog {
        profile_id: runtime._profile_id.clone(),
        profile_name: runtime._profile_name.clone(),
        python_version: python_version(&runtime)?,
        backend: backend.kind().to_owned(),
        backend_version: backend.version().to_owned(),
        backend_path: backend.path(&runtime),
        overlay_root: runtime.package_overlay.display().to_string(),
        packages,
    })
}

fn scan_packages(runtime: &RuntimeEnvironment) -> Result<Vec<RuntimePythonPackage>, String> {
    let mut command = Command::new(&runtime.python);
    command
        .args(["-I", "-c", PACKAGE_SCAN_SCRIPT])
        .arg(&runtime.package_overlay)
        .arg(runtime.runtime_root.join("vendor"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_child_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法读取 Python 包清单：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "读取 Python 包清单失败：{}",
            output_detail(&output.stderr, &output.stdout)
        ));
    }
    let scan: PackageScan = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Python 包清单格式无效：{error}"))?;
    Ok(scan.packages)
}

fn python_version(runtime: &RuntimeEnvironment) -> Result<String, String> {
    let mut command = Command::new(&runtime.python);
    command
        .args([
            "-I",
            "-c",
            "import platform; print(platform.python_version())",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_child_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法读取 Python 版本：{error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!(
            "读取 Python 版本失败：{}",
            output_detail(&output.stderr, &output.stdout)
        ))
    }
}

fn resolve_backend(runtime: &RuntimeEnvironment, paths: &AppPaths) -> PackageBackend {
    for executable in uv_candidates(runtime, paths) {
        let mut command = Command::new(&executable);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hide_child_window(&mut command);
        if let Ok(output) = command.output()
            && output.status.success()
        {
            return PackageBackend::Uv {
                executable,
                version: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            };
        }
    }

    let mut command = Command::new(&runtime.python);
    command
        .args(["-I", "-m", "pip", "--version"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_child_window(&mut command);
    if let Ok(output) = command.output()
        && output.status.success()
    {
        PackageBackend::Pip {
            version: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        }
    } else {
        PackageBackend::Unavailable
    }
}

fn uv_candidates(runtime: &RuntimeEnvironment, paths: &AppPaths) -> Vec<PathBuf> {
    let executable_name = if cfg!(windows) { "uv.exe" } else { "uv" };
    let mut candidates = vec![
        runtime.runtime_root.join("tools").join(executable_name),
        runtime.runtime_root.join(executable_name),
    ];
    if let Ok(install_root) = installation_root_from_process() {
        candidates.push(install_root.join("tools").join(executable_name));
    }
    if let Ok(profiles) = runtime_profile_candidates(paths) {
        candidates.extend(
            profiles
                .into_iter()
                .map(|profile| profile.root.join("tools").join(executable_name)),
        );
    }
    candidates.push(PathBuf::from(executable_name));
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

fn run_mutation(command: &mut Command, operation: &str) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_child_window(command);
    let output = command
        .output()
        .map_err(|error| format!("{operation}无法启动：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{operation}失败：{}",
            output_detail(&output.stderr, &output.stdout)
        ))
    }
}

fn uninstall_with_pip(
    runtime: &RuntimeEnvironment,
    package_names: &[String],
) -> Result<(), String> {
    let mut command = Command::new(&runtime.python);
    command
        .args(["-m", "pip", "uninstall", "--yes"])
        .args(package_names)
        .env("PYTHONPATH", &runtime.package_overlay)
        .env("PYTHONNOUSERSITE", "1");
    run_mutation(&mut command, "pip 批量卸载 Python 包")?;
    let remaining = scan_packages(runtime)?;
    let wanted = package_names
        .iter()
        .map(|name| canonical_package_name(name))
        .collect::<HashSet<_>>();
    let failed = remaining
        .iter()
        .filter(|item| item.removable && wanted.contains(&canonical_package_name(&item.name)))
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        return Err(format!(
            "pip 未能从当前 Profile 的用户包层移除 {}；基础运行时未被修改",
            failed.join("、")
        ));
    }
    Ok(())
}

fn validate_package_names(package_names: Vec<String>) -> Result<Vec<String>, String> {
    if package_names.is_empty() || package_names.len() > 500 {
        return Err("请选择 1 到 500 个用户包".to_owned());
    }
    let mut seen = HashSet::new();
    let mut validated = Vec::new();
    for name in package_names {
        let name = validate_package_name(&name)?.to_owned();
        if seen.insert(canonical_package_name(&name)) {
            validated.push(name);
        }
    }
    Ok(validated)
}

fn validate_export_text(value: &str, label: &str, limit: usize) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.contains(['\0', '\r']) {
        return Err(format!("{label}应为 1 到 {limit} 个有效字符"));
    }
    Ok(value.to_owned())
}

fn validate_export_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() || value.contains('\0') {
        return Err("导出路径无效".to_owned());
    }
    let mut path = PathBuf::from(value);
    if path
        .extension()
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("drpac"))
    {
        path.set_extension("drpac");
    }
    Ok(path)
}

fn export_profile_component(
    paths: &AppPaths,
    target_path: &Path,
    display_name: &str,
    description: &str,
) -> Result<RuntimeProfileExportResult, String> {
    let runtime = locate_runtime(paths)?;
    let target_path = absolute_export_path(target_path)?;
    let runtime_root = fs::canonicalize(&runtime.runtime_root)
        .map_err(|error| format!("无法定位当前 Runtime：{error}"))?;
    if target_path.starts_with(&runtime_root) {
        return Err("不能把组件导出到当前 Runtime 目录内部".to_owned());
    }
    let parent = target_path
        .parent()
        .ok_or_else(|| "导出路径缺少父目录".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{error}"))?;
    let staging = parent.join(format!(
        ".drpa-profile-export-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&staging).map_err(|error| format!("创建导出暂存目录失败：{error}"))?;
    let guard = ExportStaging(staging.clone());
    copy_runtime_tree(&runtime_root, &staging)?;
    if runtime.package_overlay.is_dir() {
        copy_runtime_tree(&runtime.package_overlay, &staging.join("vendor"))?;
    }

    let manifest_path = staging.join("manifest.json");
    let mut runtime_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(|error| format!("读取 Runtime 清单失败：{error}"))?,
    )
    .map_err(|error| format!("Runtime 清单格式无效：{error}"))?;
    runtime_manifest["displayName"] = serde_json::Value::String(display_name.to_owned());
    runtime_manifest["exportDescription"] = serde_json::Value::String(description.to_owned());
    runtime_manifest["exportedAt"] = serde_json::Value::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    );
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&runtime_manifest)
            .map_err(|error| format!("序列化 Runtime 清单失败：{error}"))?,
    )
    .map_err(|error| format!("更新 Runtime 清单失败：{error}"))?;

    let python_entrypoint = runtime_manifest
        .get("pythonExecutable")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let version = runtime_manifest
        .get("bundleVersion")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_owned();
    let component_id = format!(
        "org.drpa.python-profile.{}",
        uuid::Uuid::new_v4().simple().to_string()[..12].to_owned()
    );
    let package_count = scan_packages(&runtime)?
        .into_iter()
        .filter(|package| package.removable)
        .count();
    let temporary_archive = parent.join(format!(
        ".{}-{}.part",
        target_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("python-profile.drpac"),
        uuid::Uuid::new_v4().simple()
    ));
    let archive_guard = ExportArchive(temporary_archive.clone());
    let manifest = build_component_pack(
        &staging,
        &temporary_archive,
        ComponentManifest {
            schema: drpa_install::COMPONENT_SCHEMA,
            id: component_id.clone(),
            version,
            platform: current_platform().to_owned(),
            display_name: display_name.to_owned(),
            description: description.to_owned(),
            provides: vec!["runtime.python".to_owned()],
            requires: BTreeMap::new(),
            entrypoints: (!python_entrypoint.is_empty())
                .then(|| BTreeMap::from([("python".to_owned(), python_entrypoint)]))
                .unwrap_or_default(),
            files: Vec::new(),
        },
    )
    .map_err(|error| format!("生成 .drpac 失败：{error}"))?;
    if target_path.exists() {
        fs::remove_file(&target_path).map_err(|error| format!("替换已有导出文件失败：{error}"))?;
    }
    fs::rename(&temporary_archive, &target_path)
        .map_err(|error| format!("提交导出文件失败：{error}"))?;
    let bytes = fs::metadata(&target_path)
        .map_err(|error| format!("读取导出文件失败：{error}"))?
        .len();
    drop(guard);
    drop(archive_guard);
    Ok(RuntimeProfileExportResult {
        path: target_path.display().to_string(),
        component_id,
        display_name: display_name.to_owned(),
        description: description.to_owned(),
        package_count,
        file_count: manifest.files.len(),
        bytes,
    })
}

fn absolute_export_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|root| root.join(path))
        .map_err(|error| format!("无法解析导出路径：{error}"))
}

fn copy_runtime_tree(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| format!("创建组件目录失败：{error}"))?;
    for entry in fs::read_dir(source).map_err(|error| format!("读取组件目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取组件条目失败：{error}"))?;
        let name = entry.file_name();
        if name == "__pycache__" || name == ".pytest_cache" {
            continue;
        }
        let source_path = entry.path();
        let target_path = target.join(name);
        if source_path.is_dir() {
            copy_runtime_tree(&source_path, &target_path)?;
        } else if source_path.is_file()
            && source_path.extension().is_none_or(|extension| {
                !extension.eq_ignore_ascii_case("pyc") && !extension.eq_ignore_ascii_case("pyo")
            })
        {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("创建组件子目录失败：{error}"))?;
            }
            fs::copy(&source_path, &target_path)
                .map_err(|error| format!("复制组件文件 {} 失败：{error}", source_path.display()))?;
        }
    }
    Ok(())
}

struct ExportStaging(PathBuf);

impl Drop for ExportStaging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ExportArchive(PathBuf);

impl Drop for ExportArchive {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn validate_requirement(requirement: &str) -> Result<&str, String> {
    let requirement = requirement.trim();
    if requirement.is_empty() {
        return Err("请输入要安装的包名或 requirement".to_owned());
    }
    if requirement.len() > 512
        || requirement
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        return Err("Python 包 requirement 过长或包含非法字符".to_owned());
    }
    if requirement.starts_with('-') {
        return Err("这里接受单个包名或 requirement，不接受 pip/uv 命令行参数".to_owned());
    }
    Ok(requirement)
}

fn validate_package_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("Python 包名无效".to_owned());
    }
    Ok(name)
}

fn canonical_package_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut separator = false;
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !separator {
                result.push('-');
                separator = true;
            }
        } else {
            result.extend(character.to_lowercase());
            separator = false;
        }
    }
    result
}

fn output_detail(stderr: &[u8], stdout: &[u8]) -> String {
    let detail = if stderr.is_empty() { stdout } else { stderr };
    let detail = String::from_utf8_lossy(detail).trim().to_owned();
    if detail.len() > 12_000 {
        let boundary = detail
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= 12_000)
            .last()
            .unwrap_or(0);
        format!("{}…", &detail[..boundary])
    } else if detail.is_empty() {
        "进程未返回错误详情".to_owned()
    } else {
        detail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_requirements_are_single_safe_arguments() {
        assert_eq!(
            validate_requirement(" pandas==2.3.0 ").unwrap(),
            "pandas==2.3.0"
        );
        assert!(validate_requirement("--index-url=https://example.invalid").is_err());
        assert!(validate_requirement("pandas\nrequests").is_err());
    }

    #[test]
    fn package_names_follow_python_canonicalization() {
        assert_eq!(
            canonical_package_name("Typing_Extensions"),
            "typing-extensions"
        );
        assert_eq!(canonical_package_name("zope.interface"), "zope-interface");
        assert!(validate_package_name("requests==2").is_err());
    }

    #[test]
    fn batch_package_names_are_validated_and_deduplicated() {
        assert_eq!(
            validate_package_names(vec![
                "Rich".to_owned(),
                "rich".to_owned(),
                "typing_extensions".to_owned()
            ])
            .unwrap(),
            vec!["Rich", "typing_extensions"]
        );
        assert!(validate_package_names(Vec::new()).is_err());
        assert!(validate_package_names(vec!["bad==1".to_owned()]).is_err());
    }

    #[test]
    fn export_paths_receive_the_drpac_extension() {
        assert!(
            validate_export_path("D:/exports/python-profile")
                .unwrap()
                .ends_with("python-profile.drpac")
        );
        assert!(
            validate_export_path("D:/exports/python-profile.drpac")
                .unwrap()
                .ends_with("python-profile.drpac")
        );
    }
}
