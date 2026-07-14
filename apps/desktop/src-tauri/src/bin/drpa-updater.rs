#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use drpa_protocol::{WindowsUpdatePhase, WindowsUpdateStatus};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    schema: u32,
    worker_protocol: u32,
    package_kind: String,
    version: String,
    files: Vec<UpdateFile>,
    #[serde(default)]
    remove: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateFile {
    path: String,
    bytes: u64,
}

struct UpdateContext {
    session_id: String,
    stage: PathBuf,
    install: PathBuf,
    launch: PathBuf,
    status_path: PathBuf,
    restart_request: PathBuf,
    log_path: PathBuf,
    backup: PathBuf,
    ready_path: PathBuf,
    startup_ack: PathBuf,
}

struct ReplacedFile {
    target: PathBuf,
    saved: PathBuf,
    had_original: bool,
}

fn main() {
    if let Err(error) = run() {
        let fallback = std::env::temp_dir().join("drpa-update-error.log");
        let _ = fs::write(fallback, error);
    }
}

fn run() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().collect();
    let stage = PathBuf::from(argument(&arguments, "--stage")?);
    let install = PathBuf::from(argument(&arguments, "--install")?);
    let launch = safe_relative_path(argument(&arguments, "--launch")?)?;
    let status_path = PathBuf::from(argument(&arguments, "--status")?);
    let restart_request = PathBuf::from(argument(&arguments, "--restart-request")?);
    let session_id = argument(&arguments, "--session")?.to_owned();
    let session_root = stage
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "更新会话目录无效".to_owned())?;
    let context = UpdateContext {
        session_id,
        stage,
        install: install.clone(),
        launch: launch.clone(),
        status_path,
        restart_request,
        log_path: session_root.join("update.log"),
        backup: session_root.join("backup"),
        ready_path: session_root.join("worker-ready"),
        startup_ack: session_root.join("startup-ack"),
    };

    let _ = fs::remove_file(&context.ready_path);
    let _ = fs::remove_file(&context.startup_ack);

    if let Err(error) = apply_update(&context) {
        let mut status = read_status(&context.status_path).unwrap_or_else(|| WindowsUpdateStatus {
            session_id: context.session_id.clone(),
            version: "unknown".to_owned(),
            phase: WindowsUpdatePhase::Failed,
            progress: 0,
            completed_files: 0,
            total_files: 0,
            current_file: None,
            message: String::new(),
        });
        status.phase = WindowsUpdatePhase::Failed;
        status.message = format!("更新已回滚：{error}");
        let _ = write_status(&context.status_path, &status);
        log(&context.log_path, &status.message);
        let _ = fs::remove_file(&context.ready_path);
        if context.restart_request.is_file()
            && OpenOptions::new()
                .write(true)
                .open(context.install.join(&context.launch))
                .is_ok()
        {
            let _ = installed_app_command(&context).spawn();
        }
        return Err(error);
    }
    Ok(())
}

