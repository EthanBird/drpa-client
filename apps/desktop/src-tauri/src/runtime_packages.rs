use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
    let package_name = validate_package_name(&package_name)?.to_owned();
    let paths = paths.inner().clone();
    let catalog = async_runtime::spawn_blocking(move || {
        let runtime = locate_runtime(&paths)?;
        let uv_cache = paths.data_root.join("cache/uv");
        fs::create_dir_all(&uv_cache).map_err(|error| format!("创建 uv 缓存目录失败：{error}"))?;
        let backend = resolve_backend(&runtime, &paths);
        let installed = scan_packages(&runtime)?;
        let wanted = canonical_package_name(&package_name);
        if !installed
            .iter()
            .any(|item| item.removable && canonical_package_name(&item.name) == wanted)
        {
            return Err(format!("当前 Profile 的用户包层中未安装：{package_name}"));
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
                    .arg(&package_name)
                    .env("UV_CACHE_DIR", &uv_cache);
                run_mutation(&mut command, "uv 卸载 Python 包")?;
            }
            PackageBackend::Pip { .. } => {
                uninstall_with_pip(&runtime, &package_name)?;
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

fn uninstall_with_pip(runtime: &RuntimeEnvironment, package_name: &str) -> Result<(), String> {
    let mut command = Command::new(&runtime.python);
    command
        .args(["-m", "pip", "uninstall", "--yes"])
        .arg(package_name)
        .env("PYTHONPATH", &runtime.package_overlay)
        .env("PYTHONNOUSERSITE", "1");
    run_mutation(&mut command, "pip 卸载 Python 包")?;
    let remaining = scan_packages(runtime)?;
    if remaining.iter().any(|item| {
        item.removable && canonical_package_name(&item.name) == canonical_package_name(package_name)
    }) {
        return Err(format!(
            "pip 未能从当前 Profile 的用户包层移除 {package_name}；基础运行时未被修改"
        ));
    }
    Ok(())
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
}
