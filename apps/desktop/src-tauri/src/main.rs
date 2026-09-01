#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if prepare_webview2() {
        drpa_desktop_lib::run();
    }
}

#[cfg(windows)]
fn prepare_webview2() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(parent) = drpa_desktop_lib::installation_root_from_process() else {
        return drpa_install::system_webview2_version().is_some();
    };
    let layout = drpa_install::InstallLayout::open(&parent).ok();
    let runtime = drpa_install::resolve_fixed_webview2(layout.as_ref())
        .ok()
        .flatten()
        .or_else(|| {
            let legacy = parent.join("webview2");
            legacy
                .join("msedgewebview2.exe")
                .is_file()
                .then_some(legacy)
        });
    let Some(runtime) = runtime else {
        if drpa_install::system_webview2_version().is_some() {
            // A stale inherited fixed-runtime override must not prevent
            // WebView2Loader from selecting the registered Evergreen runtime.
            unsafe {
                std::env::remove_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER");
            }
            return true;
        }
        let installer = parent.join("DRPA Component Installer.exe");
        if installer.is_file() {
            let _ = Command::new(installer)
                .arg("--install-root")
                .arg(&parent)
                .args(["--required", "org.drpa.webview2-fixed"])
                .current_dir(&parent)
                .spawn();
        }
        return false;
    };

    // This runs before Tauri or any worker thread starts. Process-scoped
    // configuration selects the bundled runtime without registry lookup.
    // The WebView user-data folder is set through Tauri's supported
    // WebviewWindowBuilder API in lib.rs.
    unsafe {
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &runtime);
    }

    // Windows 10 requires AppContainer read/execute ACLs for Fixed Version
    // WebView2 120+. icacls changes only this portable folder, never registry.
    let _ = Command::new("icacls.exe")
        .arg(&runtime)
        .args([
            "/grant",
            "*S-1-15-2-2:(OI)(CI)(RX)",
            "/grant",
            "*S-1-15-2-1:(OI)(CI)(RX)",
            "/T",
            "/C",
            "/Q",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    true
}

#[cfg(not(windows))]
fn prepare_webview2() -> bool {
    true
}
