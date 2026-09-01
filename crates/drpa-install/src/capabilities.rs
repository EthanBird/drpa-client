use std::env;
use std::path::{Path, PathBuf};

use crate::{InstallError, InstallLayout, Result};

const BROWSER_COMPONENT_ID: &str = "org.drpa.browser.chromium";
const BROWSER_ENTRYPOINT: &str = "browser";
const WEBVIEW_COMPONENT_ID: &str = "org.drpa.webview2-fixed";
const WEBVIEW_ENTRYPOINT: &str = "webview2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserSource {
    Environment,
    Component(String),
    GoogleChrome,
    Chromium,
    MicrosoftEdge,
    Brave,
}

impl BrowserSource {
    pub fn display_name(&self) -> &str {
        match self {
            Self::Environment => "自定义浏览器",
            Self::Component(_) => "DRPA Chromium 组件",
            Self::GoogleChrome => "系统 Google Chrome",
            Self::Chromium => "系统 Chromium",
            Self::MicrosoftEdge => "系统 Microsoft Edge",
            Self::Brave => "系统 Brave",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserResolution {
    pub executable: PathBuf,
    pub source: BrowserSource,
}

pub fn resolve_browser(layout: Option<&InstallLayout>) -> Result<Option<BrowserResolution>> {
    if let Some(explicit) = env::var_os("DRPA_BROWSER_PATH") {
        let executable = resolve_executable(PathBuf::from(explicit)).ok_or_else(|| {
            InstallError::Verification("DRPA_BROWSER_PATH 指向的浏览器不存在".to_owned())
        })?;
        return Ok(Some(BrowserResolution {
            executable,
            source: BrowserSource::Environment,
        }));
    }

    if let Some(layout) = layout {
        if let Some(path) = layout.resolve_entrypoint(BROWSER_COMPONENT_ID, BROWSER_ENTRYPOINT)? {
            return Ok(Some(BrowserResolution {
                executable: path,
                source: BrowserSource::Component(BROWSER_COMPONENT_ID.to_owned()),
            }));
        }
        if let Some((id, path)) =
            layout.resolve_provider_entrypoint("browser.chromium", BROWSER_ENTRYPOINT)?
        {
            return Ok(Some(BrowserResolution {
                executable: path,
                source: BrowserSource::Component(id),
            }));
        }
    }

    Ok(detect_system_browser())
}

pub fn detect_system_browser() -> Option<BrowserResolution> {
    browser_candidates().into_iter().find_map(|(path, source)| {
        resolve_executable(path).map(|executable| BrowserResolution { executable, source })
    })
}

pub fn resolve_fixed_webview2(layout: Option<&InstallLayout>) -> Result<Option<PathBuf>> {
    let Some(layout) = layout else {
        return Ok(None);
    };
    let Some(executable) = layout.resolve_entrypoint(WEBVIEW_COMPONENT_ID, WEBVIEW_ENTRYPOINT)?
    else {
        return Ok(None);
    };
    let root = executable
        .parent()
        .filter(|root| root.join("msedgewebview2.exe").is_file())
        .map(Path::to_path_buf);
    Ok(root)
}

#[cfg(windows)]
pub fn system_webview2_version() -> Option<String> {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString, take_pwstr,
    };
    use windows_core::{PCWSTR, PWSTR};

    let mut version = PWSTR::null();
    // SAFETY: a null browser folder asks WebView2Loader to resolve the registered
    // Evergreen runtime. On success it allocates version with CoTaskMemAlloc;
    // take_pwstr copies and frees that allocation.
    unsafe {
        GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version).ok()?;
    }
    let version = take_pwstr(version);
    (!version.trim().is_empty()).then_some(version)
}

#[cfg(not(windows))]
pub fn system_webview2_version() -> Option<String> {
    None
}

fn resolve_executable(candidate: PathBuf) -> Option<PathBuf> {
    if candidate.is_file() {
        return Some(candidate);
    }
    if candidate.components().count() != 1 {
        return None;
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|root| root.join(&candidate))
        .find(|path| path.is_file())
}

fn browser_candidates() -> Vec<(PathBuf, BrowserSource)> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            let Some(root) = env::var_os(variable).map(PathBuf::from) else {
                continue;
            };
            candidates.extend([
                (
                    root.join("Google/Chrome/Application/chrome.exe"),
                    BrowserSource::GoogleChrome,
                ),
                (
                    root.join("Google/Chrome Beta/Application/chrome.exe"),
                    BrowserSource::GoogleChrome,
                ),
                (
                    root.join("Google/Chrome Dev/Application/chrome.exe"),
                    BrowserSource::GoogleChrome,
                ),
                (
                    root.join("Google/Chrome SxS/Application/chrome.exe"),
                    BrowserSource::GoogleChrome,
                ),
                (
                    root.join("Microsoft/Edge/Application/msedge.exe"),
                    BrowserSource::MicrosoftEdge,
                ),
                (
                    root.join("BraveSoftware/Brave-Browser/Application/brave.exe"),
                    BrowserSource::Brave,
                ),
            ]);
        }
        candidates.extend([
            (PathBuf::from("chrome.exe"), BrowserSource::GoogleChrome),
            (PathBuf::from("chromium.exe"), BrowserSource::Chromium),
            (PathBuf::from("msedge.exe"), BrowserSource::MicrosoftEdge),
            (PathBuf::from("brave.exe"), BrowserSource::Brave),
        ]);
    }
    #[cfg(target_os = "linux")]
    {
        candidates.extend([
            (PathBuf::from("google-chrome"), BrowserSource::GoogleChrome),
            (
                PathBuf::from("google-chrome-stable"),
                BrowserSource::GoogleChrome,
            ),
            (PathBuf::from("chromium"), BrowserSource::Chromium),
            (PathBuf::from("chromium-browser"), BrowserSource::Chromium),
            (
                PathBuf::from("microsoft-edge"),
                BrowserSource::MicrosoftEdge,
            ),
            (PathBuf::from("brave-browser"), BrowserSource::Brave),
        ]);
    }
    #[cfg(target_os = "macos")]
    {
        candidates.extend([
            (
                PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                BrowserSource::GoogleChrome,
            ),
            (
                PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
                BrowserSource::Chromium,
            ),
            (
                PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
                BrowserSource::MicrosoftEdge,
            ),
            (
                PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
                BrowserSource::Brave,
            ),
        ]);
    }
    candidates
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::TempDir;

    use crate::{ComponentManifest, build_component_pack, current_platform};

    use super::*;

    #[test]
    fn installed_browser_component_has_priority_over_system_browser() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("chrome.exe"), b"component-browser").unwrap();
        let archive = temporary.path().join("browser.drpac");
        build_component_pack(
            &source,
            &archive,
            ComponentManifest {
                schema: 1,
                id: BROWSER_COMPONENT_ID.to_owned(),
                version: "2.1.1".to_owned(),
                platform: current_platform().to_owned(),
                display_name: "Chromium".to_owned(),
                description: String::new(),
                provides: vec!["browser.chromium".to_owned()],
                requires: BTreeMap::new(),
                entrypoints: BTreeMap::from([(
                    BROWSER_ENTRYPOINT.to_owned(),
                    "chrome.exe".to_owned(),
                )]),
                files: Vec::new(),
            },
        )
        .unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();
        layout.install_component(archive).unwrap();

        let browser = resolve_browser(Some(&layout)).unwrap().unwrap();

        assert_eq!(
            browser.source,
            BrowserSource::Component(BROWSER_COMPONENT_ID.to_owned())
        );
        assert!(browser.executable.ends_with("chrome.exe"));
    }
}