fn apply_update(context: &UpdateContext) -> Result<(), String> {
    let source = fs::read_to_string(context.stage.join("update-manifest.json"))
        .map_err(|error| format!("读取更新清单失败：{error}"))?;
    let manifest: UpdateManifest =
        serde_json::from_str(&source).map_err(|error| format!("更新清单无效：{error}"))?;
    if manifest.schema != 2 || manifest.worker_protocol != 2 || manifest.package_kind != "delta" {
        return Err(format!(
            "更新 Worker 协议不兼容：schema={}，workerProtocol={}，packageKind={}",
            manifest.schema, manifest.worker_protocol, manifest.package_kind
        ));
    }
    if !is_safe_version(&manifest.version) {
        return Err("更新清单版本标识无效".to_owned());
    }
    validate_staged_payload(context, &manifest)?;
    fs::create_dir_all(&context.backup).map_err(|error| format!("创建更新备份失败：{error}"))?;
    log(
        &context.log_path,
        &format!("开始应用版本 {}", manifest.version),
    );

    let total_files = u32::try_from(manifest.files.len() + manifest.remove.len())
        .map_err(|_| "更新文件数量溢出".to_owned())?;
    let mut normal_files = Vec::new();
    let mut launch_file = None;
    for item in &manifest.files {
        let relative = safe_relative_path(&item.path)?;
        if relative
            .to_string_lossy()
            .eq_ignore_ascii_case(&context.launch.to_string_lossy())
        {
            launch_file = Some(relative);
        } else {
            normal_files.push(relative);
        }
    }
    let launch_file = launch_file.ok_or_else(|| "delta 更新缺少主程序文件".to_owned())?;

    let mut status = WindowsUpdateStatus {
        session_id: context.session_id.clone(),
        version: manifest.version.clone(),
        phase: WindowsUpdatePhase::WaitingForRestart,
        progress: 15,
        completed_files: 0,
        total_files,
        current_file: None,
        message: "更新 Worker 已就绪，等待应用安全退出".to_owned(),
    };
    write_status(&context.status_path, &status)?;
    fs::write(
        &context.ready_path,
        format!("session={}\n", context.session_id),
    )
    .map_err(|error| format!("写入 Worker 就绪标记失败：{error}"))?;
    wait_for_restart_request(&context.restart_request)?;
    wait_until_replaceable(&context.install.join(&context.launch))?;

    status.phase = WindowsUpdatePhase::Applying;
    status.progress = 20;
    status.message = "应用已退出，正在原子替换文件".to_owned();
    write_status(&context.status_path, &status)?;

    let mut replaced = Vec::new();
    let operation = (|| {
        for relative in normal_files {
            status.current_file = Some(relative.to_string_lossy().replace('\\', "/"));
            status.message = format!("正在更新 {}", relative.display());
            write_status(&context.status_path, &status)?;
            replace_file(context, &relative, &mut replaced)?;
            advance_status(&mut status);
            write_status(&context.status_path, &status)?;
        }

        for value in &manifest.remove {
            let relative = safe_relative_path(value)?;
            status.current_file = Some(relative.to_string_lossy().replace('\\', "/"));
            status.message = format!("正在移除旧文件 {}", relative.display());
            write_status(&context.status_path, &status)?;
            remove_file(context, &relative, &mut replaced)?;
            advance_status(&mut status);
            write_status(&context.status_path, &status)?;
        }

        status.current_file = Some(launch_file.to_string_lossy().replace('\\', "/"));
        status.message = "正在替换主程序".to_owned();
        write_status(&context.status_path, &status)?;
        replace_file(context, &launch_file, &mut replaced)?;
        advance_status(&mut status);
        status.phase = WindowsUpdatePhase::Restarting;
        status.progress = 96;
        status.message = "文件替换完成，正在等待新版本确认启动".to_owned();
        write_status(&context.status_path, &status)?;
        let _ = fs::remove_file(&context.startup_ack);
        let mut child = updated_app_command(context)
            .spawn()
            .map_err(|error| format!("启动更新后的应用失败：{error}"))?;
        wait_for_startup_ack(&mut child, &context.startup_ack, Duration::from_secs(30))?;

        status.phase = WindowsUpdatePhase::Completed;
        status.progress = 100;
        status.current_file = None;
        status.message = format!("版本 {} 更新完成", manifest.version);
        write_status(&context.status_path, &status)?;
        log(&context.log_path, &status.message);
        let _ = fs::remove_file(&context.ready_path);
        let _ = fs::remove_dir_all(&context.stage);
        let _ = fs::remove_dir_all(&context.backup);
        Ok(())
    })();

    if let Err(error) = operation {
        rollback(&replaced, &context.log_path);
        return Err(error);
    }
    Ok(())
}

fn validate_staged_payload(
    context: &UpdateContext,
    manifest: &UpdateManifest,
) -> Result<(), String> {
    let mut paths = HashSet::new();
    for item in &manifest.files {
        let relative = safe_relative_path(&item.path)?;
        let normalized = relative.to_string_lossy().to_ascii_lowercase();
        if !paths.insert(normalized) {
            return Err(format!("更新清单包含重复路径：{}", item.path));
        }
        let incoming = context.stage.join("files").join(&relative);
        let metadata = fs::metadata(&incoming)
            .map_err(|error| format!("更新文件缺失 {}：{error}", item.path))?;
        if !metadata.is_file() || metadata.len() != item.bytes {
            return Err(format!("更新文件大小不匹配：{}", item.path));
        }
    }
    for value in &manifest.remove {
        let relative = safe_relative_path(value)?;
        let normalized = relative.to_string_lossy().to_ascii_lowercase();
        if paths.contains(&normalized) {
            return Err(format!("更新清单的替换与删除路径冲突：{value}"));
        }
    }
    Ok(())
}

