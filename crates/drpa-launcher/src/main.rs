#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use drpa_install::{InstallLayout, discover_installation, resolve_browser};

const DESKTOP_COMPONENT_ID: &str = "org.drpa.desktop-ui";
const DESKTOP_ENTRYPOINT: &str = "desktop";

fn main() -> ExitCode {
    match launch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            show_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn launch() -> Result<(), String> {
    let launcher =
        env::current_exe().map_err(|error| format!("无法定位 DRPA Launcher：{error}"))?;
    let install_root = resolve_install_root(&launcher)?;
    let desktop = match resolve_desktop_executable(&launcher, &install_root) {
        Ok(desktop) => desktop,
        Err(desktop_error) => {
            if launch_component_installer(&install_root, None).is_ok() {
                return Ok(());
            }
            return Err(desktop_error);
        }
    };
    if !prepare_webview2_environment(&install_root) {
        launch_component_installer(&install_root, Some("org.drpa.webview2-fixed"))?;
        return Ok(());
    }
    let mut command = Command::new(&desktop);
    command
        .args(env::args_os().skip(1))
        .current_dir(&install_root)
        .env("DRPA_INSTALL_ROOT", &install_root)
        .env("DRPA_LAUNCHED_BY", &launcher);
    prepare_component_environment(&mut command, &install_root)?;
    command
        .spawn()
        .map_err(|error| format!("无法启动桌面组件 {}：{error}", desktop.display()))?;
    Ok(())
}

fn resolve_install_root(launcher: &Path) -> Result<PathBuf, String> {
    if let Some(root) = env::var_os("DRPA_INSTALL_ROOT") {
        let root = PathBuf::from(root);
        InstallLayout::open(&root).map_err(|error| error.to_string())?;
        return Ok(root);
    }
    for root in launcher.ancestors().skip(1).take(5) {
        if root.join(drpa_install::INSTALL_MARKER).is_file() {
            return Ok(root.to_path_buf());
        }
    }
    discover_installation(None)
        .map(|layout| layout.root().to_path_buf())
        .map_err(|error| format!("无法定位 DRPA 安装目录：{error}"))
}

fn launch_component_installer(install_root: &Path, required: Option<&str>) -> Result<(), String> {
    let installer = component_installer_executable(install_root)
        .ok_or_else(|| "缺少 DRPA 组件安装向导".to_owned())?;
    let mut command = Command::new(&installer);
    command
        .arg("--install-root")
        .arg(install_root)
        .current_dir(install_root)
        .env("DRPA_INSTALL_ROOT", install_root);
    if let Some(required) = required {
        command.args(["--required", required]);
    }
    command
        .spawn()
        .map_err(|error| format!("无法启动组件安装向导：{error}"))?;
    Ok(())
}

fn component_installer_executable(install_root: &Path) -> Option<PathBuf> {
    if let Some(path) = env::var_os("DRPA_COMPONENT_INSTALLER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    #[allow(unused_mut)]
    let mut candidates = vec![
        install_root.join("DRPA Component Installer.exe"),
        install_root.join("drpa-component-installer"),
    ];
    if let Ok(executable) = env::current_exe()
        && let Some(parent) = executable.parent()
    {
        candidates.push(parent.join("DRPA Component Installer.exe"));
        candidates.push(parent.join("drpa-component-installer"));
    }
    #[cfg(target_os = "linux")]
    candidates.push(PathBuf::from("/usr/lib/drpa-next/drpa-component-installer"));
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
fn prepare_webview2_environment(install_root: &Path) -> bool {
    let layout = InstallLayout::open(install_root).ok();
    let fixed_runtime = drpa_install::resolve_fixed_webview2(layout.as_ref())
        .ok()
        .flatten();
    if let Some(runtime) = fixed_runtime {
        // This process is still single-threaded. The child desktop inherits the
        // resolved fixed-runtime override, including older desktop components.
        unsafe {
            env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", runtime);
        }
        return true;
    }
    if drpa_install::system_webview2_version().is_some() {
        // Do not let a stale process-level fixed-runtime override shadow the
        // registered Evergreen runtime when upgrading Core independently.
        unsafe {
            env::remove_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER");
        }
        return true;
    }
    false
}

#[cfg(not(windows))]
fn prepare_webview2_environment(_install_root: &Path) -> bool {
    true
}

fn prepare_component_environment(command: &mut Command, install_root: &Path) -> Result<(), String> {
    let layout = InstallLayout::open(install_root).map_err(|error| error.to_string())?;
    if let Some(path) = layout
        .resolve_entrypoint("org.drpa.jcode", "jcode")
        .map_err(|error| error.to_string())?
    {
        command.env("DRPA_JCODE_PATH", path);
    }
    if let Some(browser) = resolve_browser(Some(&layout)).map_err(|error| error.to_string())? {
        command.env("DRPA_BROWSER_PATH", browser.executable);
    }
    Ok(())
}

fn resolve_desktop_executable(launcher: &Path, install_root: &Path) -> Result<PathBuf, String> {
    if let Ok(layout) = InstallLayout::open(install_root)
        && let Some(path) = layout
            .resolve_entrypoint(DESKTOP_COMPONENT_ID, DESKTOP_ENTRYPOINT)
            .map_err(|error| error.to_string())?
    {
        if same_file_path(launcher, &path) {
            return Err("桌面组件入口错误地指向了 Launcher 自身".to_owned());
        }
        return Ok(path);
    }

    #[allow(unused_mut)]
    let mut candidates = vec![
        install_root.join("desktop/DRPA Desktop.exe"),
        install_root.join("desktop/drpa-desktop.exe"),
        install_root.join("drpa-desktop.exe"),
    ];
    #[cfg(not(windows))]
    candidates.extend([
        install_root.join("desktop/AppRun"),
        install_root.join("desktop/drpa-desktop"),
        install_root.join("drpa-desktop"),
    ]);
    for candidate in candidates {
        if candidate.is_file() && !same_file_path(launcher, &candidate) {
            return Ok(candidate);
        }
    }
    Err(format!(
        "未安装桌面界面组件。可运行：\n\ndrpa component install <org.drpa.desktop-ui.drpac>\n\n安装目录：{}",
        install_root.display()
    ))
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(windows)]
fn show_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let title = wide("DRPA Next");
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
    eprintln!("DRPA Next：{message}");
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use drpa_install::{ComponentManifest, InstallLayout, build_component_pack, current_platform};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn resolves_versioned_desktop_component_instead_of_launcher() {
        let temporary = TempDir::new().unwrap();
        let install = temporary.path().join("install");
        let source = temporary.path().join("desktop-source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("DRPA Desktop.exe"), b"desktop").unwrap();
        let pack = temporary.path().join("desktop.drpac");
        build_component_pack(
            &source,
            &pack,
            ComponentManifest {
                schema: 1,
                id: DESKTOP_COMPONENT_ID.to_owned(),
                version: "2.1.1".to_owned(),
                platform: current_platform().to_owned(),
                display_name: "DRPA Desktop".to_owned(),
                provides: vec!["desktop.ui".to_owned()],
                requires: BTreeMap::new(),
                entrypoints: BTreeMap::from([(
                    DESKTOP_ENTRYPOINT.to_owned(),
                    "DRPA Desktop.exe".to_owned(),
                )]),
                files: Vec::new(),
            },
        )
        .unwrap();
        let layout = InstallLayout::initialize(&install, "stable").unwrap();
        layout.install_component(pack).unwrap();
        let launcher = install.join("DRPA Next.exe");
        fs::write(&launcher, b"launcher").unwrap();

        let resolved = resolve_desktop_executable(&launcher, &install).unwrap();

        assert!(resolved.ends_with("components/org.drpa.desktop-ui/2.1.1/DRPA Desktop.exe"));
        assert_ne!(resolved, launcher);
    }

    #[test]
    fn lightweight_install_can_fall_back_to_component_installer() {
        let temporary = TempDir::new().unwrap();
        let install = temporary.path().join("install");
        let layout = InstallLayout::initialize(&install, "stable").unwrap();
        let launcher = layout.root().join("DRPA Next.exe");
        let installer = layout.root().join("DRPA Component Installer.exe");
        fs::write(&launcher, b"launcher").unwrap();
        fs::write(&installer, b"installer").unwrap();

        assert!(resolve_desktop_executable(&launcher, layout.root()).is_err());
        assert!(installer.is_file());
    }
}
