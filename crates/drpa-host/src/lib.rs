use std::sync::atomic::{AtomicU64, Ordering};

use drpa_protocol::{
    LogEntry, LogLevel, PackageSummary, RunStatus, RunSummary, TaskProfile, TrustLevel,
    WorkspaceSnapshot, WorkspaceStats,
};
use parking_lot::RwLock;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum HostError {
    #[error("package not found: {0}")]
    PackageNotFound(String),
    #[error("profile not found: {0}")]
    ProfileNotFound(String),
    #[error("run not found: {0}")]
    RunNotFound(String),
}

pub struct HostState {
    snapshot: RwLock<WorkspaceSnapshot>,
    sequence: AtomicU64,
}

impl HostState {
    #[must_use]
    pub fn demo() -> Self {
        Self {
            snapshot: RwLock::new(demo_snapshot()),
            sequence: AtomicU64::new(100),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        self.snapshot.read().clone()
    }

    pub fn start_run(&self, package_id: &str, profile_id: &str) -> Result<String, HostError> {
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

        let run_id = format!("run-{}", Uuid::new_v4().simple());
        let run = RunSummary {
            id: run_id.clone(),
            package_name: package.name.clone(),
            profile_name: profile.name.clone(),
            status: RunStatus::Queued,
            started_at: "just now".to_owned(),
            duration: "—".to_owned(),
            progress: None,
        };
        snapshot.runs.insert(0, run);
        snapshot.stats.active_runs += 1;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        snapshot.logs.push(LogEntry {
            id: sequence,
            time: "now".to_owned(),
            level: LogLevel::Info,
            scope: "host".to_owned(),
            message: format!("Run {run_id} accepted by supervisor"),
        });
        Ok(run_id)
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
}

fn demo_snapshot() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        stats: WorkspaceStats {
            active_runs: 1,
            success_rate: 98.6,
            packages: 2,
            saved_hours: 46.8,
        },
        packages: vec![
            PackageSummary {
                id: "com.drpa.invoice-hub".to_owned(),
                name: "Invoice Hub".to_owned(),
                description: "Collect, normalize and archive invoices across supplier portals.".to_owned(),
                version: "2.4.1".to_owned(),
                runtime: "Python 3.11".to_owned(),
                trust: TrustLevel::Verified,
                accent: "#7c9cff".to_owned(),
                initials: "IH".to_owned(),
                profiles: vec![TaskProfile {
                    id: "monthly".to_owned(),
                    name: "Monthly close".to_owned(),
                    schedule: Some("1st · 08:30".to_owned()),
                    last_run: Some("2 hours ago".to_owned()),
                }],
            },
            PackageSummary {
                id: "com.drpa.portal-audit".to_owned(),
                name: "Portal Audit".to_owned(),
                description: "Check account states and export exceptions for follow-up.".to_owned(),
                version: "1.8.0".to_owned(),
                runtime: "Python 3.11".to_owned(),
                trust: TrustLevel::Local,
                accent: "#57d6a0".to_owned(),
                initials: "PA".to_owned(),
                profiles: vec![TaskProfile {
                    id: "daily".to_owned(),
                    name: "Daily control".to_owned(),
                    schedule: Some("Daily · 07:00".to_owned()),
                    last_run: Some("36 min ago".to_owned()),
                }],
            },
        ],
        runs: vec![RunSummary {
            id: "run-1842".to_owned(),
            package_name: "Invoice Hub".to_owned(),
            profile_name: "Monthly close".to_owned(),
            status: RunStatus::Running,
            started_at: "09:42:08".to_owned(),
            duration: "04:18".to_owned(),
            progress: Some(68),
        }],
        logs: vec![LogEntry {
            id: 1,
            time: "09:42:08.214".to_owned(),
            level: LogLevel::Info,
            scope: "host".to_owned(),
            message: "Run run-1842 accepted by supervisor".to_owned(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_run_rejects_unknown_package() {
        let state = HostState::demo();
        let error = state.start_run("missing", "profile").unwrap_err();
        assert!(matches!(error, HostError::PackageNotFound(_)));
    }

    #[test]
    fn start_and_cancel_run_updates_snapshot() {
        let state = HostState::demo();
        let run_id = state.start_run("com.drpa.invoice-hub", "monthly").unwrap();
        state.cancel_run(&run_id).unwrap();
        let run = state.snapshot().runs.into_iter().find(|run| run.id == run_id).unwrap();
        assert!(matches!(run.status, RunStatus::Cancelled));
    }
}
