#![cfg(windows)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use drpa_install::{ComponentManifest, InstallLayout, build_component_pack, current_platform};
use tempfile::TempDir;

#[test]
fn real_launcher_starts_the_active_desktop_component_from_install_root() {
    let temporary = TempDir::new().unwrap();
    let install = temporary.path().join("DRPA Next");
    let source = temporary.path().join("desktop");
    fs::create_dir_all(&source).unwrap();
    let command_interpreter = env::var_os("COMSPEC").expect("COMSPEC is required on Windows");
    fs::copy(command_interpreter, source.join("desktop-test.exe")).unwrap();
    let pack = temporary.path().join("desktop.drpac");
    build_component_pack(
        &source,
        &pack,
        ComponentManifest {
            schema: 1,
            id: "org.drpa.desktop-ui".to_owned(),
            version: "2.1.1".to_owned(),
            platform: current_platform().to_owned(),
            display_name: "Desktop Test".to_owned(),
            provides: vec!["desktop.ui".to_owned()],
            requires: BTreeMap::new(),
            entrypoints: BTreeMap::from([("desktop".to_owned(), "desktop-test.exe".to_owned())]),
            files: Vec::new(),
        },
    )
    .unwrap();
    let layout = InstallLayout::initialize(&install, "stable").unwrap();
    layout.install_component(pack).unwrap();
    let launcher = install.join("DRPA Next.exe");
    fs::copy(env!("CARGO_BIN_EXE_drpa-launcher"), &launcher).unwrap();

    let output = Command::new(&launcher)
        .args(["/D", "/C", "echo launched>launcher-ok.txt"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let marker = install.join("launcher-ok.txt");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.is_file() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(fs::read_to_string(marker).unwrap().trim(), "launched");
}
