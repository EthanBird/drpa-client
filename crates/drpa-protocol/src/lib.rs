use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOST_PROTOCOL_VERSION: u16 = 1;
pub const RUNTIME_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub packages: Vec<PackageSummary>,
    pub runs: Vec<RunSummary>,
    #[serde(default)]
    pub automations: Vec<AutomationSummary>,
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
pub struct WindowsUpdateSession {
    pub id: String,
    pub version: String,
    pub total_files: u32,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsUpdateStatus {
    pub session_id: String,
    pub version: String,
    pub phase: WindowsUpdatePhase,
    pub progress: u8,
    pub completed_files: u32,
    pub total_files: u32,
    pub current_file: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowsUpdatePhase {
    Verifying,
    Applying,
    WaitingForRestart,
    Restarting,
    Completed,
    Failed,
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
    #[serde(default)]
    pub parameters: Vec<ParameterSummary>,
    pub profiles: Vec<TaskProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterSummary {
    pub id: String,
    pub kind: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
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
pub struct AutomationSummary {
    pub id: String,
    pub name: String,
    pub package_name: String,
    pub profile_name: String,
    pub trigger_label: String,
    pub next_run: String,
    pub last_run: Option<String>,
    pub health: String,
    pub status: AutomationStatus,
    pub concurrency_policy: ConcurrencyPolicy,
    pub retry_policy: String,
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
#[serde(rename_all = "camelCase")]
pub enum AutomationStatus {
    Enabled,
    Paused,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConcurrencyPolicy {
    Allow,
    Forbid,
    Replace,
    QueueOne,
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
    OpenDirectory {
        sequence: u64,
        path: String,
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