fn replace_file(
    context: &UpdateContext,
    relative: &Path,
    replaced: &mut Vec<ReplacedFile>,
) -> Result<(), String> {
    let incoming = context.stage.join("files").join(relative);
    let target = context.install.join(relative);
    let saved = context.backup.join(relative);
    if !incoming.is_file() {
        return Err(format!("更新文件缺失：{}", relative.display()));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if let Some(parent) = saved.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = target.with_extension("drpa-new");
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    fs::copy(&incoming, &temporary)
        .map_err(|error| format!("写入临时更新文件失败 {}：{error}", target.display()))?;
    let had_original = target.exists();
    if had_original {
        rename_with_retry(&target, &saved)
            .map_err(|error| format!("备份当前文件失败 {}：{error}", target.display()))?;
    }
    if let Err(error) = rename_with_retry(&temporary, &target) {
        if had_original && saved.exists() {
            let _ = fs::rename(&saved, &target);
        }
        return Err(format!("替换文件失败 {}：{error}", target.display()));
    }
    replaced.push(ReplacedFile {
        target,
        saved,
        had_original,
    });
    Ok(())
}

fn remove_file(
    context: &UpdateContext,
    relative: &Path,
    replaced: &mut Vec<ReplacedFile>,
) -> Result<(), String> {
    let target = context.install.join(relative);
    if !target.exists() {
        return Ok(());
    }
    let saved = context.backup.join(relative);
    if let Some(parent) = saved.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    rename_with_retry(&target, &saved)
        .map_err(|error| format!("备份待移除文件失败 {}：{error}", target.display()))?;
    replaced.push(ReplacedFile {
        target,
        saved,
        had_original: true,
    });
    Ok(())
}

fn rename_with_retry(source: &Path, target: &Path) -> Result<(), std::io::Error> {
    let mut last_error = None;
    for _ in 0..100 {
        match fs::rename(source, target) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("rename retry exhausted")))
}

