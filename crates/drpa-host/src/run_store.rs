use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use drpa_protocol::{
    LogLevel, RunArtifactRecord, RunDetail, RunEventRecord, RunStatus, RunSummary, RuntimeEvent,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

const RUN_HISTORY_LIMIT: i64 = 500;

#[derive(Debug)]
pub struct RunStore {
    database_path: PathBuf,
}

impl RunStore {
    pub fn open(workspace_root: &Path) -> Result<Self, String> {
        let system_root = workspace_root.join("system");
        fs::create_dir_all(&system_root).map_err(|error| error.to_string())?;
        let store = Self {
            database_path: system_root.join("drpa.sqlite3"),
        };
        store.initialize()?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection =
            Connection::open(&self.database_path).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| error.to_string())?;
        Ok(connection)
    }

    fn initialize(&self) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 CREATE TABLE IF NOT EXISTS runs (
                    id TEXT PRIMARY KEY,
                    package_id TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    package_version TEXT NOT NULL,
                    profile_id TEXT NOT NULL,
                    profile_name TEXT NOT NULL,
                    source TEXT NOT NULL,
                    status TEXT NOT NULL,
                    started_at TEXT NOT NULL,
                    started_at_epoch_ms INTEGER NOT NULL,
                    finished_at TEXT,
                    duration_ms INTEGER,
                    progress INTEGER,
                    exit_code INTEGER,
                    parameters_json TEXT NOT NULL,
                    output_dir TEXT NOT NULL,
                    error_message TEXT,
                    error_traceback TEXT
                 );
                 CREATE TABLE IF NOT EXISTS run_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                    sequence INTEGER,
                    recorded_at TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    level TEXT,
                    scope TEXT,
                    message TEXT NOT NULL,
                    payload_json TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS run_artifacts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                    sequence INTEGER,
                    label TEXT NOT NULL,
                    path TEXT NOT NULL,
                    media_type TEXT,
                    created_at TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_runs_started_at ON runs(started_at_epoch_ms DESC);
                 CREATE INDEX IF NOT EXISTS idx_run_events_run_id ON run_events(run_id, id);
                 CREATE INDEX IF NOT EXISTS idx_run_artifacts_run_id ON run_artifacts(run_id, id);",
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn mark_unfinished_interrupted(&self) -> Result<(), String> {
        let finished_at = now_timestamp();
        let connection = self.connection()?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO run_events (
                    run_id, sequence, recorded_at, event_type, level, scope, message, payload_json
                 ) SELECT id, NULL, ?1, 'host', 'warning', 'host',
                          '应用重启，任务被标记为中断', '{\"reason\":\"host_restart\"}'
                   FROM runs WHERE status IN ('running', 'queued')",
                [finished_at.as_str()],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE runs
                 SET status = 'interrupted', finished_at = ?1,
                     duration_ms = MAX(0, ?2 - started_at_epoch_ms),
                     error_message = COALESCE(error_message, '应用退出前任务没有写入完成状态')
                 WHERE status IN ('running', 'queued')",
                params![finished_at, now_epoch_ms()],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn create_run(
        &self,
        summary: &RunSummary,
        parameters: &Value,
        output_dir: &Path,
        source: &str,
    ) -> Result<(), String> {
        let parameters_json =
            serde_json::to_string(parameters).map_err(|error| error.to_string())?;
        let connection = self.connection()?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO runs (
                    id, package_id, package_name, package_version, profile_id, profile_name,
                    source, status, started_at, started_at_epoch_ms, progress, parameters_json, output_dir
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    summary.id,
                    summary.package_id,
                    summary.package_name,
                    summary.package_version,
                    summary.profile_id,
                    summary.profile_name,
                    source,
                    status_name(summary.status),
                    summary.started_at,
                    now_epoch_ms(),
                    summary.progress.map(i64::from),
                    parameters_json,
                    output_dir.to_string_lossy(),
                ],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO run_events (
                    run_id, sequence, recorded_at, event_type, level, scope, message, payload_json
                 ) VALUES (?1, NULL, ?2, 'host', 'info', 'host', ?3, ?4)",
                params![
                    summary.id,
                    now_timestamp(),
                    "任务已创建并进入运行队列",
                    serde_json::to_string(&json!({"source": source}))
                        .map_err(|error| error.to_string())?,
                ],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn append_host_event(
        &self,
        run_id: &str,
        level: LogLevel,
        scope: &str,
        message: &str,
        payload: Value,
    ) -> Result<(), String> {
        self.connection()?
            .execute(
                "INSERT INTO run_events (
                    run_id, sequence, recorded_at, event_type, level, scope, message, payload_json
                 ) VALUES (?1, NULL, ?2, 'host', ?3, ?4, ?5, ?6)",
                params![
                    run_id,
                    now_timestamp(),
                    level_name(level),
                    scope,
                    message,
                    serde_json::to_string(&payload).map_err(|error| error.to_string())?,
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn append_runtime_event(&self, run_id: &str, event: &RuntimeEvent) -> Result<(), String> {
        let payload = serde_json::to_value(event).map_err(|error| error.to_string())?;
        let (sequence, event_type, level, scope, message) = runtime_event_fields(event);
        let connection = self.connection()?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO run_events (
                    run_id, sequence, recorded_at, event_type, level, scope, message, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    run_id,
                    sequence.map(|value| value as i64),
                    now_timestamp(),
                    event_type,
                    level.map(level_name),
                    scope,
                    message,
                    serde_json::to_string(&payload).map_err(|error| error.to_string())?,
                ],
            )
            .map_err(|error| error.to_string())?;

        match event {
            RuntimeEvent::Progress { value, .. } => {
                let progress = value.clamp(0.0, 100.0).round() as i64;
                transaction
                    .execute(
                        "UPDATE runs SET progress = ?2 WHERE id = ?1",
                        params![run_id, progress],
                    )
                    .map_err(|error| error.to_string())?;
            }
            RuntimeEvent::Artifact {
                sequence,
                path,
                label,
                media_type,
            } => {
                transaction
                    .execute(
                        "INSERT INTO run_artifacts (run_id, sequence, label, path, media_type, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![run_id, *sequence as i64, label, path, media_type, now_timestamp()],
                    )
                    .map_err(|error| error.to_string())?;
            }
            RuntimeEvent::Error {
                message, traceback, ..
            } => {
                transaction
                    .execute(
                        "UPDATE runs SET error_message = ?2, error_traceback = ?3 WHERE id = ?1",
                        params![run_id, message, traceback],
                    )
                    .map_err(|error| error.to_string())?;
            }
            _ => {}
        }
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn finish_run(
        &self,
        run_id: &str,
        status: RunStatus,
        exit_code: Option<i32>,
        error_message: Option<&str>,
        error_traceback: Option<&str>,
    ) -> Result<(String, u64), String> {
        let connection = self.connection()?;
        let started_at_epoch_ms: i64 = connection
            .query_row(
                "SELECT started_at_epoch_ms FROM runs WHERE id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let finished_epoch_ms = now_epoch_ms();
        let duration_ms = finished_epoch_ms.saturating_sub(started_at_epoch_ms) as u64;
        let finished_at = now_timestamp();
        let progress = if status == RunStatus::Success {
            Some(100_i64)
        } else {
            None
        };
        connection
            .execute(
                "UPDATE runs SET
                    status = ?2,
                    finished_at = ?3,
                    duration_ms = ?4,
                    progress = COALESCE(?5, progress),
                    exit_code = COALESCE(?6, exit_code),
                    error_message = COALESCE(?7, error_message),
                    error_traceback = COALESCE(?8, error_traceback)
                 WHERE id = ?1",
                params![
                    run_id,
                    status_name(status),
                    finished_at,
                    duration_ms as i64,
                    progress,
                    exit_code,
                    error_message,
                    error_traceback,
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok((finished_at, duration_ms))
    }

    pub fn load_summaries(&self) -> Result<Vec<RunSummary>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, package_id, package_name, package_version, profile_id, profile_name,
                        status, started_at, finished_at, duration_ms, progress, exit_code
                 FROM runs ORDER BY started_at_epoch_ms DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([RUN_HISTORY_LIMIT], summary_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn get_detail(&self, run_id: &str) -> Result<Option<RunDetail>, String> {
        let connection = self.connection()?;
        let run = connection
            .query_row(
                "SELECT id, package_id, package_name, package_version, profile_id, profile_name,
                        status, started_at, finished_at, duration_ms, progress, exit_code,
                        parameters_json, output_dir, error_message, error_traceback
                 FROM runs WHERE id = ?1",
                [run_id],
                |row| {
                    let summary = summary_from_row(row)?;
                    let parameters_json: String = row.get(12)?;
                    Ok((
                        summary,
                        parameters_json,
                        row.get::<_, String>(13)?,
                        row.get::<_, Option<String>>(14)?,
                        row.get::<_, Option<String>>(15)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((summary, parameters_json, output_dir, error_message, error_traceback)) = run
        else {
            return Ok(None);
        };
        let parameters = serde_json::from_str(&parameters_json).unwrap_or_else(|_| json!({}));
        let events = load_events(&connection, run_id)?;
        let artifacts = load_artifacts(&connection, run_id)?;
        Ok(Some(RunDetail {
            summary,
            parameters,
            output_dir,
            error_message,
            error_traceback,
            events,
            artifacts,
        }))
    }

    pub fn delete_run(&self, run_id: &str) -> Result<Option<String>, String> {
        let connection = self.connection()?;
        let output_dir = connection
            .query_row(
                "SELECT output_dir FROM runs WHERE id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        connection
            .execute("DELETE FROM runs WHERE id = ?1", [run_id])
            .map_err(|error| error.to_string())?;
        Ok(output_dir)
    }
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunSummary> {
    let duration_ms = row
        .get::<_, Option<i64>>(9)?
        .map(|value| value.max(0) as u64);
    Ok(RunSummary {
        id: row.get(0)?,
        package_id: row.get(1)?,
        package_name: row.get(2)?,
        package_version: row.get(3)?,
        profile_id: row.get(4)?,
        profile_name: row.get(5)?,
        status: parse_status(&row.get::<_, String>(6)?),
        started_at: row.get(7)?,
        finished_at: row.get(8)?,
        duration: duration_ms.map_or_else(|| "—".to_owned(), format_duration),
        duration_ms,
        progress: row
            .get::<_, Option<i64>>(10)?
            .map(|value| value.clamp(0, 100) as u8),
        exit_code: row.get(11)?,
    })
}

fn load_events(connection: &Connection, run_id: &str) -> Result<Vec<RunEventRecord>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, run_id, sequence, recorded_at, event_type, level, scope, message, payload_json
             FROM run_events WHERE run_id = ?1 ORDER BY id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([run_id], |row| {
            let payload_json: String = row.get(8)?;
            Ok(RunEventRecord {
                id: row.get::<_, i64>(0)?.max(0) as u64,
                run_id: row.get(1)?,
                sequence: row
                    .get::<_, Option<i64>>(2)?
                    .map(|value| value.max(0) as u64),
                recorded_at: row.get(3)?,
                event_type: row.get(4)?,
                level: row.get::<_, Option<String>>(5)?.as_deref().map(parse_level),
                scope: row.get(6)?,
                message: row.get(7)?,
                payload: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn load_artifacts(connection: &Connection, run_id: &str) -> Result<Vec<RunArtifactRecord>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, run_id, sequence, label, path, media_type, created_at
             FROM run_artifacts WHERE run_id = ?1 ORDER BY id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([run_id], |row| {
            let path: String = row.get(4)?;
            let size = fs::metadata(&path).ok().map(|metadata| metadata.len());
            Ok(RunArtifactRecord {
                id: row.get::<_, i64>(0)?.max(0) as u64,
                run_id: row.get(1)?,
                sequence: row
                    .get::<_, Option<i64>>(2)?
                    .map(|value| value.max(0) as u64),
                label: row.get(3)?,
                path,
                media_type: row.get(5)?,
                size,
                created_at: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn runtime_event_fields(
    event: &RuntimeEvent,
) -> (
    Option<u64>,
    &'static str,
    Option<LogLevel>,
    Option<String>,
    String,
) {
    match event {
        RuntimeEvent::Ready { protocol } => (
            None,
            "ready",
            Some(LogLevel::Info),
            Some("runtime".to_owned()),
            format!("运行时握手完成 · protocol={protocol}"),
        ),
        RuntimeEvent::Log {
            sequence,
            level,
            scope,
            message,
        } => (
            Some(*sequence),
            "log",
            Some(*level),
            Some(scope.clone()),
            message.clone(),
        ),
        RuntimeEvent::Progress {
            sequence,
            value,
            message,
        } => (
            Some(*sequence),
            "progress",
            Some(LogLevel::Info),
            Some("progress".to_owned()),
            message
                .clone()
                .unwrap_or_else(|| format!("进度 {value:.0}%")),
        ),
        RuntimeEvent::Artifact {
            sequence,
            path,
            label,
            ..
        } => (
            Some(*sequence),
            "artifact",
            Some(LogLevel::Success),
            Some("artifact".to_owned()),
            format!("产物已登记 · {label} · {path}"),
        ),
        RuntimeEvent::OpenDirectory { sequence, path } => (
            Some(*sequence),
            "open_directory",
            Some(LogLevel::Info),
            Some("workspace".to_owned()),
            format!("请求打开输出目录 · {path}"),
        ),
        RuntimeEvent::Warning { sequence, message } => (
            Some(*sequence),
            "warning",
            Some(LogLevel::Warning),
            Some("runtime".to_owned()),
            message.clone(),
        ),
        RuntimeEvent::Error {
            sequence, message, ..
        } => (
            Some(*sequence),
            "error",
            Some(LogLevel::Error),
            Some("runtime".to_owned()),
            message.clone(),
        ),
        RuntimeEvent::Completed {
            sequence,
            exit_code,
        } => (
            Some(*sequence),
            "completed",
            Some(if *exit_code == 0 {
                LogLevel::Success
            } else {
                LogLevel::Error
            }),
            Some("runtime".to_owned()),
            format!("任务结束 · exitCode={exit_code}"),
        ),
    }
}

pub fn now_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn now_epoch_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn format_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms} ms")
    } else if duration_ms < 60_000 {
        format!("{:.1} s", duration_ms as f64 / 1_000.0)
    } else {
        let minutes = duration_ms / 60_000;
        let seconds = (duration_ms % 60_000) / 1_000;
        format!("{minutes}m {seconds:02}s")
    }
}

fn status_name(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Queued => "queued",
        RunStatus::Success => "success",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
        RunStatus::Interrupted => "interrupted",
    }
}

fn parse_status(value: &str) -> RunStatus {
    match value {
        "running" => RunStatus::Running,
        "queued" => RunStatus::Queued,
        "success" => RunStatus::Success,
        "cancelled" => RunStatus::Cancelled,
        "interrupted" => RunStatus::Interrupted,
        _ => RunStatus::Failed,
    }
}

fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "trace",
        LogLevel::Info => "info",
        LogLevel::Success => "success",
        LogLevel::Warning => "warning",
        LogLevel::Error => "error",
    }
}

fn parse_level(value: &str) -> LogLevel {
    match value {
        "trace" => LogLevel::Trace,
        "success" => LogLevel::Success,
        "warning" => LogLevel::Warning,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn summary() -> RunSummary {
        RunSummary {
            id: "run-test".to_owned(),
            package_id: "sample.package".to_owned(),
            package_name: "示例任务".to_owned(),
            package_version: "1.0.0".to_owned(),
            profile_id: "default".to_owned(),
            profile_name: "默认配置".to_owned(),
            status: RunStatus::Running,
            started_at: now_timestamp(),
            finished_at: None,
            duration: "—".to_owned(),
            duration_ms: None,
            progress: None,
            exit_code: None,
        }
    }

    #[test]
    fn persists_run_events_artifacts_and_completion() {
        let directory = tempdir().unwrap();
        let store = RunStore::open(directory.path()).unwrap();
        let output_dir = directory.path().join("runs/run-test/outputs");
        fs::create_dir_all(&output_dir).unwrap();
        let artifact = output_dir.join("result.txt");
        fs::write(&artifact, "done").unwrap();
        store
            .create_run(
                &summary(),
                &json!({"secret": "***"}),
                &output_dir,
                "workbench",
            )
            .unwrap();
        store
            .append_runtime_event(
                "run-test",
                &RuntimeEvent::Progress {
                    sequence: 1,
                    value: 50.0,
                    message: Some("处理中".to_owned()),
                },
            )
            .unwrap();
        store
            .append_runtime_event(
                "run-test",
                &RuntimeEvent::Artifact {
                    sequence: 2,
                    path: artifact.to_string_lossy().into_owned(),
                    label: "结果".to_owned(),
                    media_type: Some("text/plain".to_owned()),
                },
            )
            .unwrap();
        store
            .finish_run("run-test", RunStatus::Success, Some(0), None, None)
            .unwrap();

        let detail = store.get_detail("run-test").unwrap().unwrap();
        assert_eq!(detail.summary.status, RunStatus::Success);
        assert_eq!(detail.summary.progress, Some(100));
        assert_eq!(detail.summary.exit_code, Some(0));
        assert_eq!(detail.parameters, json!({"secret": "***"}));
        assert_eq!(detail.artifacts[0].size, Some(4));
        assert!(detail.events.iter().any(|event| event.message == "处理中"));
    }

    #[test]
    fn marks_unfinished_runs_as_interrupted_after_restart() {
        let directory = tempdir().unwrap();
        let store = RunStore::open(directory.path()).unwrap();
        store
            .create_run(
                &summary(),
                &json!({}),
                &directory.path().join("outputs"),
                "studio",
            )
            .unwrap();
        store.mark_unfinished_interrupted().unwrap();

        let runs = store.load_summaries().unwrap();
        assert_eq!(runs[0].status, RunStatus::Interrupted);
        assert!(runs[0].finished_at.is_some());
        let detail = store.get_detail("run-test").unwrap().unwrap();
        assert!(
            detail
                .events
                .iter()
                .any(|event| event.message.contains("标记为中断"))
        );
    }
}
