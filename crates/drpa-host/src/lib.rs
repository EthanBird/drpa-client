use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use drpa_package::{ArchiveBudget, ArchiveLimits, Entrypoint, PackageManifest, ParameterKind};
use drpa_protocol::{
    LogEntry, LogLevel, PackageSummary, ParameterSummary, RunStatus, RunSummary, RuntimeEvent,
    TaskProfile, TrustLevel, WorkspaceSnapshot, WorkspaceStats,
};
use parking_lot::RwLock;
use thiserror::Error;
use uuid::Uuid;
use zip::ZipArchive;

#[derive(Debug, Error)]
pub enum HostError {
    #[error("找不到脚本包：{0}")]
    PackageNotFound(String),
    #[error("找不到任务配置：{0}")]
    ProfileNotFound(String),
    #[error("找不到运行记录：{0}")]
    RunNotFound(String),
    #[error("无法读取脚本包：{0}")]
    Io(#[from] io::Error),
    #[error("无效的 rpaz 压缩包：{0}")]
    Archive(#[from] zip::result::ZipError),
    #[error("无效的 manifest.yaml：{0}")]
    Manifest(#[from] drpa_package::ManifestError),
    #[error("rpaz 必须在根目录包含 manifest.yaml")]
    MissingManifest,
    #[error("rpaz 包含不安全路径：{0}")]
    UnsafeArchivePath(String),
    #[error("当前版本只支持 Python 入口点")]
    UnsupportedEntrypoint,
}

#[derive(Debug, Clone)]
pub struct RunLaunch {
    pub run_id: String,
    pub package_id: String,
    pub package_dir: PathBuf,
    pub output_dir: PathBuf,
    pub entrypoint: String,
    pub callable: String,
}

pub struct HostState {
    workspace_root: PathBuf,
    snapshot: RwLock<WorkspaceSnapshot>,
    sequence: AtomicU64,
}

impl HostState {
    #[must_use]
    pub fn new(workspace_root: PathBuf) -> Self {
        let packages_root = workspace_root.join("packages");
        let _ = fs::create_dir_all(&packages_root);
        let packages = scan_installed_packages(&packages_root);
        Self {
            workspace_root,
            snapshot: RwLock::new(WorkspaceSnapshot {
                stats: WorkspaceStats {
                    active_runs: 0,
                    success_rate: 0.0,
                    packages: packages.len() as u32,
                    saved_hours: 0.0,
                },
                packages,
                runs: Vec::new(),
                logs: vec![LogEntry {
                    id: 1,
                    time: "现在".to_owned(),
                    level: LogLevel::Info,
                    scope: "host".to_owned(),
                    message: "DRPA 工作区已就绪".to_owned(),
                }],
            }),
            sequence: AtomicU64::new(2),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        self.snapshot.read().clone()
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn install_package(&self, archive_path: &Path) -> Result<PackageSummary, HostError> {
        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)?;
        validate_archive(&mut archive)?;
        let manifest = read_manifest(&mut archive)?;

        let packages_root = self.workspace_root.join("packages");
        fs::create_dir_all(&packages_root)?;
        let staging = packages_root.join(format!(".staging-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&staging)?;

        if let Err(error) = extract_archive(&mut archive, &staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }

        let package_root = packages_root.join(&manifest.id);
        fs::create_dir_all(&package_root)?;
        let target = package_root.join(&manifest.version);
        let backup = package_root.join(format!(".backup-{}", Uuid::new_v4().simple()));
        if target.exists() {
            fs::rename(&target, &backup)?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(HostError::Io(error));
        }
        if backup.exists() {
            let _ = fs::remove_dir_all(backup);
        }

        let summary = manifest_summary(&manifest);
        let mut snapshot = self.snapshot.write();
        snapshot.packages.retain(|item| item.id != summary.id);
        snapshot.packages.insert(0, summary.clone());
        snapshot.stats.packages = snapshot.packages.len() as u32;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        snapshot.logs.push(LogEntry {
            id: sequence,
            time: "现在".to_owned(),
            level: LogLevel::Success,
            scope: "package".to_owned(),
            message: format!("已安装 {} v{}", summary.name, summary.version),
        });
        Ok(summary)
    }

    pub fn uninstall_package(&self, package_id: &str) -> Result<(), HostError> {
        let package = {
            let snapshot = self.snapshot.read();
            snapshot
                .packages
                .iter()
                .find(|item| item.id == package_id)
                .cloned()
                .ok_or_else(|| HostError::PackageNotFound(package_id.to_owned()))?
        };
        let package_root = self.workspace_root.join("packages").join(package_id);
        if package_root.is_dir() {
            fs::remove_dir_all(&package_root)?;
        }

        let mut snapshot = self.snapshot.write();
        snapshot.packages.retain(|item| item.id != package_id);
        snapshot.stats.packages = snapshot.packages.len() as u32;
        self.push_log(
            &mut snapshot,
            LogLevel::Info,
            "package",
            format!("已卸载 {} v{}", package.name, package.version),
        );
        Ok(())
    }

    pub fn prepare_run(
        &self,
        package_id: &str,
        profile_id: &str,
        parameters: &serde_json::Value,
    ) -> Result<RunLaunch, HostError> {
        let mut snapshot = self.snapshot.write();
        let package = snapshot
            .packages
            .iter()
            .find(|package| package.id == package_id)
            .ok_or_else(|| HostError::PackageNotFound(package_id.to_owned()))?;
        let profile = package
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| HostError::ProfileNotFound(profile_id.to_owned()))?;

        let manifest_path = self
            .workspace_root
            .join("packages")
            .join(package_id)
            .join(&package.version)
            .join("manifest.yaml");
        let manifest = PackageManifest::from_yaml(&fs::read_to_string(manifest_path)?)?;
        let (entrypoint, callable) = match manifest.entrypoint {
            Entrypoint::Python { module, callable } => (module, callable),
            Entrypoint::Command { .. } => return Err(HostError::UnsupportedEntrypoint),
        };
        let run_id = format!("run-{}", Uuid::new_v4().simple());
        let package_dir = self
            .workspace_root
            .join("packages")
            .join(package_id)
            .join(&package.version);
        let output_dir = self
            .workspace_root
            .join("runs")
            .join(&run_id)
            .join("outputs");
        let run = RunSummary {
            id: run_id.clone(),
            package_name: package.name.clone(),
            profile_name: profile.name.clone(),
            status: RunStatus::Running,
            started_at: "刚刚".to_owned(),
            duration: "—".to_owned(),
            progress: None,
        };
        snapshot.runs.insert(0, run);
        snapshot.stats.active_runs += 1;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        snapshot.logs.push(LogEntry {
            id: sequence,
            time: "现在".to_owned(),
            level: LogLevel::Info,
            scope: "host".to_owned(),
            message: format!(
                "运行 {run_id} 已启动 · {} 个参数",
                parameters.as_object().map_or(0, |map| map.len())
            ),
        });
        Ok(RunLaunch {
            run_id,
            package_id: package_id.to_owned(),
            package_dir,
            output_dir,
            entrypoint,
            callable,
        })
    }

    pub fn prepare_development_run(
        &self,
        package_dir: &Path,
        manifest: &PackageManifest,
        parameters: &serde_json::Value,
    ) -> Result<RunLaunch, HostError> {
        let (entrypoint, callable) = match &manifest.entrypoint {
            Entrypoint::Python { module, callable } => (module.clone(), callable.clone()),
            Entrypoint::Command { .. } => return Err(HostError::UnsupportedEntrypoint),
        };
        let run_id = format!("run-{}", Uuid::new_v4().simple());
        let output_dir = self
            .workspace_root
            .join("runs")
            .join(&run_id)
            .join("outputs");
        let mut snapshot = self.snapshot.write();
        snapshot.runs.insert(
            0,
            RunSummary {
                id: run_id.clone(),
                package_name: manifest.name.clone(),
                profile_name: "Studio 直接运行".to_owned(),
                status: RunStatus::Running,
                started_at: "刚刚".to_owned(),
                duration: "—".to_owned(),
                progress: None,
            },
        );
        snapshot.stats.active_runs += 1;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        snapshot.logs.push(LogEntry {
            id: sequence,
            time: "现在".to_owned(),
            level: LogLevel::Info,
            scope: "studio".to_owned(),
            message: format!(
                "开发态运行 {run_id} 已启动 · {} 个参数",
                parameters.as_object().map_or(0, |map| map.len())
            ),
        });
        Ok(RunLaunch {
            run_id,
            package_id: manifest.id.clone(),
            package_dir: package_dir.to_owned(),
            output_dir,
            entrypoint,
            callable,
        })
    }

    pub fn record_runtime_event(&self, run_id: &str, event: RuntimeEvent) {
        let mut snapshot = self.snapshot.write();
        match event {
            RuntimeEvent::Ready { .. } => self.push_log(
                &mut snapshot,
                LogLevel::Info,
                "runtime",
                "封装 Python 运行时已就绪".to_owned(),
            ),
            RuntimeEvent::Log {
                level,
                scope,
                message,
                ..
            } => self.push_log(&mut snapshot, level, &scope, message),
            RuntimeEvent::Progress { value, message, .. } => {
                if let Some(run) = snapshot.runs.iter_mut().find(|run| run.id == run_id) {
                    run.progress = Some(value.clamp(0.0, 100.0).round() as u8);
                }
                if let Some(message) = message {
                    self.push_log(&mut snapshot, LogLevel::Info, "progress", message);
                }
            }
            RuntimeEvent::Artifact { path, label, .. } => self.push_log(
                &mut snapshot,
                LogLevel::Success,
                "artifact",
                format!("已生成 {label} · {path}"),
            ),
            RuntimeEvent::Warning { message, .. } => {
                self.push_log(&mut snapshot, LogLevel::Warning, "runtime", message);
            }
            RuntimeEvent::Error { message, .. } => {
                self.push_log(&mut snapshot, LogLevel::Error, "runtime", message);
                finish_run(&mut snapshot, run_id, RunStatus::Failed);
            }
            RuntimeEvent::Completed { exit_code, .. } => {
                let status = if exit_code == 0 {
                    RunStatus::Success
                } else {
                    RunStatus::Failed
                };
                finish_run(&mut snapshot, run_id, status);
                let (level, message) = if exit_code == 0 {
                    (LogLevel::Success, "运行完成".to_owned())
                } else {
                    (LogLevel::Error, format!("运行失败，退出码 {exit_code}"))
                };
                self.push_log(&mut snapshot, level, "runtime", message);
            }
        }
    }

    pub fn fail_run(&self, run_id: &str, message: String) {
        let mut snapshot = self.snapshot.write();
        finish_run(&mut snapshot, run_id, RunStatus::Failed);
        self.push_log(&mut snapshot, LogLevel::Error, "runtime", message);
    }

    pub fn cancel_run(&self, run_id: &str) -> Result<(), HostError> {
        let mut snapshot = self.snapshot.write();
        let run = snapshot
            .runs
            .iter_mut()
            .find(|run| run.id == run_id)
            .ok_or_else(|| HostError::RunNotFound(run_id.to_owned()))?;
        if matches!(run.status, RunStatus::Running | RunStatus::Queued) {
            run.status = RunStatus::Cancelled;
            snapshot.stats.active_runs = snapshot.stats.active_runs.saturating_sub(1);
        }
        Ok(())
    }

    fn push_log(
        &self,
        snapshot: &mut WorkspaceSnapshot,
        level: LogLevel,
        scope: &str,
        message: String,
    ) {
        snapshot.logs.push(LogEntry {
            id: self.sequence.fetch_add(1, Ordering::Relaxed),
            time: "现在".to_owned(),
            level,
            scope: scope.to_owned(),
            message,
        });
    }
}

fn finish_run(snapshot: &mut WorkspaceSnapshot, run_id: &str, status: RunStatus) {
    if let Some(run) = snapshot.runs.iter_mut().find(|run| run.id == run_id) {
        let was_active = matches!(run.status, RunStatus::Running | RunStatus::Queued);
        run.status = status;
        run.progress = Some(if matches!(status, RunStatus::Success) {
            100
        } else {
            run.progress.unwrap_or(0)
        });
        if was_active {
            snapshot.stats.active_runs = snapshot.stats.active_runs.saturating_sub(1);
        }
    }
}

fn read_manifest(archive: &mut ZipArchive<File>) -> Result<PackageManifest, HostError> {
    let mut source = String::new();
    archive
        .by_name("manifest.yaml")
        .map_err(|_| HostError::MissingManifest)?
        .read_to_string(&mut source)?;
    Ok(PackageManifest::from_yaml(&source)?)
}

fn validate_archive(archive: &mut ZipArchive<File>) -> Result<(), HostError> {
    let limits = ArchiveLimits::default();
    let mut budget = ArchiveBudget::default();
    let mut paths = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        if entry.enclosed_name().is_none() || name.contains('\\') || name.contains(':') {
            return Err(HostError::UnsafeArchivePath(name));
        }
        if !paths.insert(name.clone()) {
            return Err(HostError::UnsafeArchivePath(format!("重复路径：{name}")));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(HostError::UnsafeArchivePath(name));
        }
        if !entry.is_dir() {
            budget.observe(entry.compressed_size(), entry.size(), limits)?;
        }
    }
    Ok(())
}

fn extract_archive(archive: &mut ZipArchive<File>, staging: &Path) -> Result<(), HostError> {
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| HostError::UnsafeArchivePath(entry.name().to_owned()))?
            .to_owned();
        let output = staging.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut destination = File::create(output)?;
        io::copy(&mut entry, &mut destination)?;
    }
    Ok(())
}

fn scan_installed_packages(packages_root: &Path) -> Vec<PackageSummary> {
    let mut packages = Vec::new();
    let Ok(package_dirs) = fs::read_dir(packages_root) else {
        return packages;
    };
    for package_dir in package_dirs.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(version_dirs) = fs::read_dir(package_dir.path()) else {
            continue;
        };
        let mut manifests: Vec<_> = version_dirs
            .flatten()
            .map(|entry| entry.path().join("manifest.yaml"))
            .filter(|path| path.is_file())
            .collect();
        manifests.sort();
        if let Some(path) = manifests.pop()
            && let Ok(source) = fs::read_to_string(path)
            && let Ok(manifest) = PackageManifest::from_yaml(&source)
        {
            packages.push(manifest_summary(&manifest));
        }
    }
    packages
}

fn manifest_summary(manifest: &PackageManifest) -> PackageSummary {
    let initials: String = manifest
        .name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect();
    PackageSummary {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        description: "本地安装的自动化脚本包".to_owned(),
        version: manifest.version.clone(),
        runtime: manifest.runtime.python.as_ref().map_or_else(
            || "命令行".to_owned(),
            |version| format!("Python {version}"),
        ),
        trust: TrustLevel::Local,
        accent: "#57d6a0".to_owned(),
        initials: if initials.is_empty() {
            "RP".to_owned()
        } else {
            initials
        },
        parameters: manifest
            .parameters
            .iter()
            .map(|parameter| ParameterSummary {
                id: parameter.id.clone(),
                kind: match parameter.kind {
                    ParameterKind::String => "string",
                    ParameterKind::Number => "number",
                    ParameterKind::Boolean => "boolean",
                    ParameterKind::Secret => "secret",
                    ParameterKind::File => "file",
                    ParameterKind::Directory => "directory",
                }
                .to_owned(),
                required: parameter.required,
            })
            .collect(),
        profiles: vec![TaskProfile {
            id: "default".to_owned(),
            name: "默认配置".to_owned(),
            schedule: None,
            last_run: Some("从未运行".to_owned()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn empty_workspace_is_ready() {
        let root = std::env::temp_dir().join(format!("drpa-host-test-{}", Uuid::new_v4()));
        let state = HostState::new(root.clone());
        assert!(state.snapshot().packages.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn installs_schema_v2_rpaz_and_restores_it_on_restart() {
        let root = std::env::temp_dir().join(format!("drpa-host-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("example.rpaz");
        let file = File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file("manifest.yaml", options).unwrap();
        archive
            .write_all(
                b"schema: 2\nid: com.example.test\nname: Test\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\nruntime:\n  python: '3.11.*'\nparameters:\n  - id: market\n    type: string\n    required: false\n",
            )
            .unwrap();
        archive.start_file("main.py", options).unwrap();
        archive.write_all(b"def main(ctx): pass\n").unwrap();
        archive.finish().unwrap();

        let workspace = root.join("workspace");
        let state = HostState::new(workspace.clone());
        let installed = state.install_package(&archive_path).unwrap();
        assert_eq!(installed.id, "com.example.test");
        assert_eq!(installed.parameters.len(), 1);
        assert!(
            workspace
                .join("packages/com.example.test/0.1.0/main.py")
                .is_file()
        );

        let launch = state
            .prepare_run(
                "com.example.test",
                "default",
                &serde_json::json!({"market": "zh-CN"}),
            )
            .unwrap();
        assert_eq!(state.snapshot().stats.active_runs, 1);
        state.record_runtime_event(
            &launch.run_id,
            RuntimeEvent::Completed {
                sequence: 1,
                exit_code: 0,
            },
        );
        assert!(matches!(
            state.snapshot().runs[0].status,
            RunStatus::Success
        ));
        assert_eq!(state.snapshot().stats.active_runs, 0);

        let restored = HostState::new(workspace);
        assert_eq!(restored.snapshot().packages.len(), 1);
        restored.uninstall_package("com.example.test").unwrap();
        assert!(restored.snapshot().packages.is_empty());
        assert!(
            !restored
                .workspace_root()
                .join("packages/com.example.test")
                .exists()
        );
        let _ = fs::remove_dir_all(root);
    }
}
