use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use drpa_package::{ArchiveBudget, ArchiveLimits, Entrypoint, PackageManifest, ParameterKind};
use drpa_protocol::{
    AutomationStatus, AutomationSummary, ConcurrencyPolicy, LogEntry, LogLevel, PackageSummary,
    ParameterSummary, RunStatus, RunSummary, RuntimeEvent, TaskProfile, TrustLevel,
    WorkspaceSnapshot, WorkspaceStats,
};
use parking_lot::RwLock;
use thiserror::Error;
use uuid::Uuid;
use zip::ZipArchive;

mod run_store;

use run_store::{RunStore, now_timestamp};

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
    #[error("运行记录存储失败：{0}")]
    Storage(String),
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

#[derive(Clone)]
pub struct HostState {
    workspace_root: PathBuf,
    snapshot: Arc<RwLock<WorkspaceSnapshot>>,
    sequence: Arc<AtomicU64>,
    run_store: Arc<RunStore>,
}

impl HostState {
    #[must_use]
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::try_new(workspace_root).expect("failed to initialize DRPA host state")
    }

    pub fn try_new(workspace_root: PathBuf) -> Result<Self, HostError> {
        let packages_root = workspace_root.join("packages");
        fs::create_dir_all(&packages_root)?;
        let packages = scan_installed_packages(&packages_root);
        let automations = derive_automations(&packages);
        let run_store = Arc::new(RunStore::open(&workspace_root).map_err(HostError::Storage)?);
        run_store
            .mark_unfinished_interrupted()
            .map_err(HostError::Storage)?;
        let runs = run_store.load_summaries().map_err(HostError::Storage)?;
        let completed = runs
            .iter()
            .filter(|run| matches!(run.status, RunStatus::Success | RunStatus::Failed))
            .count();
        let succeeded = runs
            .iter()
            .filter(|run| run.status == RunStatus::Success)
            .count();
        Ok(Self {
            workspace_root,
            snapshot: Arc::new(RwLock::new(WorkspaceSnapshot {
                stats: WorkspaceStats {
                    active_runs: 0,
                    success_rate: if completed == 0 {
                        0.0
                    } else {
                        succeeded as f64 / completed as f64 * 100.0
                    },
                    packages: packages.len() as u32,
                    saved_hours: 0.0,
                },
                packages,
                automations,
                runs,
                logs: vec![LogEntry {
                    id: 1,
                    time: log_timestamp(),
                    level: LogLevel::Info,
                    scope: "host".to_owned(),
                    message: "Host 初始化完成；工作区状态与本地脚本包索引已载入".to_owned(),
                }],
            })),
            sequence: Arc::new(AtomicU64::new(2)),
            run_store,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        self.snapshot.read().clone()
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn get_run_detail(&self, run_id: &str) -> Result<drpa_protocol::RunDetail, HostError> {
        self.run_store
            .get_detail(run_id)
            .map_err(HostError::Storage)?
            .ok_or_else(|| HostError::RunNotFound(run_id.to_owned()))
    }

    pub fn delete_run_record(&self, run_id: &str) -> Result<Option<String>, HostError> {
        if self.run_is_active(run_id) {
            return Err(HostError::Storage("运行中的记录不能删除".to_owned()));
        }
        let output_dir = self
            .run_store
            .delete_run(run_id)
            .map_err(HostError::Storage)?;
        self.snapshot.write().runs.retain(|run| run.id != run_id);
        Ok(output_dir)
    }

    fn run_is_active(&self, run_id: &str) -> bool {
        self.snapshot.read().runs.iter().any(|run| {
            run.id == run_id && matches!(run.status, RunStatus::Running | RunStatus::Queued)
        })
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
        snapshot.automations = derive_automations(&snapshot.packages);
        snapshot.stats.packages = snapshot.packages.len() as u32;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        snapshot.logs.push(LogEntry {
            id: sequence,
            time: log_timestamp(),
            level: LogLevel::Success,
            scope: "package".to_owned(),
            message: format!(
                "脚本包安装完成 · package={} · version={}",
                summary.id, summary.version
            ),
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
        snapshot.automations = derive_automations(&snapshot.packages);
        snapshot.stats.packages = snapshot.packages.len() as u32;
        self.push_log(
            &mut snapshot,
            LogLevel::Info,
            "package",
            format!(
                "脚本包卸载完成 · package={} · version={}",
                package.id, package.version
            ),
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
            package_id: package.id.clone(),
            package_name: package.name.clone(),
            package_version: package.version.clone(),
            profile_id: profile.id.clone(),
            profile_name: profile.name.clone(),
            status: RunStatus::Running,
            started_at: now_timestamp(),
            finished_at: None,
            duration: "—".to_owned(),
            duration_ms: None,
            progress: None,
            exit_code: None,
        };
        let redacted_parameters = redact_parameters(parameters, &package.parameters);
        self.run_store
            .create_run(&run, &redacted_parameters, &output_dir, "workbench")
            .map_err(HostError::Storage)?;
        snapshot.runs.insert(0, run);
        snapshot.stats.active_runs += 1;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        snapshot.logs.push(LogEntry {
            id: sequence,
            time: log_timestamp(),
            level: LogLevel::Info,
            scope: "host".to_owned(),
            message: format!(
                "任务已创建 · run={run_id} · package={package_id} · profile={profile_id} · parameters={}",
                parameters.as_object().map_or(0, |map| map.len()),
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
        let run = RunSummary {
            id: run_id.clone(),
            package_id: manifest.id.clone(),
            package_name: manifest.name.clone(),
            package_version: manifest.version.clone(),
            profile_id: "studio".to_owned(),
            profile_name: "Studio 直接运行".to_owned(),
            status: RunStatus::Running,
            started_at: now_timestamp(),
            finished_at: None,
            duration: "—".to_owned(),
            duration_ms: None,
            progress: None,
            exit_code: None,
        };
        self.run_store
            .create_run(
                &run,
                &redact_parameters_by_name(parameters),
                &output_dir,
                "studio",
            )
            .map_err(HostError::Storage)?;
        snapshot.runs.insert(0, run);
        snapshot.stats.active_runs += 1;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        snapshot.logs.push(LogEntry {
            id: sequence,
            time: log_timestamp(),
            level: LogLevel::Info,
            scope: "studio".to_owned(),
            message: format!(
                "Studio 任务已创建 · run={run_id} · package={} · parameters={}",
                manifest.id,
                parameters.as_object().map_or(0, |map| map.len()),
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
        let _ = self.run_store.append_runtime_event(run_id, &event);
        let mut snapshot = self.snapshot.write();
        match event {
            RuntimeEvent::Ready { .. } => self.push_log(
                &mut snapshot,
                LogLevel::Info,
                "runtime",
                format!("运行时握手完成 · run={run_id} · protocol=drpa-runtime-v1"),
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
                format!("产物已登记 · run={run_id} · label={label} · path={path}"),
            ),
            RuntimeEvent::OpenDirectory { path, .. } => self.push_log(
                &mut snapshot,
                LogLevel::Info,
                "workspace",
                format!("已请求打开输出目录 · run={run_id} · path={path}"),
            ),
            RuntimeEvent::Warning { message, .. } => {
                self.push_log(&mut snapshot, LogLevel::Warning, "runtime", message);
            }
            RuntimeEvent::Error {
                message, traceback, ..
            } => {
                self.push_log(&mut snapshot, LogLevel::Error, "runtime", message);
                self.finish_run(
                    &mut snapshot,
                    run_id,
                    RunStatus::Failed,
                    Some(1),
                    None,
                    traceback.as_deref(),
                );
            }
            RuntimeEvent::Completed { exit_code, .. } => {
                let status = if exit_code == 0 {
                    RunStatus::Success
                } else {
                    RunStatus::Failed
                };
                self.finish_run(&mut snapshot, run_id, status, Some(exit_code), None, None);
                let (level, message) = if exit_code == 0 {
                    (
                        LogLevel::Success,
                        format!("任务完成 · run={run_id} · exitCode=0"),
                    )
                } else {
                    (
                        LogLevel::Error,
                        format!("任务失败 · run={run_id} · exitCode={exit_code}"),
                    )
                };
                self.push_log(&mut snapshot, level, "runtime", message);
            }
        }
    }

    pub fn fail_run(&self, run_id: &str, message: String) {
        let mut snapshot = self.snapshot.write();
        let _ = self.run_store.append_host_event(
            run_id,
            LogLevel::Error,
            "host",
            &message,
            serde_json::json!({"failure": "host"}),
        );
        self.finish_run(
            &mut snapshot,
            run_id,
            RunStatus::Failed,
            Some(1),
            Some(&message),
            None,
        );
        self.push_log(&mut snapshot, LogLevel::Error, "runtime", message);
    }

    pub fn cancel_run(&self, run_id: &str) -> Result<(), HostError> {
        let mut snapshot = self.snapshot.write();
        let run_index = snapshot
            .runs
            .iter()
            .position(|run| run.id == run_id)
            .ok_or_else(|| HostError::RunNotFound(run_id.to_owned()))?;
        if matches!(
            snapshot.runs[run_index].status,
            RunStatus::Running | RunStatus::Queued
        ) {
            let _ = self.run_store.append_host_event(
                run_id,
                LogLevel::Warning,
                "host",
                "用户取消任务",
                serde_json::json!({"reason": "user_cancelled"}),
            );
            snapshot.runs[run_index].status = RunStatus::Cancelled;
            snapshot.stats.active_runs = snapshot.stats.active_runs.saturating_sub(1);
            if let Ok((finished_at, duration_ms)) = self.run_store.finish_run(
                run_id,
                RunStatus::Cancelled,
                Some(130),
                Some("用户取消任务"),
                None,
            ) {
                let run = &mut snapshot.runs[run_index];
                run.finished_at = Some(finished_at);
                run.duration_ms = Some(duration_ms);
                run.duration = format_duration(duration_ms);
                run.exit_code = Some(130);
            }
        }
        Ok(())
    }

    pub fn run_is_cancelled(&self, run_id: &str) -> bool {
        self.snapshot
            .read()
            .runs
            .iter()
            .any(|run| run.id == run_id && matches!(&run.status, RunStatus::Cancelled))
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
            time: log_timestamp(),
            level,
            scope: scope.to_owned(),
            message,
        });
    }

    fn finish_run(
        &self,
        snapshot: &mut WorkspaceSnapshot,
        run_id: &str,
        status: RunStatus,
        exit_code: Option<i32>,
        error_message: Option<&str>,
        error_traceback: Option<&str>,
    ) {
        let persisted = self
            .run_store
            .finish_run(run_id, status, exit_code, error_message, error_traceback)
            .ok();
        finish_run(snapshot, run_id, status, exit_code, persisted);
        refresh_run_stats(snapshot);
    }
}

fn log_timestamp() -> String {
    let milliseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() % 86_400_000);
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds / 60_000) % 60;
    let seconds = (milliseconds / 1_000) % 60;
    let fraction = milliseconds % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{fraction:03}Z")
}

fn finish_run(
    snapshot: &mut WorkspaceSnapshot,
    run_id: &str,
    status: RunStatus,
    exit_code: Option<i32>,
    persisted: Option<(String, u64)>,
) {
    if let Some(run) = snapshot.runs.iter_mut().find(|run| run.id == run_id) {
        let was_active = matches!(run.status, RunStatus::Running | RunStatus::Queued);
        run.status = status;
        run.progress = Some(if matches!(status, RunStatus::Success) {
            100
        } else {
            run.progress.unwrap_or(0)
        });
        run.exit_code = exit_code.or(run.exit_code);
        if let Some((finished_at, duration_ms)) = persisted {
            run.finished_at = Some(finished_at);
            run.duration_ms = Some(duration_ms);
            run.duration = format_duration(duration_ms);
        }
        if was_active {
            snapshot.stats.active_runs = snapshot.stats.active_runs.saturating_sub(1);
        }
    }
}

fn format_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms} ms")
    } else if duration_ms < 60_000 {
        format!("{:.1} s", duration_ms as f64 / 1_000.0)
    } else {
        format!(
            "{}m {:02}s",
            duration_ms / 60_000,
            (duration_ms % 60_000) / 1_000
        )
    }
}

fn refresh_run_stats(snapshot: &mut WorkspaceSnapshot) {
    let completed = snapshot
        .runs
        .iter()
        .filter(|run| matches!(run.status, RunStatus::Success | RunStatus::Failed))
        .count();
    let succeeded = snapshot
        .runs
        .iter()
        .filter(|run| run.status == RunStatus::Success)
        .count();
    snapshot.stats.success_rate = if completed == 0 {
        0.0
    } else {
        succeeded as f64 / completed as f64 * 100.0
    };
}

fn redact_parameters(
    parameters: &serde_json::Value,
    definitions: &[ParameterSummary],
) -> serde_json::Value {
    let mut redacted = parameters.clone();
    let Some(values) = redacted.as_object_mut() else {
        return serde_json::json!({});
    };
    for definition in definitions {
        if definition.kind == "secret" && values.contains_key(&definition.id) {
            values.insert(
                definition.id.clone(),
                serde_json::Value::String("***".to_owned()),
            );
        }
    }
    redacted
}

fn redact_parameters_by_name(parameters: &serde_json::Value) -> serde_json::Value {
    let mut redacted = parameters.clone();
    let Some(values) = redacted.as_object_mut() else {
        return serde_json::json!({});
    };
    for (name, value) in values.iter_mut() {
        let lower = name.to_ascii_lowercase();
        if ["password", "secret", "token", "api_key", "apikey"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            *value = serde_json::Value::String("***".to_owned());
        }
    }
    redacted
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

fn derive_automations(packages: &[PackageSummary]) -> Vec<AutomationSummary> {
    packages
        .iter()
        .flat_map(|package| {
            package
                .profiles
                .iter()
                .filter_map(|profile| {
                    let schedule = profile.schedule.as_ref()?;
                    Some(AutomationSummary {
                        id: format!("{}::{}", package.id, profile.id),
                        name: format!("{} / {}", package.name, profile.name),
                        package_name: package.name.clone(),
                        profile_name: profile.name.clone(),
                        trigger_label: schedule.clone(),
                        next_run: "等待调度器计算".to_owned(),
                        last_run: profile.last_run.clone(),
                        health: "来自脚本包配置的计划；Host Scheduler 尚未接管".to_owned(),
                        status: AutomationStatus::Paused,
                        concurrency_policy: ConcurrencyPolicy::Forbid,
                        retry_policy: "未配置重试策略".to_owned(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
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
                default_value: parameter.default.clone(),
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
    fn cloned_host_state_shares_live_run_snapshots() {
        let root = std::env::temp_dir().join(format!("drpa-host-clone-test-{}", Uuid::new_v4()));
        let state = HostState::new(root.clone());
        let background = state.clone();
        background.fail_run("missing-run", "background marker".to_owned());

        assert!(
            state
                .snapshot()
                .logs
                .iter()
                .any(|entry| entry.message == "background marker")
        );
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
                b"schema: 2\nid: com.example.test\nname: Test\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\nruntime:\n  python: '3.11.*'\nparameters:\n  - id: market\n    type: string\n    required: false\n    default: zh-CN\n",
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
        assert_eq!(
            installed.parameters[0].default_value,
            Some(serde_json::json!("zh-CN"))
        );
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
