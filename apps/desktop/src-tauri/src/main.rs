#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    prepare_portable_webview2();
    drpa_desktop_lib::run();
}

#[cfg(windows)]
fn prepare_portable_webview2() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let Some(parent) = executable.parent() else {
        return;
    };
    let runtime = parent.join("webview2");
    if !runtime.join("msedgewebview2.exe").is_file() {
        return;
    }

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
}

#[cfg(not(windows))]
fn prepare_portable_webview2() {}
