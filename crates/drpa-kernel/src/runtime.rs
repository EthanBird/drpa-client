use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use drpa_host::{HostState, RunLaunch};
use drpa_install::{
    BrowserSource, ComponentLeaseGuard, ComponentSelection, InstallLayout, resolve_browser,
};
use drpa_package::{Entrypoint, PackageManifest};
use drpa_protocol::{RUNTIME_PROTOCOL_VERSION, RuntimeEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PYTHON_MODULE_BOOTSTRAP: &str = r#"import os, runpy, site, sys
for key in ('DRPA_PYTHON_RUNTIME_SOURCE', 'DRPA_PYTHON_PACKAGE_PATH'):
    path = os.environ.get(key, '').strip()
    if path:
        site.addsitedir(path)
        if path in sys.path:
            sys.path.remove(path)
        sys.path.insert(0, path)
module = sys.argv[1]
sys.argv = sys.argv[1:]
runpy.run_module(module, run_name='__main__', alter_sys=True)
"#;

#[derive(Debug, Clone)]
pub struct RuntimeEnvironment {
    pub python: PathBuf,
    pub python_path: Option<PathBuf>,
    pub browser: Option<PathBuf>,
    pub rpa_bundle: Option<PathBuf>,
    pub runtime_root: Option<PathBuf>,
    pub package_overlay: PathBuf,
    pub profile_id: String,
    _lease: Option<ComponentLeaseGuard>,
    _browser_lease: Option<ComponentLeaseGuard>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub source: String,
    pub python: String,
    pub browser: Option<String>,
    pub runtime_root: Option<String>,
    pub ready: bool,
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileInfo {
    pub id: String,
    pub name: String,
    pub component_version: String,
    pub python_version: String,
    pub environment_mode: String,
    pub features: Vec<String>,
    pub selected: bool,
    pub ready: bool,
    pub in_use: usize,
    pub runtime_root: String,
    pub environment_root: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfflineRuntimeManifest {
    #[serde(default)]
    bundle_version: String,
    platform: String,
    #[serde(default)]
    python_version: String,
    python_executable: String,
    #[serde(default)]
    browser_executable: String,
    #[serde(default = "default_environment_mode")]
    environment_mode: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    features: Vec<String>,
}

fn default_environment_mode() -> String {
    "materialized".to_owned()
}

pub fn locate_runtime(
    layout: Option<&InstallLayout>,
    data_root: &Path,
    workspace_root: &Path,
    legacy_roots: &[PathBuf],
) -> Result<RuntimeEnvironment, String> {
    if let Some(python) = std::env::var_os("DRPA_RUNTIME_PYTHON") {
        let (browser, browser_lease) = resolve_browser_with_lease(layout, "core-browser")?;
        return Ok(RuntimeEnvironment {
            python: PathBuf::from(python),
            python_path: std::env::var_os("DRPA_RUNTIME_PYTHONPATH").map(PathBuf::from),
            browser,
            rpa_bundle: std::env::var_os("DRPA_RPA_BUNDLE").map(PathBuf::from),
            runtime_root: None,
            package_overlay: data_root
                .join("runtime-package-overlays/environment-override/site-packages"),
            profile_id: "environment.override".to_owned(),
            _lease: None,
            _browser_lease: browser_lease,
        });
    }

    if let Some(layout) = layout {
        let mut providers = layout
            .providers_for("runtime.python")
            .map_err(|error| error.to_string())?;
        providers.sort_by_key(|(id, _, _)| (id != "org.drpa.python-runtime", id.clone()));
        let selected = selected_profile_id(workspace_root);
        if let Some((id, selection, root)) = selected
            .as_deref()
            .and_then(|selected| providers.iter().find(|(id, _, _)| id == selected))
            .cloned()
            .or_else(|| providers.into_iter().next())
        {
            return runtime_from_root(layout, data_root, id, selection, root);
        }
    }

    for root in legacy_roots {
        if root.join("manifest.json").is_file() {
            return runtime_from_legacy_root(
                layout,
                data_root,
                "legacy.bundled".to_owned(),
                None,
                root.clone(),
            );
        }
    }
    Err(
        "未找到 Python 运行组件；可运行 `drpa component install <python-runtime.drpac>` 安装"
            .to_owned(),
    )
}

pub fn runtime_status(
    layout: Option<&InstallLayout>,
    data_root: &Path,
    workspace_root: &Path,
    legacy_roots: &[PathBuf],
) -> RuntimeStatus {
    match locate_runtime(layout, data_root, workspace_root, legacy_roots) {
        Ok(runtime) => RuntimeStatus {
            source: if runtime.runtime_root.as_ref().is_some_and(|root| {
                root.components()
                    .any(|part| part.as_os_str() == "components")
            }) {
                "component".to_owned()
            } else if runtime.runtime_root.is_some() {
                "legacy".to_owned()
            } else {
                "environment".to_owned()
            },
            ready: runtime.python.is_file() || runtime.python.components().count() == 1,
            python: runtime.python.display().to_string(),
            browser: runtime.browser.map(|path| path.display().to_string()),
            runtime_root: runtime.runtime_root.map(|path| path.display().to_string()),
            profile_id: runtime.profile_id,
        },
        Err(error) => RuntimeStatus {
            source: error,
            ready: false,
            python: String::new(),
            browser: None,
            runtime_root: None,
            profile_id: String::new(),
        },
    }
}

pub fn list_runtime_profiles(
    layout: Option<&InstallLayout>,
    data_root: &Path,
    workspace_root: &Path,
    legacy_roots: &[PathBuf],
) -> Result<Vec<RuntimeProfileInfo>, String> {
    let selected = selected_profile_id(workspace_root);
    let mut profiles = Vec::new();
    if let Some(layout) = layout {
        for (id, selection, root) in layout
            .providers_for("runtime.python")
            .map_err(|error| error.to_string())?
        {
            let source = match fs::read_to_string(root.join("manifest.json")) {
                Ok(source) => source,
                Err(_) => continue,
            };
            let manifest: OfflineRuntimeManifest = match serde_json::from_str::<
                OfflineRuntimeManifest,
            >(&source)
            {
                Ok(manifest) if manifest.platform == drpa_install::current_platform() => manifest,
                _ => continue,
            };
            let environment_root = profile_environment_root(
                data_root,
                &id,
                &selection.version,
                &source,
                &root,
                &manifest,
            );
            let ready = profile_ready(&root, &environment_root, &manifest);
            let in_use = layout
                .active_component_leases(&id, Some(&selection.version))
                .map(|leases| leases.len())
                .unwrap_or(0);
            profiles.push(RuntimeProfileInfo {
                id: id.clone(),
                name: if manifest.display_name.trim().is_empty() {
                    id.clone()
                } else {
                    manifest.display_name.clone()
                },
                component_version: selection.version,
                python_version: manifest.python_version,
                environment_mode: manifest.environment_mode,
                features: manifest.features,
                selected: selected.as_deref() == Some(id.as_str()),
                ready,
                in_use,
                runtime_root: root.display().to_string(),
                environment_root: environment_root.display().to_string(),
            });
        }
    }
    for root in legacy_roots {
        if !root.join("manifest.json").is_file()
            || profiles
                .iter()
                .any(|profile| Path::new(&profile.runtime_root) == root)
        {
            continue;
        }
        let source = match fs::read_to_string(root.join("manifest.json")) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let manifest: OfflineRuntimeManifest =
            match serde_json::from_str::<OfflineRuntimeManifest>(&source) {
                Ok(manifest) if manifest.platform == drpa_install::current_platform() => manifest,
                _ => continue,
            };
        let id = "legacy.bundled".to_owned();
        let version = if manifest.bundle_version.is_empty() {
            "legacy"
        } else {
            manifest.bundle_version.as_str()
        };
        let environment_root =
            profile_environment_root(data_root, &id, version, &source, root, &manifest);
        let ready = profile_ready(root, &environment_root, &manifest);
        profiles.push(RuntimeProfileInfo {
            id: id.clone(),
            name: if manifest.display_name.trim().is_empty() {
                "内置 Python 运行环境".to_owned()
            } else {
                manifest.display_name.clone()
            },
            component_version: version.to_owned(),
            python_version: manifest.python_version,
            environment_mode: manifest.environment_mode.clone(),
            features: manifest.features,
            selected: selected.as_deref() == Some(id.as_str()),
            ready,
            in_use: 0,
            runtime_root: root.display().to_string(),
            environment_root: environment_root.display().to_string(),
        });
    }
    profiles.sort_by_key(|profile| {
        (
            profile.id != "org.drpa.python-runtime",
            profile.name.clone(),
        )
    });
    if !profiles.iter().any(|profile| profile.selected)
        && let Some(profile) = profiles.first_mut()
    {
        profile.selected = true;
    }
    Ok(profiles)
}

pub fn select_runtime_profile(
    layout: Option<&InstallLayout>,
    data_root: &Path,
    workspace_root: &Path,
    legacy_roots: &[PathBuf],
    profile_id: &str,
) -> Result<RuntimeProfileInfo, String> {
    let profiles = list_runtime_profiles(layout, data_root, workspace_root, legacy_roots)?;
    let selected = profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| format!("运行时 Profile 不存在或与当前平台不兼容：{profile_id}"))?;
    let path = workspace_root.join("system/runtime-profile.json");
    let parent = path
        .parent()
        .ok_or_else(|| "Profile 配置路径无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(
        ".runtime-profile-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let value = serde_json::json!({
        "schema": 1,
        "profileId": profile_id,
        "selectedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    });
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    replace_profile_file(&temporary, &path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })?;
    Ok(RuntimeProfileInfo {
        selected: true,
        ..selected
    })
}

pub fn execute_python_run(
    state: &HostState,
    workspace_root: &Path,
    runtime: &RuntimeEnvironment,
    launch: &RunLaunch,
    parameters: &serde_json::Value,
    mut event_sink: impl FnMut(&RuntimeEvent),
) -> Result<(), String> {
    fs::create_dir_all(&launch.output_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(workspace_root.join("rpa-python")).map_err(|error| error.to_string())?;
    let run_root = launch
        .output_dir
        .parent()
        .ok_or_else(|| "无效的运行输出目录".to_owned())?;
    fs::create_dir_all(run_root).map_err(|error| error.to_string())?;
    let request_path = run_root.join("request.json");
    let database_path = workspace_root.join("databases/workspace.sqlite3");
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
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
        "package_catalog": installed_package_catalog(workspace_root)?,
    });
    serde_json::to_writer_pretty(
        File::create(&request_path).map_err(|error| error.to_string())?,
        &request,
    )
    .map_err(|error| error.to_string())?;

    let mut command = Command::new(&runtime.python);
    command
        .args([
            "-I",
            "-c",
            PYTHON_MODULE_BOOTSTRAP,
            "drpa_runner.cli",
            "--request",
        ])
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("DRPA_PYTHON_PACKAGE_PATH", &runtime.package_overlay)
        .env(
            "DRPA_BROWSER_PROFILE_ROOT",
            workspace_root.join("browser/drissionpage"),
        )
        .env("DRPA_RPA_HOME", workspace_root.join("rpa-python"));
    if let Some(path) = &runtime.python_path {
        command.env("PYTHONPATH", path);
        command.env("DRPA_PYTHON_RUNTIME_SOURCE", path);
    }
    if let Some(path) = &runtime.browser {
        command.env("DRPA_BROWSER_PATH", path);
    }
    if let Some(path) = &runtime.rpa_bundle {
        command.env("DRPA_RPA_BUNDLE", path);
    }
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动 Python 运行组件：{error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 Python 标准输出".to_owned())?;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("读取运行事件失败：{error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<RuntimeEvent>(&line) {
            Ok(event) => {
                state.record_runtime_event(&launch.run_id, event.clone());
                event_sink(&event);
            }
            Err(error) => state.fail_run(
                &launch.run_id,
                format!("运行时返回了无效事件：{error} · {line}"),
            ),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 Python 运行组件失败：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if error.is_empty() {
            format!("Python 进程异常退出：{}", output.status)
        } else {
            format!("Python 进程异常退出：{error}")
        };
        state.fail_run(&launch.run_id, message.clone());
        Err(message)
    }
}

fn runtime_from_root(
    layout: &InstallLayout,
    data_root: &Path,
    profile_id: String,
    selection: ComponentSelection,
    root: PathBuf,
) -> Result<RuntimeEnvironment, String> {
    runtime_from_legacy_root(Some(layout), data_root, profile_id, Some(selection), root)
}

fn runtime_from_legacy_root(
    layout: Option<&InstallLayout>,
    data_root: &Path,
    profile_id: String,
    selection: Option<ComponentSelection>,
    root: PathBuf,
) -> Result<RuntimeEnvironment, String> {
    let source = fs::read_to_string(root.join("manifest.json"))
        .map_err(|error| format!("无法读取运行时清单：{error}"))?;
    let manifest: OfflineRuntimeManifest =
        serde_json::from_str(&source).map_err(|error| format!("运行时清单无效：{error}"))?;
    if manifest.platform != drpa_install::current_platform() {
        return Err(format!("运行时平台不匹配：{}", manifest.platform));
    }
    let version = selection
        .as_ref()
        .map(|selection| selection.version.as_str())
        .unwrap_or("legacy");
    let lease = selection
        .as_ref()
        .zip(layout)
        .map(|(selection, layout)| {
            layout.acquire_component_lease(&profile_id, &selection.version, "core-runtime")
        })
        .transpose()
        .map_err(|error| error.to_string())?;
    let environment_digest = runtime_digest(&profile_id, version, source.as_bytes());
    let environment_root = data_root
        .join("runtime-environments")
        .join(&environment_digest)
        .join("environment");
    let package_overlay = data_root
        .join("runtime-package-overlays")
        .join(&environment_digest)
        .join("site-packages");
    let python = match manifest.environment_mode.as_str() {
        "frozen" => resolve_required(&root, &manifest.python_executable)?,
        "materialized" => {
            let bundled_python = resolve_required(&root, &manifest.python_executable)?;
            let python = environment_python_path(&environment_root);
            let bootstrap = root.join("bootstrap_runtime.py");
            if !bootstrap.is_file() {
                return Err("运行组件缺少 bootstrap_runtime.py".to_owned());
            }
            let mut command = Command::new(bundled_python);
            command
                .arg(&bootstrap)
                .arg("--environment")
                .arg(&environment_root)
                .current_dir(&root)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            hide_child_window(&mut command);
            let output = command
                .output()
                .map_err(|error| format!("无法初始化 Python 运行组件：{error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "Python 运行组件初始化失败：{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            if !python.is_file() {
                return Err("运行组件没有生成 Python 解释器".to_owned());
            }
            python
        }
        mode => return Err(format!("不支持的 Python 环境模式：{mode}")),
    };
    let (preferred_browser, browser_lease) = resolve_browser_with_lease(layout, "core-browser")?;
    let browser = preferred_browser.or_else(|| {
        if manifest.browser_executable.is_empty() {
            None
        } else {
            resolve_optional(&root, &manifest.browser_executable)
        }
    });
    let rpa_bundle = root
        .join("rpa/rpa_python.zip")
        .is_file()
        .then(|| root.join("rpa/rpa_python.zip"));
    let runtime = RuntimeEnvironment {
        python,
        python_path: root.join("vendor").is_dir().then(|| root.join("vendor")),
        browser,
        rpa_bundle,
        runtime_root: Some(root),
        package_overlay,
        profile_id,
        _lease: lease,
        _browser_lease: browser_lease,
    };
    Ok(runtime)
}

fn resolve_browser_with_lease(
    layout: Option<&InstallLayout>,
    purpose: &str,
) -> Result<(Option<PathBuf>, Option<ComponentLeaseGuard>), String> {
    let browser = resolve_browser(layout).map_err(|error| error.to_string())?;
    let lease = match (layout, browser.as_ref().map(|item| &item.source)) {
        (Some(layout), Some(BrowserSource::Component(id))) => layout
            .active_component(id)
            .map_err(|error| error.to_string())?
            .map(|(selection, _)| layout.acquire_component_lease(id, &selection.version, purpose))
            .transpose()
            .map_err(|error| error.to_string())?,
        _ => None,
    };
    Ok((browser.map(|item| item.executable), lease))
}

fn selected_profile_id(workspace_root: &Path) -> Option<String> {
    if let Some(value) = std::env::var_os("DRPA_RUNTIME_PROFILE") {
        return Some(value.to_string_lossy().trim().to_owned());
    }
    let source = fs::read_to_string(workspace_root.join("system/runtime-profile.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&source)
        .ok()?
        .get("profileId")?
        .as_str()
        .map(str::to_owned)
}

fn runtime_digest(profile_id: &str, version: &str, manifest: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(profile_id.as_bytes());
    digest.update([0]);
    digest.update(version.as_bytes());
    digest.update([0]);
    digest.update(manifest);
    hex::encode(digest.finalize())[..20].to_owned()
}

fn profile_environment_root(
    data_root: &Path,
    profile_id: &str,
    version: &str,
    manifest_source: &str,
    runtime_root: &Path,
    manifest: &OfflineRuntimeManifest,
) -> PathBuf {
    if manifest.environment_mode == "frozen" {
        runtime_root.to_path_buf()
    } else {
        data_root
            .join("runtime-environments")
            .join(runtime_digest(
                profile_id,
                version,
                manifest_source.as_bytes(),
            ))
            .join("environment")
    }
}

fn profile_ready(
    runtime_root: &Path,
    environment_root: &Path,
    manifest: &OfflineRuntimeManifest,
) -> bool {
    if manifest.environment_mode == "frozen" {
        resolve_required(runtime_root, &manifest.python_executable).is_ok()
    } else {
        environment_python_path(environment_root).is_file()
            && environment_root.join(".drpa-runtime.json").is_file()
    }
}

#[cfg(windows)]
fn replace_profile_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_profile_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|error| error.to_string())
}

fn installed_package_catalog(workspace_root: &Path) -> Result<serde_json::Value, String> {
    let mut catalog = BTreeMap::new();
    let packages_root = workspace_root.join("packages");
    if !packages_root.is_dir() {
        return Ok(serde_json::json!(catalog));
    }
    for entry in fs::read_dir(packages_root)
        .map_err(|error| error.to_string())?
        .flatten()
        .filter(|entry| entry.path().is_dir())
    {
        let mut versions = fs::read_dir(entry.path())
            .into_iter()
            .flatten()
            .flatten()
            .map(|item| item.path())
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
        catalog.insert(manifest.id, serde_json::json!({ "package_dir": package_dir, "entrypoint": module, "callable": callable }));
    }
    Ok(serde_json::json!(catalog))
}

fn resolve_required(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = safe_join(root, relative)?;
    path.is_file()
        .then_some(path)
        .ok_or_else(|| format!("运行组件缺少文件：{relative}"))
}

fn resolve_optional(root: &Path, relative: &str) -> Option<PathBuf> {
    safe_join(root, relative).ok().filter(|path| path.is_file())
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = relative.replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.is_empty()
        || path.is_absolute()
        || normalized.contains(':')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        Err(format!("不安全路径：{relative}"))
    } else {
        Ok(root.join(path))
    }
}

fn environment_python_path(root: &Path) -> PathBuf {
    root.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    })
}

#[cfg(windows)]
fn hide_child_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use drpa_install::{
        COMPONENT_SCHEMA, ComponentManifest, build_component_pack, current_platform,
    };
    use tempfile::TempDir;

    fn install_frozen_profile(
        layout: &InstallLayout,
        temporary: &TempDir,
        id: &str,
        name: &str,
        python_version: &str,
    ) {
        let source = temporary.path().join(format!("source-{python_version}"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("python"), b"frozen-python").unwrap();
        fs::write(
            source.join("manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": 2,
                "bundleVersion": "test",
                "displayName": name,
                "platform": current_platform(),
                "pythonVersion": python_version,
                "pythonExecutable": "python",
                "environmentMode": "frozen",
                "features": ["stdlib"],
            }))
            .unwrap(),
        )
        .unwrap();
        let pack = temporary.path().join(format!("{id}.drpac"));
        build_component_pack(
            &source,
            &pack,
            ComponentManifest {
                schema: COMPONENT_SCHEMA,
                id: id.to_owned(),
                version: "1.0.0".to_owned(),
                platform: current_platform().to_owned(),
                display_name: name.to_owned(),
                description: String::new(),
                provides: vec!["runtime.python".to_owned()],
                requires: BTreeMap::new(),
                entrypoints: BTreeMap::from([("python".to_owned(), "python".to_owned())]),
                files: Vec::new(),
            },
        )
        .unwrap();
        layout.install_component(pack).unwrap();
    }

    #[test]
    fn workspace_profile_switch_selects_provider_and_holds_component_lease() {
        let temporary = TempDir::new().unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "test").unwrap();
        install_frozen_profile(
            &layout,
            &temporary,
            "org.drpa.python-runtime",
            "Python Full",
            "3.11",
        );
        install_frozen_profile(
            &layout,
            &temporary,
            "org.drpa.python-runtime.py314-minimal",
            "Python Minimal",
            "3.14",
        );
        let data_root = temporary.path().join("data");
        let workspace_root = temporary.path().join("workspace");
        let selected = select_runtime_profile(
            Some(&layout),
            &data_root,
            &workspace_root,
            &[],
            "org.drpa.python-runtime.py314-minimal",
        )
        .unwrap();
        assert!(selected.selected);

        let runtime = locate_runtime(Some(&layout), &data_root, &workspace_root, &[]).unwrap();
        assert_eq!(runtime.profile_id, "org.drpa.python-runtime.py314-minimal");
        assert!(runtime.python.ends_with("python"));
        assert!(
            layout
                .remove_component("org.drpa.python-runtime.py314-minimal", true)
                .is_err()
        );
        drop(runtime);
        layout
            .remove_component("org.drpa.python-runtime.py314-minimal", true)
            .unwrap();
    }
}
