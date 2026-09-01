use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn command(temporary: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_drpa"));
    command.env("DRPA_LOCATOR_HOME", temporary.path().join("locator"));
    command
}

#[test]
fn initializes_registry_free_install_and_manages_component_end_to_end() {
    let temporary = TempDir::new().unwrap();
    let install = temporary.path().join("DRPA Next");
    let source = temporary.path().join("source");
    let pack = temporary.path().join("test.drpac");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tool.txt"), b"hello component").unwrap();

    let initialized = command(&temporary)
        .args(["install", "init"])
        .arg(&install)
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    assert!(install.join(".drpa-install.json").is_file());

    let packed = command(&temporary)
        .args(["component", "pack"])
        .arg(&source)
        .arg(&pack)
        .args([
            "--id",
            "org.drpa.cli-test",
            "--version",
            "1.0.0",
            "--name",
            "CLI Test",
            "--provide",
            "test.cli",
            "--entry",
            "main=tool.txt",
        ])
        .output()
        .unwrap();
    assert!(
        packed.status.success(),
        "{}",
        String::from_utf8_lossy(&packed.stderr)
    );

    let installed = command(&temporary)
        .arg("--install-root")
        .arg(&install)
        .args(["component", "install"])
        .arg(&pack)
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );

    let verified = command(&temporary)
        .arg("--install-root")
        .arg(&install)
        .args(["component", "verify", "org.drpa.cli-test"])
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );

    let status = command(&temporary)
        .arg("--install-root")
        .arg(&install)
        .args(["--json", "status"])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(
        json["components"]["active"]["org.drpa.cli-test"]["version"],
        "1.0.0"
    );
}

#[test]
fn exposes_json_lines_core_protocol_over_stdio() {
    let temporary = TempDir::new().unwrap();
    let mut child = command(&temporary)
        .arg("--data-root")
        .arg(temporary.path().join("data"))
        .args(["serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(
            concat!(
                "{\"protocol\":1,\"id\":\"ping\",\"method\":\"ping\"}\n",
                "{\"protocol\":1,\"id\":\"stop\",\"method\":\"shutdown\"}\n"
            )
            .as_bytes(),
        )
        .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], "ping");
    assert_eq!(responses[0]["result"]["protocol"], 1);
    assert_eq!(responses[1]["result"]["stopped"], true);
}
