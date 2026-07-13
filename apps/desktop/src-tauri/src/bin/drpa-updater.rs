#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    version: String,
    files: Vec<UpdateFile>,
}

#[derive(Deserialize)]
struct UpdateFile {
    path: String,
}

fn main() {
    let result = apply_from_args();
    if let Err(error) = result {
        let fallback = std::env::temp_dir().join("drpa-update-error.log");
        let _ = fs::write(fallback, error);
    }
}

fn apply_from_args() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().collect();
    let stage = argument(&arguments, "--stage")?;
    let install = argument(&arguments, "--install")?;
    let launch = argument(&arguments, "--launch")?;
    let stage = PathBuf::from(stage);
    let install = PathBuf::from(install);
    let launch = safe_relative_path(launch)?;
    let source = fs::read_to_string(stage.join("update-manifest.json"))
        .map_err(|error| format!("无法读取更新清单：{error}"))?;
    let manifest: UpdateManifest =
        serde_json::from_str(&source).map_err(|error| format!("更新清单无效：{error}"))?;
    if !is_safe_version(&manifest.version) {
        return Err("更新清单版本标识无效".to_owned());
    }
    let updates_root = stage
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "更新暂存目录无效".to_owned())?;
    let backup = updates_root.join("backups").join(&manifest.version);
    fs::create_dir_all(&backup).map_err(|error| format!("无法创建更新备份：{error}"))?;
    let log_path = updates_root.join("update.log");
    log(&log_path, &format!("开始应用 {}", manifest.version));

    wait_until_replaceable(&install.join(&launch))?;
    let mut replaced: Vec<(PathBuf, PathBuf)> = Vec::new();
    for item in &manifest.files {
        let relative = safe_relative_path(&item.path)?;
        let incoming = stage.join("files").join(&relative);
        let target = install.join(&relative);
        let saved = backup.join(&relative);
        if !incoming.is_file() {
            rollback(&replaced, &log_path);
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
            .map_err(|error| format!("无法写入临时更新文件 {}：{error}", target.display()))?;
        if target.exists() {
            if saved.exists() {
                fs::remove_file(&saved).map_err(|error| error.to_string())?;
            }
            fs::rename(&target, &saved)
                .map_err(|error| format!("无法备份正在使用的文件 {}：{error}", target.display()))?;
        }
        if let Err(error) = fs::rename(&temporary, &target) {
            if saved.exists() {
                let _ = fs::rename(&saved, &target);
            }
            rollback(&replaced, &log_path);
            return Err(format!("无法替换 {}：{error}", target.display()));
        }
        replaced.push((target, saved));
    }

    log(&log_path, &format!("{} 应用成功", manifest.version));
    let _ = fs::remove_dir_all(&stage);
    Command::new(install.join(launch))
        .spawn()
        .map_err(|error| format!("更新完成但无法重启应用：{error}"))?;
    Ok(())
}

fn wait_until_replaceable(executable: &Path) -> Result<(), String> {
    for _ in 0..300 {
        if OpenOptions::new().write(true).open(executable).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err("等待主程序退出超时，更新未应用".to_owned())
}

fn rollback(replaced: &[(PathBuf, PathBuf)], log_path: &Path) {
    log(log_path, "更新失败，开始回滚");
    for (target, saved) in replaced.iter().rev() {
        let _ = fs::remove_file(target);
        if saved.exists() {
            let _ = fs::rename(saved, target);
        }
    }
}

fn safe_relative_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || value.contains('\\')
        || value.starts_with("data/")
        || value.eq_ignore_ascii_case("drpa-updater.exe")
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

fn log(path: &Path, message: &str) {
    if let Ok(mut file) = File::options().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}
