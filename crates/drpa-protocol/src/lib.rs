use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOST_PROTOCOL_VERSION: u16 = 1;
pub const RUNTIME_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub packages: Vec<PackageSummary>,
    pub runs: Vec<RunSummary>,
    pub logs: Vec<LogEntry>,
    pub stats: WorkspaceStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStats {
    pub active_runs: u32,
    pub success_rate: f64,
    pub packages: u32,
    pub saved_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub runtime: String,
    pub trust: TrustLevel,
    pub accent: String,
    pub initials: String,
    pub profiles: Vec<TaskProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProfile {
    pub id: String,
    pub name: String,
    pub schedule: Option<String>,
    pub last_run: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: String,
    pub package_name: String,
    pub profile_name: String,
    pub status: RunStatus,
    pub started_at: String,
    pub duration: String,
    pub progress: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub time: String,
    pub level: LogLevel,
    pub scope: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Queued,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    Verified,
    Local,
    Untrusted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeRequest {
    Initialize {
        protocol: u16,
        run_id: String,
        package_dir: String,
        output_dir: String,
    },
    Start {
        entrypoint: String,
        parameters: Value,
        secret_handles: Vec<String>,
    },
    Cancel {
        reason: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Ready {
        protocol: u16,
    },
    Log {
        sequence: u64,
        level: LogLevel,
        scope: String,
        message: String,
    },
    Progress {
        sequence: u64,
        value: f64,
        message: Option<String>,
    },
    Artifact {
        sequence: u64,
        path: String,
        label: String,
        media_type: Option<String>,
    },
    Warning {
        sequence: u64,
        message: String,
    },
    Error {
        sequence: u64,
        message: String,
        traceback: Option<String>,
    },
    Completed {
        sequence: u64,
        exit_code: i32,
    },
}