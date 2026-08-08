use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::agent::{AgentStreamEvent, AgentTurnResult, AgentUsage};

const MAX_RETAINED_RUNS: usize = 256;
const MAX_RETAINED_EVENTS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AgentRunStatus {
    Running,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentRunJournalEvent {
    pub(crate) sequence: u64,
    pub(crate) at: u64,
    pub(crate) event: AgentStreamEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentRunSnapshot {
    pub(crate) request_id: String,
    pub(crate) session_id: String,
    pub(crate) status: AgentRunStatus,
    pub(crate) created_at: u64,
    pub(crate) started_at: Option<u64>,
    pub(crate) finished_at: Option<u64>,
    pub(crate) stop_reason: String,
    pub(crate) error: String,
    pub(crate) usage: AgentUsage,
    pub(crate) duration_ms: u64,
    pub(crate) events: Vec<AgentRunJournalEvent>,
}

struct ManagedRun {
    snapshot: AgentRunSnapshot,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(crate) struct AgentRunControl {
    request_id: String,
    session_id: String,
    cancel: Arc<AtomicBool>,
    started: Instant,
    deadline: Instant,
}

impl AgentRunControl {
    pub(crate) fn request_id(&self) -> &str {
        &self.request_id
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            return Err("Agent 运行已取消".to_owned());
        }
        if Instant::now() >= self.deadline {
            return Err("Agent 运行超过最大时长，已停止".to_owned());
        }
        Ok(())
    }

    pub(crate) fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        let started = Instant::now();
        Self {
            request_id: "test-run".to_owned(),
            session_id: "test-session".to_owned(),
            cancel: Arc::new(AtomicBool::new(false)),
            started,
            deadline: started + Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct AgentRunManager {
    runs: Arc<Mutex<HashMap<String, ManagedRun>>>,
    session_leases: Arc<Mutex<HashMap<String, String>>>,
    journal_root: Option<Arc<PathBuf>>,
}

impl AgentRunManager {
    pub(crate) fn with_workspace(workspace_root: &Path) -> Self {
        Self {
            journal_root: Some(Arc::new(workspace_root.join("agent").join("runs"))),
            ..Self::default()
        }
    }

    pub(crate) fn begin(
        &self,
        request_id: &str,
        session_id: &str,
        max_wall_time: Duration,
    ) -> Result<AgentRunControl, String> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Agent Run 状态已损坏".to_owned())?;
        if runs.get(request_id).is_some_and(|run| {
            matches!(
                run.snapshot.status,
                AgentRunStatus::Running | AgentRunStatus::Cancelling
            )
        }) {
            return Err("同一 requestId 的 Agent Run 已在执行".to_owned());
        }
        let session_id = session_id.trim().to_owned();
        if !session_id.is_empty() {
            let mut leases = self
                .session_leases
                .lock()
                .map_err(|_| "Agent 会话租约状态已损坏".to_owned())?;
            if let Some(owner) = leases.get(&session_id) {
                return Err(format!("当前会话已有 Agent Run 在执行：{owner}"));
            }
            leases.insert(session_id.clone(), request_id.to_owned());
        }
        if runs.len() >= MAX_RETAINED_RUNS {
            let mut completed = runs
                .iter()
                .filter(|(_, run)| {
                    !matches!(
                        run.snapshot.status,
                        AgentRunStatus::Running | AgentRunStatus::Cancelling
                    )
                })
                .map(|(id, run)| {
                    (
                        id.clone(),
                        run.snapshot.finished_at.unwrap_or(run.snapshot.created_at),
                    )
                })
                .collect::<Vec<_>>();
            completed.sort_by_key(|(_, finished)| *finished);
            for (id, _) in completed
                .into_iter()
                .take(runs.len().saturating_sub(MAX_RETAINED_RUNS - 1))
            {
                runs.remove(&id);
            }
        }
        let now = now_millis();
        let cancel = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        runs.insert(
            request_id.to_owned(),
            ManagedRun {
                snapshot: AgentRunSnapshot {
                    request_id: request_id.to_owned(),
                    session_id: session_id.clone(),
                    status: AgentRunStatus::Running,
                    created_at: now,
                    started_at: Some(now),
                    finished_at: None,
                    stop_reason: String::new(),
                    error: String::new(),
                    usage: AgentUsage::default(),
                    duration_ms: 0,
                    events: Vec::new(),
                },
                cancel: Arc::clone(&cancel),
            },
        );
        if let Some(run) = runs.get(request_id) {
            self.persist_snapshot(&run.snapshot);
        }
        Ok(AgentRunControl {
            request_id: request_id.to_owned(),
            session_id,
            cancel,
            started,
            deadline: started + max_wall_time,
        })
    }

    pub(crate) fn record(&self, request_id: &str, event: AgentStreamEvent) {
        if let Ok(mut runs) = self.runs.lock()
            && let Some(run) = runs.get_mut(request_id)
        {
            let sequence = run
                .snapshot
                .events
                .last()
                .map_or(1, |event| event.sequence.saturating_add(1));
            run.snapshot.events.push(AgentRunJournalEvent {
                sequence,
                at: now_millis(),
                event,
            });
            if run.snapshot.events.len() > MAX_RETAINED_EVENTS {
                let excess = run.snapshot.events.len() - MAX_RETAINED_EVENTS;
                run.snapshot.events.drain(..excess);
            }
            self.append_event(
                request_id,
                run.snapshot.events.last().expect("event was appended"),
            );
            self.persist_snapshot(&run.snapshot);
        }
    }

    pub(crate) fn complete(&self, control: &AgentRunControl, result: &AgentTurnResult) {
        self.finish(
            control,
            AgentRunStatus::Completed,
            &result.stop_reason,
            "",
            Some(result),
        );
    }

    pub(crate) fn fail(&self, control: &AgentRunControl, error: &str) {
        let cancelled = control.is_cancelled() || error.contains("运行已取消");
        self.finish(
            control,
            if cancelled {
                AgentRunStatus::Cancelled
            } else {
                AgentRunStatus::Failed
            },
            if cancelled { "cancelled" } else { "error" },
            error,
            None,
        );
    }

    fn finish(
        &self,
        control: &AgentRunControl,
        status: AgentRunStatus,
        stop_reason: &str,
        error: &str,
        result: Option<&AgentTurnResult>,
    ) {
        if let Ok(mut runs) = self.runs.lock()
            && let Some(run) = runs.get_mut(control.request_id())
        {
            run.snapshot.status = status;
            run.snapshot.finished_at = Some(now_millis());
            run.snapshot.stop_reason = stop_reason.to_owned();
            run.snapshot.error = error.to_owned();
            run.snapshot.duration_ms = result
                .map(|result| result.duration_ms)
                .unwrap_or_else(|| control.elapsed_ms());
            if let Some(result) = result {
                run.snapshot.usage = result.usage.clone();
            }
            self.persist_snapshot(&run.snapshot);
        }
        self.release_session(control);
    }

    fn release_session(&self, control: &AgentRunControl) {
        if control.session_id().is_empty() {
            return;
        }
        if let Ok(mut leases) = self.session_leases.lock()
            && leases
                .get(control.session_id())
                .is_some_and(|owner| owner == control.request_id())
        {
            leases.remove(control.session_id());
        }
    }

    pub(crate) fn cancel(&self, request_id: &str) -> Result<AgentRunSnapshot, String> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Agent Run 状态已损坏".to_owned())?;
        let run = runs
            .get_mut(request_id)
            .ok_or_else(|| "Agent Run 不存在".to_owned())?;
        match run.snapshot.status {
            AgentRunStatus::Running | AgentRunStatus::Cancelling => {
                run.cancel.store(true, Ordering::Release);
                run.snapshot.status = AgentRunStatus::Cancelling;
                run.snapshot.stop_reason = "cancellation-requested".to_owned();
            }
            _ => {}
        }
        Ok(run.snapshot.clone())
    }

    pub(crate) fn snapshot(&self, request_id: &str) -> Result<AgentRunSnapshot, String> {
        self.runs
            .lock()
            .map_err(|_| "Agent Run 状态已损坏".to_owned())?
            .get(request_id)
            .map(|run| run.snapshot.clone())
            .ok_or_else(|| "Agent Run 不存在".to_owned())
    }

    fn persist_snapshot(&self, snapshot: &AgentRunSnapshot) {
        let Some(root) = self.journal_root.as_deref() else {
            return;
        };
        if fs::create_dir_all(root).is_err() {
            return;
        }
        let Ok(bytes) = serde_json::to_vec_pretty(snapshot) else {
            return;
        };
        let target = root.join(format!("{}.json", snapshot.request_id));
        let _ = fs::write(target, bytes);
    }

    fn append_event(&self, request_id: &str, event: &AgentRunJournalEvent) {
        let Some(root) = self.journal_root.as_deref() else {
            return;
        };
        if fs::create_dir_all(root).is_err() {
            return;
        }
        let Ok(mut bytes) = serde_json::to_vec(event) else {
            return;
        };
        bytes.push(b'\n');
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(format!("{request_id}.jsonl")))
        {
            let _ = file.write_all(&bytes);
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_sessions_and_records_monotonic_events() {
        let manager = AgentRunManager::default();
        let control = manager
            .begin("req-one", "session-one", Duration::from_secs(10))
            .unwrap();
        assert!(
            manager
                .begin("req-two", "session-one", Duration::from_secs(10))
                .is_err()
        );
        manager.record("req-one", AgentStreamEvent::RoundStarted { round: 1 });
        manager.record(
            "req-one",
            AgentStreamEvent::Delta {
                content: "ok".to_owned(),
            },
        );
        let snapshot = manager.snapshot("req-one").unwrap();
        assert_eq!(snapshot.events[0].sequence, 1);
        assert_eq!(snapshot.events[1].sequence, 2);

        manager.fail(&control, "done with an error");
        assert!(
            manager
                .begin("req-two", "session-one", Duration::from_secs(10))
                .is_ok()
        );
    }

    #[test]
    fn cancellation_is_visible_to_the_running_adapter() {
        let manager = AgentRunManager::default();
        let control = manager
            .begin("req-cancel", "session-cancel", Duration::from_secs(10))
            .unwrap();
        manager.cancel("req-cancel").unwrap();
        assert!(control.is_cancelled());
        assert!(control.check().unwrap_err().contains("已取消"));
    }
}