fn wait_until_replaceable(executable: &Path) -> Result<(), String> {
    for _ in 0..300 {
        if OpenOptions::new().write(true).open(executable).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("等待主程序退出超时，更新已回滚".to_owned())
}

fn wait_for_restart_request(path: &Path) -> Result<(), String> {
    for _ in 0..6_000 {
        if path.is_file() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("等待应用确认重启超时，更新已回滚".to_owned())
}

fn installed_app_command(context: &UpdateContext) -> Command {
    let mut command = Command::new(context.install.join(&context.launch));
    command
        .current_dir(&context.install)
        .env_remove("DRPA_UPDATE_SESSION_ID");
    command
}

fn updated_app_command(context: &UpdateContext) -> Command {
    let mut command = installed_app_command(context);
    command.env("DRPA_UPDATE_SESSION_ID", &context.session_id);
    command
}

fn wait_for_startup_ack(
    child: &mut Child,
    startup_ack: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if startup_ack.is_file() {
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("检查新版本启动状态失败：{error}"))?
        {
            return Err(format!("新版本未确认启动便退出，退出状态：{status}"));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("新版本未在 30 秒内确认主窗口启动".to_owned());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn advance_status(status: &mut WindowsUpdateStatus) {
    status.completed_files = status.completed_files.saturating_add(1);
    if let Some(ratio) = status
        .completed_files
        .saturating_mul(70)
        .checked_div(status.total_files)
    {
        status.progress = u8::try_from(20 + ratio).unwrap_or(90).min(90);
    }
}

fn rollback(replaced: &[ReplacedFile], log_path: &Path) {
    log(log_path, "更新失败，正在恢复原文件");
    for item in replaced.iter().rev() {
        let _ = fs::remove_file(&item.target);
        if item.had_original && item.saved.exists() {
            let _ = fs::rename(&item.saved, &item.target);
        }
    }
}

fn safe_relative_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    let folded = value.to_ascii_lowercase();
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || value.contains('\\')
        || folded.starts_with("data/")
        || folded.starts_with("webview2/")
    {
        return Err(format!("更新清单包含不安全路径：{value}"));
    }
    Ok(path.to_owned())
}

fn is_safe_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 80
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn argument<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| format!("缺少参数 {name}"))
}

fn read_status(path: &Path) -> Option<WindowsUpdateStatus> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_status(path: &Path, status: &WindowsUpdateStatus) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(status).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn log(path: &Path, message: &str) {
    if let Ok(mut file) = File::options().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(root: &Path) -> UpdateContext {
        UpdateContext {
            session_id: "0123456789abcdef0123456789abcdef".to_owned(),
            stage: root.join("session/stage"),
            install: root.join("install"),
            launch: PathBuf::from("DRPA Next.exe"),
            status_path: root.join("session/status.json"),
            restart_request: root.join("session/restart-requested"),
            log_path: root.join("session/update.log"),
            backup: root.join("session/backup"),
            ready_path: root.join("session/worker-ready"),
            startup_ack: root.join("session/startup-ack"),
        }
    }

    #[test]
    fn validates_staged_payload_size_without_hashing() {
        let root = std::env::temp_dir().join(format!("drpa-updater-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let context = context(&root);
        fs::create_dir_all(context.stage.join("files/examples")).unwrap();
        fs::write(context.stage.join("files/examples/new.txt"), b"new").unwrap();
        let manifest = UpdateManifest {
            schema: 2,
            worker_protocol: 2,
            package_kind: "delta".to_owned(),
            version: "test-1".to_owned(),
            files: vec![UpdateFile {
                path: "examples/new.txt".to_owned(),
                bytes: 3,
            }],
            remove: vec![],
        };

        validate_staged_payload(&context, &manifest).unwrap();
        fs::write(context.stage.join("files/examples/new.txt"), b"wrong").unwrap();
        assert!(
            validate_staged_payload(&context, &manifest)
                .unwrap_err()
                .contains("大小不匹配")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rolls_back_files_when_a_later_replacement_fails() {
        let root =
            std::env::temp_dir().join(format!("drpa-updater-rollback-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let context = context(&root);
        fs::create_dir_all(context.stage.join("files")).unwrap();
        fs::create_dir_all(&context.install).unwrap();
        fs::write(context.stage.join("files/first.txt"), b"new").unwrap();
        fs::write(context.install.join("first.txt"), b"old").unwrap();
        let mut replaced = Vec::new();

        replace_file(&context, Path::new("first.txt"), &mut replaced).unwrap();
        assert!(replace_file(&context, Path::new("missing.txt"), &mut replaced).is_err());
        rollback(&replaced, &context.log_path);

        assert_eq!(fs::read(context.install.join("first.txt")).unwrap(), b"old");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relaunch_command_uses_the_install_directory() {
        let root = std::env::temp_dir().join(format!(
            "drpa-updater-launch-command-test-{}",
            std::process::id()
        ));
        let install = root.join("install");
        let context = context(&root);

        let command = installed_app_command(&context);

        assert_eq!(command.get_current_dir(), Some(install.as_path()));
    }

    #[test]
    fn updated_app_receives_the_startup_ack_session() {
        let root = std::env::temp_dir().join(format!(
            "drpa-updater-launch-environment-test-{}",
            std::process::id()
        ));
        let context = context(&root);
        let command = updated_app_command(&context);
        let session = command
            .get_envs()
            .find(|(name, _)| *name == "DRPA_UPDATE_SESSION_ID")
            .and_then(|(_, value)| value)
            .unwrap();

        assert_eq!(session, context.session_id.as_str());
    }

    #[test]
    fn early_child_exit_without_startup_ack_is_reported() {
        #[cfg(windows)]
        let mut child = Command::new("cmd")
            .args(["/C", "exit", "7"])
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let mut child = Command::new("sh").args(["-c", "exit 7"]).spawn().unwrap();

        let ack = std::env::temp_dir().join(format!(
            "drpa-updater-missing-startup-ack-{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&ack);
        let error = wait_for_startup_ack(&mut child, &ack, Duration::from_millis(250)).unwrap_err();

        assert!(error.contains('7'));
    }

    #[test]
    fn old_update_schema_is_rejected_before_restart() {
        let root = std::env::temp_dir().join(format!(
            "drpa-updater-old-schema-test-{}",
            std::process::id()
        ));
        let context = context(&root);
        fs::create_dir_all(&context.stage).unwrap();
        fs::write(
            context.stage.join("update-manifest.json"),
            r#"{"schema":1,"workerProtocol":1,"packageKind":"delta","version":"old","files":[],"remove":[]}"#,
        )
        .unwrap();

        assert!(apply_update(&context).unwrap_err().contains("协议不兼容"));
        assert!(!context.restart_request.exists());
        assert!(!context.ready_path.exists());
        let _ = fs::remove_dir_all(root);
    }
}
