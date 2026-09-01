use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use drpa_install::{BrowserResolution, BrowserSource, InstallLayout, resolve_browser};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{AppPaths, StudioKernelManager, agent_browser::AgentBrowserManager};

const BROWSER_SELECTION_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeBrowserCandidate {
    id: String,
    name: String,
    family: String,
    source: String,
    executable: String,
    selected: bool,
    automation_compatible: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeBrowserConfiguration {
    pub(crate) mode: String,
    pub(crate) active_name: String,
    pub(crate) active_family: String,
    pub(crate) active_executable: String,
    pub(crate) automation_compatible: bool,
    pub(crate) message: String,
    pub(crate) candidates: Vec<RuntimeBrowserCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserSelection {
    schema: u32,
    mode: String,
    path: String,
    family: String,
    selected_at: u64,
}

#[tauri::command(async)]
pub(crate) fn list_runtime_browsers(
    paths: State<'_, AppPaths>,
) -> Result<RuntimeBrowserConfiguration, String> {
    browser_configuration(&paths)
}

#[tauri::command(async)]
pub(crate) fn select_runtime_browser(
    path: Option<String>,
    family: Option<String>,
    paths: State<'_, AppPaths>,
    kernels: State<'_, StudioKernelManager>,
    browsers: State<'_, AgentBrowserManager>,
) -> Result<RuntimeBrowserConfiguration, String> {
    let selection = match path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => BrowserSelection {
            schema: BROWSER_SELECTION_SCHEMA,
            mode: "auto".to_owned(),
            path: String::new(),
            family: String::new(),
            selected_at: now_millis(),
        },
        Some(raw_path) => {
            let executable = validate_browser_executable(Path::new(raw_path))?;
            let detected_family = family
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(validate_browser_family)
                .transpose()?
                .unwrap_or_else(|| infer_browser_family(&executable));
            BrowserSelection {
                schema: BROWSER_SELECTION_SCHEMA,
                mode: "manual".to_owned(),
                path: executable.display().to_string(),
                family: detected_family,
                selected_at: now_millis(),
            }
        }
    };
    write_selection(&paths, &selection)?;
    kernels
        .sessions
        .lock()
        .map_err(|_| "Studio Kernel 状态已损坏".to_owned())?
        .clear();
    browsers.reset_hosts()?;
    browser_configuration(&paths)
}

pub(crate) fn resolve_automation_browser(
    paths: &AppPaths,
    layout: Option<&InstallLayout>,
) -> Result<Option<BrowserResolution>, String> {
    let Some(selection) = read_selection(paths)? else {
        return resolve_browser(layout).map_err(|error| error.to_string());
    };
    if selection.mode == "auto" {
        return resolve_browser(layout).map_err(|error| error.to_string());
    }
    if selection.family == "firefox" {
        return Ok(None);
    }
    let Ok(executable) = validate_browser_executable(Path::new(&selection.path)) else {
        return Ok(None);
    };
    Ok(Some(BrowserResolution {
        executable,
        source: BrowserSource::Environment,
    }))
}

pub(crate) fn allows_runtime_manifest_fallback(paths: &AppPaths) -> bool {
    !matches!(
        read_selection(paths),
        Ok(Some(BrowserSelection { mode, .. })) if mode == "manual"
    )
}

pub(crate) fn browser_configuration(
    paths: &AppPaths,
) -> Result<RuntimeBrowserConfiguration, String> {
    let install_root = crate::installation_root_from_process().ok();
    let layout = install_root
        .as_deref()
        .and_then(|root| InstallLayout::open(root).ok());
    let selection = read_selection(paths)?;
    let auto = resolve_browser(layout.as_ref()).map_err(|error| error.to_string())?;
    let active = match selection
        .as_ref()
        .filter(|selection| selection.mode == "manual")
    {
        Some(selection) => {
            let executable = PathBuf::from(&selection.path);
            Some((
                browser_display_name(&executable),
                selection.family.clone(),
                executable,
                "手动选择".to_owned(),
            ))
        }
        None => auto.as_ref().map(|browser| {
            (
                browser.source.display_name().to_owned(),
                "chromium".to_owned(),
                browser.executable.clone(),
                "自动检测".to_owned(),
            )
        }),
    };
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    if let Some(browser) = auto {
        let selected = active
            .as_ref()
            .is_some_and(|(_, _, path, _)| path == &browser.executable);
        push_candidate(
            &mut candidates,
            &mut seen,
            browser.executable,
            "chromium",
            browser.source.display_name(),
            "DRPA / 系统自动检测",
            selected,
        );
    }
    for (path, family, name, source) in browser_candidates() {
        if let Some(executable) = resolve_executable(path) {
            let selected = active
                .as_ref()
                .is_some_and(|(_, _, path, _)| path == &executable);
            push_candidate(
                &mut candidates,
                &mut seen,
                executable,
                family,
                name,
                source,
                selected,
            );
        }
    }
    if let Some((name, family, executable, _)) = &active {
        push_candidate(
            &mut candidates,
            &mut seen,
            executable.clone(),
            family,
            name,
            "手动选择",
            true,
        );
    }
    candidates.sort_by(|left, right| {
        right
            .selected
            .cmp(&left.selected)
            .then_with(|| left.family.cmp(&right.family))
            .then_with(|| left.name.cmp(&right.name))
    });
    let (active_name, active_family, active_executable, source) = active.unwrap_or_else(|| {
        (
            "未检测到浏览器".to_owned(),
            "none".to_owned(),
            String::new().into(),
            "自动检测".to_owned(),
        )
    });
    let automation_compatible = active_family == "chromium" && active_executable.is_file();
    let message = if active_family == "firefox" {
        "Firefox 已设为默认浏览器；当前 DrissionPage Agent/RPA 自动化仍使用 Chromium 协议，因此浏览器工具会暂时停用。".to_owned()
    } else if automation_compatible {
        format!("{source}到 Chromium 兼容浏览器，Agent、Studio 与 RPAZ 新任务立即使用该配置。")
    } else {
        "未找到可用浏览器；可手动选择 Chrome、Edge、Chromium、Brave、Vivaldi、Opera 或 Firefox。"
            .to_owned()
    };
    Ok(RuntimeBrowserConfiguration {
        mode: selection.map_or_else(|| "auto".to_owned(), |selection| selection.mode),
        active_name,
        active_family,
        active_executable: active_executable.display().to_string(),
        automation_compatible,
        message,
        candidates,
    })
}

fn push_candidate(
    candidates: &mut Vec<RuntimeBrowserCandidate>,
    seen: &mut HashSet<String>,
    executable: PathBuf,
    family: &str,
    name: &str,
    source: &str,
    selected: bool,
) {
    let key = executable.to_string_lossy().to_ascii_lowercase();
    if !seen.insert(key.clone()) {
        if selected
            && let Some(candidate) = candidates.iter_mut().find(|candidate| candidate.id == key)
        {
            candidate.selected = true;
        }
        return;
    }
    candidates.push(RuntimeBrowserCandidate {
        id: key,
        name: name.to_owned(),
        family: family.to_owned(),
        source: source.to_owned(),
        executable: executable.display().to_string(),
        selected,
        automation_compatible: family == "chromium",
    });
}

fn selection_path(paths: &AppPaths) -> PathBuf {
    paths.workspace_root.join("system/browser-selection.json")
}

fn read_selection(paths: &AppPaths) -> Result<Option<BrowserSelection>, String> {
    let path = selection_path(paths);
    if !path.is_file() {
        return Ok(None);
    }
    let selection: BrowserSelection = serde_json::from_slice(
        &fs::read(&path).map_err(|error| format!("读取浏览器配置失败：{error}"))?,
    )
    .map_err(|error| format!("浏览器配置格式无效：{error}"))?;
    if selection.schema != BROWSER_SELECTION_SCHEMA
        || !matches!(selection.mode.as_str(), "auto" | "manual")
    {
        return Err("浏览器配置版本或模式无效".to_owned());
    }
    if selection.mode == "manual" {
        validate_browser_family(&selection.family)?;
    }
    Ok(Some(selection))
}

fn write_selection(paths: &AppPaths, selection: &BrowserSelection) -> Result<(), String> {
    let path = selection_path(paths);
    let parent = path
        .parent()
        .ok_or_else(|| "浏览器配置路径无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建浏览器配置目录失败：{error}"))?;
    let temporary = parent.join(format!(
        ".browser-selection-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(selection)
            .map_err(|error| format!("序列化浏览器配置失败：{error}"))?,
    )
    .map_err(|error| format!("写入浏览器配置失败：{error}"))?;
    replace_selection_file(&temporary, &path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

#[cfg(windows)]
fn replace_selection_file(source: &Path, destination: &Path) -> Result<(), String> {
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
        Err(format!(
            "无法原子更新浏览器配置：{}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_selection_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|error| format!("无法原子更新浏览器配置：{error}"))
}

fn validate_browser_family(value: &str) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "chromium" => Ok("chromium".to_owned()),
        "firefox" => Ok("firefox".to_owned()),
        _ => Err("浏览器内核应为 chromium 或 firefox".to_owned()),
    }
}

fn validate_browser_executable(path: &Path) -> Result<PathBuf, String> {
    let executable = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("无法解析浏览器路径：{error}"))?
            .join(path)
    };
    if !executable.is_file() {
        return Err(format!("浏览器可执行文件不存在：{}", executable.display()));
    }
    Ok(executable)
}

fn infer_browser_family(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("firefox") {
        "firefox".to_owned()
    } else {
        "chromium".to_owned()
    }
}

fn browser_display_name(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Browser")
        .to_ascii_lowercase();
    if name.contains("firefox") {
        "Mozilla Firefox".to_owned()
    } else if name.contains("msedge") {
        "Microsoft Edge".to_owned()
    } else if name.contains("brave") {
        "Brave".to_owned()
    } else if name.contains("vivaldi") {
        "Vivaldi".to_owned()
    } else if name.contains("opera") {
        "Opera".to_owned()
    } else if name.contains("chromium") {
        "Chromium".to_owned()
    } else {
        "Google Chrome / Chromium".to_owned()
    }
}

fn resolve_executable(candidate: PathBuf) -> Option<PathBuf> {
    if candidate.is_file() {
        return Some(candidate);
    }
    if candidate.components().count() != 1 {
        return None;
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|root| root.join(&candidate))
        .find(|path| path.is_file())
}

fn browser_candidates() -> Vec<(PathBuf, &'static str, &'static str, &'static str)> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            let Some(root) = std::env::var_os(variable).map(PathBuf::from) else {
                continue;
            };
            candidates.extend([
                (
                    root.join("Google/Chrome/Application/chrome.exe"),
                    "chromium",
                    "Google Chrome",
                    "系统安装",
                ),
                (
                    root.join("Microsoft/Edge/Application/msedge.exe"),
                    "chromium",
                    "Microsoft Edge",
                    "系统安装",
                ),
                (
                    root.join("BraveSoftware/Brave-Browser/Application/brave.exe"),
                    "chromium",
                    "Brave",
                    "系统安装",
                ),
                (
                    root.join("Vivaldi/Application/vivaldi.exe"),
                    "chromium",
                    "Vivaldi",
                    "系统安装",
                ),
                (
                    root.join("Programs/Opera/opera.exe"),
                    "chromium",
                    "Opera",
                    "系统安装",
                ),
                (
                    root.join("Mozilla Firefox/firefox.exe"),
                    "firefox",
                    "Mozilla Firefox",
                    "系统安装",
                ),
            ]);
        }
        candidates.extend([
            (
                PathBuf::from("chrome.exe"),
                "chromium",
                "Google Chrome",
                "PATH",
            ),
            (
                PathBuf::from("chromium.exe"),
                "chromium",
                "Chromium",
                "PATH",
            ),
            (
                PathBuf::from("msedge.exe"),
                "chromium",
                "Microsoft Edge",
                "PATH",
            ),
            (PathBuf::from("brave.exe"), "chromium", "Brave", "PATH"),
            (PathBuf::from("vivaldi.exe"), "chromium", "Vivaldi", "PATH"),
            (PathBuf::from("opera.exe"), "chromium", "Opera", "PATH"),
            (
                PathBuf::from("firefox.exe"),
                "firefox",
                "Mozilla Firefox",
                "PATH",
            ),
        ]);
    }
    #[cfg(target_os = "linux")]
    {
        candidates.extend([
            (
                PathBuf::from("google-chrome"),
                "chromium",
                "Google Chrome",
                "PATH",
            ),
            (
                PathBuf::from("google-chrome-stable"),
                "chromium",
                "Google Chrome",
                "PATH",
            ),
            (PathBuf::from("chromium"), "chromium", "Chromium", "PATH"),
            (
                PathBuf::from("chromium-browser"),
                "chromium",
                "Chromium",
                "PATH",
            ),
            (
                PathBuf::from("microsoft-edge"),
                "chromium",
                "Microsoft Edge",
                "PATH",
            ),
            (PathBuf::from("brave-browser"), "chromium", "Brave", "PATH"),
            (PathBuf::from("vivaldi"), "chromium", "Vivaldi", "PATH"),
            (PathBuf::from("opera"), "chromium", "Opera", "PATH"),
            (
                PathBuf::from("firefox"),
                "firefox",
                "Mozilla Firefox",
                "PATH",
            ),
        ]);
    }
    #[cfg(target_os = "macos")]
    {
        candidates.extend([
            (
                PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                "chromium",
                "Google Chrome",
                "系统安装",
            ),
            (
                PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
                "chromium",
                "Chromium",
                "系统安装",
            ),
            (
                PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
                "chromium",
                "Microsoft Edge",
                "系统安装",
            ),
            (
                PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
                "chromium",
                "Brave",
                "系统安装",
            ),
            (
                PathBuf::from("/Applications/Vivaldi.app/Contents/MacOS/Vivaldi"),
                "chromium",
                "Vivaldi",
                "系统安装",
            ),
            (
                PathBuf::from("/Applications/Firefox.app/Contents/MacOS/firefox"),
                "firefox",
                "Mozilla Firefox",
                "系统安装",
            ),
        ]);
    }
    candidates
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_firefox_without_misclassifying_chromium_forks() {
        assert_eq!(infer_browser_family(Path::new("firefox.exe")), "firefox");
        assert_eq!(infer_browser_family(Path::new("vivaldi.exe")), "chromium");
        assert_eq!(
            infer_browser_family(Path::new("custom-browser.exe")),
            "chromium"
        );
    }
}
