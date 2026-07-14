#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use drpa_protocol::{WindowsUpdatePhase, WindowsUpdateStatus};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    version: String,
    files: Vec<UpdateFile>,
    #[serde(default)]
    remove: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateFile {
    path: String,
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
    };

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
        if context.restart_request.is_file() {
            let _ = Command::new(install.join(launch)).spawn();
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
    if !is_safe_version(&manifest.version) {
        return Err("更新清单版本标识无效".to_owned());
    }
    fs::create_dir_all(&context.backup).map_err(|error| format!("创建更新备份失败：{error}"))?;
    log(
        &context.log_path,
        &format!("开始应用版本 {}", manifest.version),
    );

    let total_files = u32::try_from(manifest.files.len() + manifest.remove.len())
        .map_err(|_| "更新文件数量溢出".to_owned())?;
    let mut status = WindowsUpdateStatus {
        session_id: context.session_id.clone(),
        version: manifest.version.clone(),
        phase: WindowsUpdatePhase::Applying,
        progress: 5,
        completed_files: 0,
        total_files,
        current_file: None,
        message: "正在替换应用文件".to_owned(),
    };
    write_status(&context.status_path, &status)?;

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

        if let Some(relative) = launch_file {
            status.phase = WindowsUpdatePhase::WaitingForRestart;
            status.progress = 92;
            status.current_file = Some(relative.to_string_lossy().replace('\\', "/"));
            status.message = "文件已准备完成，正在安全重启应用".to_owned();
            write_status(&context.status_path, &status)?;
            wait_for_restart_request(&context.restart_request)?;
            wait_until_replaceable(&context.install.join(&context.launch))?;
            replace_file(context, &relative, &mut replaced)?;
            advance_status(&mut status);
            status.phase = WindowsUpdatePhase::Restarting;
            status.progress = 98;
            status.message = "应用文件替换完成，正在启动新版本".to_owned();
            write_status(&context.status_path, &status)?;
            Command::new(context.install.join(&context.launch))
                .spawn()
                .map_err(|error| format!("启动更新后的应用失败：{error}"))?;
        }

        status.phase = WindowsUpdatePhase::Completed;
        status.progress = 100;
        status.current_file = None;
        status.message = format!("版本 {} 更新完成", manifest.version);
        write_status(&context.status_path, &status)?;
        log(&context.log_path, &status.message);
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

fn advance_status(status: &mut WindowsUpdateStatus) {
    status.completed_files = status.completed_files.saturating_add(1);
    if let Some(ratio) = status
        .completed_files
        .saturating_mul(85)
        .checked_div(status.total_files)
    {
        status.progress = u8::try_from(5 + ratio).unwrap_or(90).min(90);
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

    #[test]
    fn applies_non_executable_files_without_restarting() {
        let root = std::env::temp_dir().join(format!("drpa-updater-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let stage = root.join("session/stage");
        let install = root.join("install");
        fs::create_dir_all(stage.join("files/examples")).unwrap();
        fs::create_dir_all(install.join("examples")).unwrap();
        fs::write(stage.join("files/examples/new.txt"), b"new").unwrap();
        fs::write(install.join("examples/new.txt"), b"old").unwrap();
        fs::write(install.join("obsolete.txt"), b"obsolete").unwrap();
        fs::write(
            stage.join("update-manifest.json"),
            r#"{"version":"test-1","files":[{"path":"examples/new.txt"}],"remove":["obsolete.txt"]}"#,
        )
        .unwrap();
        let context = UpdateContext {
            session_id: "test-session".to_owned(),
            stage: stage.clone(),
            install: install.clone(),
            launch: PathBuf::from("DRPA Next.exe"),
            status_path: root.join("session/status.json"),
            restart_request: root.join("session/restart-requested"),
            log_path: root.join("session/update.log"),
            backup: root.join("session/backup"),
        };

        apply_update(&context).unwrap();

        assert_eq!(fs::read(install.join("examples/new.txt")).unwrap(), b"new");
        assert!(!install.join("obsolete.txt").exists());
        let status = read_status(&context.status_path).unwrap();
        assert_eq!(status.phase, WindowsUpdatePhase::Completed);
        assert_eq!(status.progress, 100);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rolls_back_files_when_a_later_replacement_fails() {
        let root =
            std::env::temp_dir().join(format!("drpa-updater-rollback-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let stage = root.join("session/stage");
        let install = root.join("install");
        fs::create_dir_all(stage.join("files")).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::write(stage.join("files/first.txt"), b"new").unwrap();
        fs::write(install.join("first.txt"), b"old").unwrap();
        fs::write(
            stage.join("update-manifest.json"),
            r#"{"version":"test-rollback","files":[{"path":"first.txt"},{"path":"missing.txt"}],"remove":[]}"#,
        )
        .unwrap();
        let context = UpdateContext {
            session_id: "rollback-session".to_owned(),
            stage: stage.clone(),
            install: install.clone(),
            launch: PathBuf::from("DRPA Next.exe"),
            status_path: root.join("session/status.json"),
            restart_request: root.join("session/restart-requested"),
            log_path: root.join("session/update.log"),
            backup: root.join("session/backup"),
        };

        assert!(apply_update(&context).is_err());
        assert_eq!(fs::read(install.join("first.txt")).unwrap(), b"old");
        let _ = fs::remove_dir_all(root);
    }
}
