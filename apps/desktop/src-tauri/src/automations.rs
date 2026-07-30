use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use drpa_host::HostState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::{AppPaths, RunProcessManager};

const STORE_SCHEMA: u32 = 1;
const MAX_HISTORY_ENTRIES: usize = 500;
const SCHEDULER_POLL_SECONDS: u64 = 5;
const QUEUE_POLL_MILLIS: u64 = 250;
const MAX_NAME_CHARS: usize = 120;
const MAX_DESCRIPTION_CHARS: usize = 4_000;
const MAX_TIMEOUT_SECONDS: u64 = 86_400;
const MAX_RETRY_ATTEMPTS: u32 = 20;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomationPlan {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) enabled: bool,
    pub(crate) schedule: AutomationSchedule,
    pub(crate) action: AutomationAction,
    #[serde(default)]
    pub(crate) concurrency_policy: ConcurrencyPolicy,
    #[serde(default)]
    pub(crate) retry_policy: RetryPolicy,
    #[serde(default = "default_timeout_seconds")]
    pub(crate) timeout_seconds: u64,
    #[serde(default)]
    pub(crate) delivery_targets: Vec<DeliveryTarget>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    #[serde(default)]
    pub(crate) last_run_at: Option<i64>,
    #[serde(default)]
    pub(crate) next_run_at: Option<i64>,
    #[serde(default)]
    pub(crate) last_scheduled_minute: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomationPlanInput {
    #[serde(default)]
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) enabled: bool,
    pub(crate) schedule: AutomationSchedule,
    pub(crate) action: AutomationAction,
    #[serde(default)]
    pub(crate) concurrency_policy: ConcurrencyPolicy,
    #[serde(default)]
    pub(crate) retry_policy: RetryPolicy,
    #[serde(default = "default_timeout_seconds")]
    pub(crate) timeout_seconds: u64,
    #[serde(default)]
    pub(crate) delivery_targets: Vec<DeliveryTarget>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomationSchedule {
    #[serde(rename = "type")]
    pub(crate) kind: ScheduleKind,
    #[serde(default)]
    pub(crate) cron: String,
    #[serde(default)]
    pub(crate) time: String,
    #[serde(default)]
    pub(crate) days_of_week: Vec<u8>,
    #[serde(default)]
    pub(crate) interval_minutes: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ScheduleKind {
    Cron,
    Daily,
    Weekly,
    Interval,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomationAction {
    #[serde(rename = "type", default = "default_action_kind")]
    pub(crate) kind: String,
    pub(crate) package_id: String,
    #[serde(default)]
    pub(crate) entrypoint: String,
    #[serde(default)]
    pub(crate) parameters: Value,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConcurrencyPolicy {
    #[default]
    Skip,
    Queue,
    Parallel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetryPolicy {
    #[serde(default = "default_max_attempts")]
    pub(crate) max_attempts: u32,
    #[serde(default = "default_retry_delay_seconds")]
    pub(crate) delay_seconds: u64,
    #[serde(default = "default_backoff_multiplier")]
    pub(crate) backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            delay_seconds: default_retry_delay_seconds(),
            backoff_multiplier: default_backoff_multiplier(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeliveryTarget {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) name: String,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) configuration: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomationRun {
    pub(crate) id: String,
    pub(crate) plan_id: String,
    pub(crate) plan_name: String,
    pub(crate) trigger: AutomationTrigger,
    pub(crate) status: AutomationRunStatus,
    pub(crate) queued_at: i64,
    #[serde(default)]
    pub(crate) started_at: Option<i64>,
    #[serde(default)]
    pub(crate) finished_at: Option<i64>,
    #[serde(default)]
    pub(crate) attempt: u32,
    #[serde(default)]
    pub(crate) result: Option<Value>,
    #[serde(default)]
    pub(crate) error: String,
    #[serde(default)]
    pub(crate) delivery_results: Vec<DeliveryResult>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AutomationTrigger {
    Manual,
    Scheduled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AutomationRunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeliveryResult {
    pub(crate) target_id: String,
    pub(crate) target_name: String,
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) error: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanStore {
    schema: u32,
    plans: Vec<AutomationPlan>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryStore {
    schema: u32,
    runs: Vec<AutomationRun>,
}

struct SchedulerInner {
    io_lock: Mutex<()>,
    running: Mutex<HashMap<String, usize>>,
    started: AtomicBool,
    stop: AtomicBool,
}

#[derive(Clone)]
struct SchedulerContext {
    host: HostState,
    paths: AppPaths,
    processes: RunProcessManager,
}

impl SchedulerContext {
    fn from_app(app: &AppHandle) -> Self {
        Self {
            host: app.state::<HostState>().inner().clone(),
            paths: app.state::<AppPaths>().inner().clone(),
            processes: app.state::<RunProcessManager>().inner().clone(),
        }
    }
}

pub(crate) struct SchedulerManager {
    inner: Arc<SchedulerInner>,
    stop_on_drop: bool,
}

impl Clone for SchedulerManager {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            stop_on_drop: false,
        }
    }
}

impl Default for SchedulerManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(SchedulerInner {
                io_lock: Mutex::new(()),
                running: Mutex::new(HashMap::new()),
                started: AtomicBool::new(false),
                stop: AtomicBool::new(false),
            }),
            stop_on_drop: true,
        }
    }
}

impl SchedulerManager {
    /// Starts the process-wide scheduler. Calling this more than once is harmless.
    ///
    /// The application should manage `SchedulerManager`, `AppPaths`, `HostState`, and
    /// `RunProcessManager` before invoking this method from its Tauri setup hook.
    pub(crate) fn start(&self, app: AppHandle) {
        if self.inner.started.swap(true, Ordering::AcqRel) {
            return;
        }
        self.inner.stop.store(false, Ordering::Release);
        let manager = self.clone();
        let context = SchedulerContext::from_app(&app);
        thread::spawn(move || {
            while !manager.inner.stop.load(Ordering::Acquire) {
                if let Err(error) = manager.tick(&context) {
                    eprintln!("automation scheduler tick failed: {error}");
                }
                for _ in 0..SCHEDULER_POLL_SECONDS {
                    if manager.inner.stop.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            }
            manager.inner.started.store(false, Ordering::Release);
        });
    }

    pub(crate) fn stop(&self) {
        self.inner.stop.store(true, Ordering::Release);
    }

    fn tick(&self, context: &SchedulerContext) -> Result<(), String> {
        let now = Local::now();
        let minute = minute_bucket(now.timestamp_millis());
        let due = {
            let _guard = self
                .inner
                .io_lock
                .lock()
                .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
            let mut store = load_plan_store(&context.paths.workspace_root)?;
            let mut due = Vec::new();
            let mut changed = false;
            for plan in &mut store.plans {
                let next = next_run_at(plan, now.timestamp_millis());
                if plan.next_run_at != next {
                    plan.next_run_at = next;
                    changed = true;
                }
                if should_trigger_scheduled(plan, now) {
                    plan.last_scheduled_minute = Some(minute);
                    plan.last_run_at = Some(now_millis());
                    plan.next_run_at = next_run_at(plan, minute);
                    plan.updated_at = now_millis();
                    due.push(plan.clone());
                    changed = true;
                }
            }
            if changed {
                save_plan_store(&context.paths.workspace_root, &store)?;
            }
            due
        };

        for plan in due {
            self.launch(context.clone(), plan, AutomationTrigger::Scheduled)?;
        }
        Ok(())
    }

    fn launch(
        &self,
        context: SchedulerContext,
        plan: AutomationPlan,
        trigger: AutomationTrigger,
    ) -> Result<AutomationRun, String> {
        let mut run = AutomationRun {
            id: format!("automation-run-{}", Uuid::new_v4().simple()),
            plan_id: plan.id.clone(),
            plan_name: plan.name.clone(),
            trigger,
            status: AutomationRunStatus::Queued,
            queued_at: now_millis(),
            started_at: None,
            finished_at: None,
            attempt: 0,
            result: None,
            error: String::new(),
            delivery_results: Vec::new(),
        };

        let already_running = self.running_count(&plan.id)? > 0;
        if already_running && plan.concurrency_policy == ConcurrencyPolicy::Skip {
            run.status = AutomationRunStatus::Skipped;
            run.finished_at = Some(now_millis());
            run.error = "已有同一计划的任务正在运行，并发策略为跳过".to_owned();
            self.append_history(&context.paths.workspace_root, run.clone())?;
            return Ok(run);
        }

        self.append_history(&context.paths.workspace_root, run.clone())?;
        let manager = self.clone();
        let run_id = run.id.clone();
        thread::spawn(move || manager.execute_queued(context, plan, run_id));
        Ok(run)
    }

    fn execute_queued(&self, context: SchedulerContext, plan: AutomationPlan, run_id: String) {
        if plan.concurrency_policy == ConcurrencyPolicy::Queue {
            loop {
                match self.try_acquire(&plan.id, false) {
                    Ok(true) => break,
                    Ok(false) if self.inner.stop.load(Ordering::Acquire) => {
                        let _ = self.finish_with_error(
                            &context.paths.workspace_root,
                            &run_id,
                            "应用正在关闭，排队任务已取消".to_owned(),
                        );
                        return;
                    }
                    Ok(false) => thread::sleep(Duration::from_millis(QUEUE_POLL_MILLIS)),
                    Err(error) => {
                        let _ =
                            self.finish_with_error(&context.paths.workspace_root, &run_id, error);
                        return;
                    }
                }
            }
        } else {
            match self.try_acquire(
                &plan.id,
                plan.concurrency_policy == ConcurrencyPolicy::Parallel,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    let _ = self.update_history(&context.paths.workspace_root, &run_id, |run| {
                        run.status = AutomationRunStatus::Skipped;
                        run.finished_at = Some(now_millis());
                        run.error = "已有同一计划的任务正在运行，并发策略为跳过".to_owned();
                    });
                    return;
                }
                Err(error) => {
                    let _ = self.finish_with_error(&context.paths.workspace_root, &run_id, error);
                    return;
                }
            }
        }

        let _running_guard = RunningGuard {
            manager: self.clone(),
            plan_id: plan.id.clone(),
        };
        if let Err(error) = self.mark_running(&context.paths.workspace_root, &run_id) {
            let _ = self.finish_with_error(&context.paths.workspace_root, &run_id, error);
            return;
        }

        let max_attempts = plan.retry_policy.max_attempts.max(1);
        let mut final_result = None;
        let mut final_error = String::new();
        let mut attempts = 0;
        while attempts < max_attempts {
            attempts += 1;
            let started = std::time::Instant::now();
            let result = crate::dispatch_automation_run(
                &context.host,
                &context.paths,
                &context.processes,
                &plan,
            );
            match result {
                Ok(value) if started.elapsed().as_secs() <= plan.timeout_seconds => {
                    final_result = Some(value);
                    final_error.clear();
                    break;
                }
                Ok(_) => {
                    final_error = format!("任务超过 {} 秒超时限制", plan.timeout_seconds);
                }
                Err(error) => {
                    final_error = error;
                }
            }
            if attempts < max_attempts {
                let delay = retry_delay(&plan.retry_policy, attempts);
                if delay > 0 {
                    thread::sleep(Duration::from_secs(delay));
                }
            }
        }

        let result = self.update_history(&context.paths.workspace_root, &run_id, |run| {
            run.attempt = attempts;
            run.finished_at = Some(now_millis());
            if let Some(result) = final_result {
                run.status = AutomationRunStatus::Succeeded;
                run.result = Some(result);
                run.error.clear();
                run.delivery_results = plan
                    .delivery_targets
                    .iter()
                    .filter(|target| target.enabled)
                    .map(|target| DeliveryResult {
                        target_id: target.id.clone(),
                        target_name: target.name.clone(),
                        status: "dispatched".to_owned(),
                        error: String::new(),
                    })
                    .collect();
            } else {
                run.status = AutomationRunStatus::Failed;
                run.error = final_error;
            }
        });
        if let Err(error) = result {
            eprintln!("updating automation run {run_id} failed: {error}");
        }
    }

    fn running_count(&self, plan_id: &str) -> Result<usize, String> {
        self.inner
            .running
            .lock()
            .map_err(|_| "自动化运行状态锁已损坏".to_owned())
            .map(|running| running.get(plan_id).copied().unwrap_or(0))
    }

    fn try_acquire(&self, plan_id: &str, allow_parallel: bool) -> Result<bool, String> {
        let mut running = self
            .inner
            .running
            .lock()
            .map_err(|_| "自动化运行状态锁已损坏".to_owned())?;
        let count = running.get(plan_id).copied().unwrap_or(0);
        if count > 0 && !allow_parallel {
            return Ok(false);
        }
        running.insert(plan_id.to_owned(), count + 1);
        Ok(true)
    }

    fn release(&self, plan_id: &str) {
        if let Ok(mut running) = self.inner.running.lock()
            && let Some(count) = running.get_mut(plan_id)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                running.remove(plan_id);
            }
        }
    }

    fn mark_running(&self, workspace_root: &Path, run_id: &str) -> Result<(), String> {
        self.update_history(workspace_root, run_id, |run| {
            run.status = AutomationRunStatus::Running;
            run.started_at = Some(now_millis());
            run.attempt = 1;
        })
    }

    fn finish_with_error(
        &self,
        workspace_root: &Path,
        run_id: &str,
        error: String,
    ) -> Result<(), String> {
        self.update_history(workspace_root, run_id, |run| {
            run.status = AutomationRunStatus::Failed;
            run.finished_at = Some(now_millis());
            run.error = error;
        })
    }

    fn append_history(&self, workspace_root: &Path, run: AutomationRun) -> Result<(), String> {
        let _guard = self
            .inner
            .io_lock
            .lock()
            .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
        let mut store = load_history_store(workspace_root)?;
        store.runs.insert(0, run);
        store.runs.truncate(MAX_HISTORY_ENTRIES);
        save_history_store(workspace_root, &store)
    }

    fn update_history(
        &self,
        workspace_root: &Path,
        run_id: &str,
        update: impl FnOnce(&mut AutomationRun),
    ) -> Result<(), String> {
        let _guard = self
            .inner
            .io_lock
            .lock()
            .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
        let mut store = load_history_store(workspace_root)?;
        let run = store
            .runs
            .iter_mut()
            .find(|run| run.id == run_id)
            .ok_or_else(|| "自动化运行记录不存在".to_owned())?;
        update(run);
        save_history_store(workspace_root, &store)
    }
}

impl Drop for SchedulerManager {
    fn drop(&mut self) {
        // The state instance managed by Tauri is the owner. Scheduler/run-thread
        // clones are deliberately non-owning, so dropping one cannot stop service.
        if self.stop_on_drop {
            self.stop();
        }
    }
}

struct RunningGuard {
    manager: SchedulerManager,
    plan_id: String,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.manager.release(&self.plan_id);
    }
}

#[tauri::command]
pub(crate) fn list_automation_plans(
    paths: State<'_, AppPaths>,
    manager: State<'_, SchedulerManager>,
) -> Result<Vec<AutomationPlan>, String> {
    let _guard = manager
        .inner
        .io_lock
        .lock()
        .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
    let mut store = load_plan_store(&paths.workspace_root)?;
    let now = now_millis();
    for plan in &mut store.plans {
        plan.next_run_at = next_run_at(plan, now);
    }
    store
        .plans
        .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    save_plan_store(&paths.workspace_root, &store)?;
    Ok(store.plans)
}

#[tauri::command]
pub(crate) fn create_automation_plan(
    input: AutomationPlanInput,
    paths: State<'_, AppPaths>,
    manager: State<'_, SchedulerManager>,
) -> Result<AutomationPlan, String> {
    validate_input(&input)?;
    let _guard = manager
        .inner
        .io_lock
        .lock()
        .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
    let mut store = load_plan_store(&paths.workspace_root)?;
    let now = now_millis();
    let mut plan = AutomationPlan {
        id: format!("automation-{}", Uuid::new_v4().simple()),
        name: input.name.trim().to_owned(),
        description: input.description.trim().to_owned(),
        enabled: input.enabled,
        schedule: input.schedule,
        action: input.action,
        concurrency_policy: input.concurrency_policy,
        retry_policy: input.retry_policy,
        timeout_seconds: input.timeout_seconds,
        delivery_targets: normalize_delivery_targets(input.delivery_targets),
        created_at: now,
        updated_at: now,
        last_run_at: None,
        next_run_at: None,
        last_scheduled_minute: None,
    };
    plan.next_run_at = next_run_at(&plan, now);
    store.plans.push(plan.clone());
    save_plan_store(&paths.workspace_root, &store)?;
    Ok(plan)
}

#[tauri::command]
pub(crate) fn update_automation_plan(
    input: AutomationPlanInput,
    paths: State<'_, AppPaths>,
    manager: State<'_, SchedulerManager>,
) -> Result<AutomationPlan, String> {
    validate_identifier(&input.id, "自动化计划")?;
    validate_input(&input)?;
    let _guard = manager
        .inner
        .io_lock
        .lock()
        .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
    let mut store = load_plan_store(&paths.workspace_root)?;
    let plan = store
        .plans
        .iter_mut()
        .find(|plan| plan.id == input.id)
        .ok_or_else(|| "自动化计划不存在".to_owned())?;
    plan.name = input.name.trim().to_owned();
    plan.description = input.description.trim().to_owned();
    plan.enabled = input.enabled;
    plan.schedule = input.schedule;
    plan.action = input.action;
    plan.concurrency_policy = input.concurrency_policy;
    plan.retry_policy = input.retry_policy;
    plan.timeout_seconds = input.timeout_seconds;
    plan.delivery_targets = normalize_delivery_targets(input.delivery_targets);
    plan.updated_at = now_millis();
    plan.next_run_at = next_run_at(plan, plan.updated_at);
    let updated = plan.clone();
    save_plan_store(&paths.workspace_root, &store)?;
    Ok(updated)
}

#[tauri::command]
pub(crate) fn delete_automation_plan(
    plan_id: String,
    paths: State<'_, AppPaths>,
    manager: State<'_, SchedulerManager>,
) -> Result<(), String> {
    validate_identifier(&plan_id, "自动化计划")?;
    if manager.running_count(&plan_id)? > 0 {
        return Err("计划正在运行，暂时不能删除".to_owned());
    }
    let _guard = manager
        .inner
        .io_lock
        .lock()
        .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
    let mut store = load_plan_store(&paths.workspace_root)?;
    let old_len = store.plans.len();
    store.plans.retain(|plan| plan.id != plan_id);
    if old_len == store.plans.len() {
        return Err("自动化计划不存在".to_owned());
    }
    save_plan_store(&paths.workspace_root, &store)
}

#[tauri::command]
pub(crate) fn set_automation_plan_enabled(
    plan_id: String,
    enabled: bool,
    paths: State<'_, AppPaths>,
    manager: State<'_, SchedulerManager>,
) -> Result<AutomationPlan, String> {
    validate_identifier(&plan_id, "自动化计划")?;
    let _guard = manager
        .inner
        .io_lock
        .lock()
        .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
    let mut store = load_plan_store(&paths.workspace_root)?;
    let plan = store
        .plans
        .iter_mut()
        .find(|plan| plan.id == plan_id)
        .ok_or_else(|| "自动化计划不存在".to_owned())?;
    plan.enabled = enabled;
    plan.updated_at = now_millis();
    plan.next_run_at = next_run_at(plan, plan.updated_at);
    let updated = plan.clone();
    save_plan_store(&paths.workspace_root, &store)?;
    Ok(updated)
}

#[tauri::command]
pub(crate) fn run_automation_plan_now(
    plan_id: String,
    app: AppHandle,
    paths: State<'_, AppPaths>,
    manager: State<'_, SchedulerManager>,
) -> Result<AutomationRun, String> {
    validate_identifier(&plan_id, "自动化计划")?;
    let plan = {
        let _guard = manager
            .inner
            .io_lock
            .lock()
            .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
        let mut store = load_plan_store(&paths.workspace_root)?;
        let plan = store
            .plans
            .iter_mut()
            .find(|plan| plan.id == plan_id)
            .ok_or_else(|| "自动化计划不存在".to_owned())?;
        plan.last_run_at = Some(now_millis());
        plan.updated_at = now_millis();
        let plan = plan.clone();
        save_plan_store(&paths.workspace_root, &store)?;
        plan
    };
    manager.launch(
        SchedulerContext::from_app(&app),
        plan,
        AutomationTrigger::Manual,
    )
}

#[tauri::command]
pub(crate) fn list_automation_runs(
    plan_id: Option<String>,
    limit: Option<usize>,
    paths: State<'_, AppPaths>,
    manager: State<'_, SchedulerManager>,
) -> Result<Vec<AutomationRun>, String> {
    if let Some(plan_id) = plan_id.as_deref() {
        validate_identifier(plan_id, "自动化计划")?;
    }
    let _guard = manager
        .inner
        .io_lock
        .lock()
        .map_err(|_| "自动化计划存储锁已损坏".to_owned())?;
    let store = load_history_store(&paths.workspace_root)?;
    let limit = limit.unwrap_or(100).clamp(1, MAX_HISTORY_ENTRIES);
    Ok(store
        .runs
        .into_iter()
        .filter(|run| {
            plan_id
                .as_ref()
                .is_none_or(|plan_id| run.plan_id == *plan_id)
        })
        .take(limit)
        .collect())
}

fn validate_input(input: &AutomationPlanInput) -> Result<(), String> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("计划名称应为 1 到 {MAX_NAME_CHARS} 个字符"));
    }
    if input.description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(format!("计划描述不能超过 {MAX_DESCRIPTION_CHARS} 个字符"));
    }
    validate_schedule(&input.schedule)?;
    if input.action.kind != "rpaz-package" {
        return Err("当前自动化计划仅支持 RPAZ 包任务".to_owned());
    }
    validate_identifier(&input.action.package_id, "RPAZ 包")?;
    if input.action.entrypoint.len() > 200 {
        return Err("RPAZ 入口点过长".to_owned());
    }
    if input.timeout_seconds == 0 || input.timeout_seconds > MAX_TIMEOUT_SECONDS {
        return Err(format!("超时时间应为 1 到 {MAX_TIMEOUT_SECONDS} 秒"));
    }
    if input.retry_policy.max_attempts == 0
        || input.retry_policy.max_attempts > MAX_RETRY_ATTEMPTS
        || input.retry_policy.delay_seconds > MAX_TIMEOUT_SECONDS
        || !input.retry_policy.backoff_multiplier.is_finite()
        || input.retry_policy.backoff_multiplier < 1.0
        || input.retry_policy.backoff_multiplier > 10.0
    {
        return Err(format!(
            "重试次数应为 1 到 {MAX_RETRY_ATTEMPTS}，重试延迟不能超过 {MAX_TIMEOUT_SECONDS} 秒，退避倍数应为 1 到 10"
        ));
    }
    let serialized = serde_json::to_vec(&input.action.parameters).map_err(|e| e.to_string())?;
    if serialized.len() > 1024 * 1024 {
        return Err("任务参数不能超过 1 MB".to_owned());
    }
    for target in &input.delivery_targets {
        if !target.id.is_empty() {
            validate_identifier(&target.id, "投递目标")?;
        }
        if target.name.trim().is_empty() || target.name.chars().count() > MAX_NAME_CHARS {
            return Err("投递目标名称无效".to_owned());
        }
        if target.kind.trim().is_empty() || target.kind.len() > 64 {
            return Err("投递目标类型无效".to_owned());
        }
    }
    Ok(())
}

fn validate_schedule(schedule: &AutomationSchedule) -> Result<(), String> {
    match schedule.kind {
        ScheduleKind::Cron => {
            parse_cron(&schedule.cron)?;
        }
        ScheduleKind::Daily => {
            parse_time(&schedule.time)?;
        }
        ScheduleKind::Weekly => {
            parse_time(&schedule.time)?;
            if schedule.days_of_week.is_empty() || schedule.days_of_week.iter().any(|day| *day > 6)
            {
                return Err("每周计划至少选择一天，星期值应位于 0（周日）到 6（周六）".to_owned());
            }
        }
        ScheduleKind::Interval => {
            if schedule.interval_minutes == 0 || schedule.interval_minutes > 525_600 {
                return Err("间隔分钟数应为 1 到 525600".to_owned());
            }
        }
    }
    Ok(())
}

fn normalize_delivery_targets(mut targets: Vec<DeliveryTarget>) -> Vec<DeliveryTarget> {
    for target in &mut targets {
        if target.id.is_empty() {
            target.id = format!("delivery-{}", Uuid::new_v4().simple());
        }
        target.name = target.name.trim().to_owned();
        target.kind = target.kind.trim().to_owned();
    }
    targets
}

fn schedule_matches(plan: &AutomationPlan, now: DateTime<Local>) -> bool {
    match plan.schedule.kind {
        ScheduleKind::Cron => parse_cron(&plan.schedule.cron).is_ok_and(|cron| cron.matches(now)),
        ScheduleKind::Daily => parse_time(&plan.schedule.time)
            .is_ok_and(|(hour, minute)| now.hour() == hour && now.minute() == minute),
        ScheduleKind::Weekly => parse_time(&plan.schedule.time).is_ok_and(|(hour, minute)| {
            now.hour() == hour
                && now.minute() == minute
                && plan
                    .schedule
                    .days_of_week
                    .contains(&(now.weekday().num_days_from_sunday() as u8))
        }),
        ScheduleKind::Interval => {
            let interval = i64::from(plan.schedule.interval_minutes) * 60_000;
            let elapsed = minute_bucket(now.timestamp_millis()) - minute_bucket(plan.created_at);
            interval > 0 && elapsed >= interval && elapsed % interval == 0
        }
    }
}

fn should_trigger_scheduled(plan: &AutomationPlan, now: DateTime<Local>) -> bool {
    plan.enabled
        && plan.last_scheduled_minute != Some(minute_bucket(now.timestamp_millis()))
        && schedule_matches(plan, now)
}

fn next_run_at(plan: &AutomationPlan, after_millis: i64) -> Option<i64> {
    if !plan.enabled {
        return None;
    }
    let start_minute = minute_bucket(after_millis).saturating_add(60_000);
    // Five years covers leap years and sparse annual cron expressions while
    // keeping malformed/impossible schedules bounded.
    let max_minutes = 5 * 366 * 24 * 60;
    for offset in 0..max_minutes {
        let timestamp = start_minute.saturating_add(offset * 60_000);
        let Some(candidate) = Local.timestamp_millis_opt(timestamp).single() else {
            continue;
        };
        if schedule_matches(plan, candidate) {
            return Some(timestamp);
        }
    }
    None
}

fn parse_time(value: &str) -> Result<(u32, u32), String> {
    let (hour, minute) = value
        .trim()
        .split_once(':')
        .ok_or_else(|| "时间应使用 HH:MM 格式".to_owned())?;
    let hour = hour.parse::<u32>().map_err(|_| "小时无效".to_owned())?;
    let minute = minute.parse::<u32>().map_err(|_| "分钟无效".to_owned())?;
    if hour > 23 || minute > 59 {
        return Err("时间超出有效范围".to_owned());
    }
    Ok((hour, minute))
}

#[derive(Clone, Debug)]
struct CronExpression {
    minutes: Vec<bool>,
    hours: Vec<bool>,
    days_of_month: Vec<bool>,
    months: Vec<bool>,
    days_of_week: Vec<bool>,
    day_of_month_wildcard: bool,
    day_of_week_wildcard: bool,
}

impl CronExpression {
    fn matches(&self, value: DateTime<Local>) -> bool {
        let ordinary_fields_match = self.minutes[value.minute() as usize]
            && self.hours[value.hour() as usize]
            && self.months[value.month() as usize];
        if !ordinary_fields_match {
            return false;
        }
        let day_of_month_matches = self.days_of_month[value.day() as usize];
        let day_of_week_matches =
            self.days_of_week[value.weekday().num_days_from_sunday() as usize];
        let day_matches = match (self.day_of_month_wildcard, self.day_of_week_wildcard) {
            (true, true) => true,
            (true, false) => day_of_week_matches,
            (false, true) => day_of_month_matches,
            (false, false) => day_of_month_matches || day_of_week_matches,
        };
        day_matches
    }
}

fn parse_cron(value: &str) -> Result<CronExpression, String> {
    let fields: Vec<_> = value.split_whitespace().collect();
    if fields.len() != 5 {
        return Err("Cron 表达式必须包含 5 段：分 时 日 月 星期".to_owned());
    }
    let (minutes, _) = parse_cron_field(fields[0], 0, 59, false)?;
    let (hours, _) = parse_cron_field(fields[1], 0, 23, false)?;
    let (days_of_month, day_of_month_wildcard) = parse_cron_field(fields[2], 1, 31, false)?;
    let (months, _) = parse_cron_field(fields[3], 1, 12, false)?;
    let (days_of_week_raw, day_of_week_wildcard) = parse_cron_field(fields[4], 0, 7, true)?;
    let mut days_of_week = vec![false; 7];
    for (index, selected) in days_of_week_raw.into_iter().enumerate() {
        if selected {
            days_of_week[index % 7] = true;
        }
    }
    Ok(CronExpression {
        minutes,
        hours,
        days_of_month,
        months,
        days_of_week,
        day_of_month_wildcard,
        day_of_week_wildcard,
    })
}

fn parse_cron_field(
    field: &str,
    minimum: u32,
    maximum: u32,
    sunday_alias: bool,
) -> Result<(Vec<bool>, bool), String> {
    let wildcard = field == "*";
    let mut selected = vec![false; maximum as usize + 1];
    for component in field.split(',') {
        if component.is_empty() {
            return Err("Cron 字段包含空项".to_owned());
        }
        let (range, step) = component
            .split_once('/')
            .map_or((component, 1), |(range, step)| {
                (range, step.parse::<u32>().unwrap_or(0))
            });
        if step == 0 {
            return Err("Cron 步长必须大于 0".to_owned());
        }
        let (start, end) = if range == "*" {
            (minimum, maximum)
        } else if let Some((start, end)) = range.split_once('-') {
            (
                parse_cron_number(start, minimum, maximum, sunday_alias)?,
                parse_cron_number(end, minimum, maximum, sunday_alias)?,
            )
        } else {
            let value = parse_cron_number(range, minimum, maximum, sunday_alias)?;
            (value, value)
        };
        if start > end {
            return Err("Cron 范围起点不能大于终点".to_owned());
        }
        let mut value = start;
        while value <= end {
            selected[value as usize] = true;
            let Some(next) = value.checked_add(step) else {
                break;
            };
            value = next;
        }
    }
    if !selected.iter().any(|selected| *selected) {
        return Err("Cron 字段没有选择任何值".to_owned());
    }
    Ok((selected, wildcard))
}

fn parse_cron_number(
    value: &str,
    minimum: u32,
    maximum: u32,
    _sunday_alias: bool,
) -> Result<u32, String> {
    let value = value
        .parse::<u32>()
        .map_err(|_| format!("Cron 数值无效：{value}"))?;
    if value < minimum || value > maximum {
        return Err(format!(
            "Cron 数值 {value} 超出 {minimum} 到 {maximum} 的范围"
        ));
    }
    Ok(value)
}

fn retry_delay(policy: &RetryPolicy, completed_attempts: u32) -> u64 {
    let exponent = completed_attempts.saturating_sub(1) as i32;
    let multiplier = policy.backoff_multiplier.powi(exponent);
    ((policy.delay_seconds as f64) * multiplier)
        .min(MAX_TIMEOUT_SECONDS as f64)
        .round() as u64
}

fn automation_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("automations")
}

fn plans_path(workspace_root: &Path) -> PathBuf {
    automation_root(workspace_root).join("plans.json")
}

fn history_path(workspace_root: &Path) -> PathBuf {
    automation_root(workspace_root).join("history.json")
}

fn load_plan_store(workspace_root: &Path) -> Result<PlanStore, String> {
    let mut store: PlanStore = read_json_or_default(&plans_path(workspace_root))?;
    if store.schema == 0 {
        store.schema = STORE_SCHEMA;
    }
    if store.schema != STORE_SCHEMA {
        return Err(format!("不支持的自动化计划数据版本：{}", store.schema));
    }
    Ok(store)
}

fn save_plan_store(workspace_root: &Path, store: &PlanStore) -> Result<(), String> {
    write_json_atomic(&plans_path(workspace_root), store)
}

fn load_history_store(workspace_root: &Path) -> Result<HistoryStore, String> {
    let mut store: HistoryStore = read_json_or_default(&history_path(workspace_root))?;
    if store.schema == 0 {
        store.schema = STORE_SCHEMA;
    }
    if store.schema != STORE_SCHEMA {
        return Err(format!("不支持的自动化历史数据版本：{}", store.schema));
    }
    Ok(store)
}

fn save_history_store(workspace_root: &Path, store: &HistoryStore) -> Result<(), String> {
    write_json_atomic(&history_path(workspace_root), store)
}

fn read_json_or_default<T>(path: &Path) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Default,
{
    recover_atomic_backup(path)?;
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("读取 {} 失败：{error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(format!("读取 {} 失败：{error}", path.display())),
    }
}

fn recover_atomic_backup(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let prefix = format!(".{file_name}.");
    let mut backups = match fs::read_dir(parent) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&prefix) && name.ends_with(".bak")
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("检查自动化数据备份失败：{error}")),
    };
    backups.sort_by_key(|entry| {
        entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    if let Some(backup) = backups.pop() {
        fs::rename(backup.path(), path)
            .map_err(|error| format!("恢复自动化数据备份失败：{error}"))?;
    }
    Ok(())
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "自动化存储路径无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建自动化数据目录失败：{error}"))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4().simple()
    ));
    let backup = parent.join(format!(
        ".{}.{}.bak",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4().simple()
    ));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("序列化自动化数据失败：{error}"))?;
    let mut temporary_file =
        File::create(&temporary).map_err(|error| format!("创建自动化临时数据失败：{error}"))?;
    temporary_file
        .write_all(&bytes)
        .and_then(|_| temporary_file.sync_all())
        .map_err(|error| format!("写入自动化临时数据失败：{error}"))?;
    drop(temporary_file);

    let had_original = path.exists();
    if had_original {
        if let Err(error) = fs::rename(path, &backup) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("备份原自动化数据失败：{error}"));
        }
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_original {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("提交自动化数据失败：{error}"));
    }
    if had_original {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!("{label}标识无效"));
    }
    Ok(())
}

fn minute_bucket(timestamp_millis: i64) -> i64 {
    timestamp_millis.div_euclid(60_000) * 60_000
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

const fn default_timeout_seconds() -> u64 {
    300
}

const fn default_max_attempts() -> u32 {
    1
}

const fn default_retry_delay_seconds() -> u64 {
    10
}

const fn default_backoff_multiplier() -> f64 {
    2.0
}

const fn default_true() -> bool {
    true
}

fn default_action_kind() -> String {
    "rpaz-package".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(schedule: AutomationSchedule, created_at: i64) -> AutomationPlan {
        AutomationPlan {
            id: "automation-test".to_owned(),
            name: "测试计划".to_owned(),
            description: String::new(),
            enabled: true,
            schedule,
            action: AutomationAction {
                kind: "rpaz-package".to_owned(),
                package_id: "com.drpa.test".to_owned(),
                entrypoint: String::new(),
                parameters: serde_json::json!({}),
            },
            concurrency_policy: ConcurrencyPolicy::Skip,
            retry_policy: RetryPolicy::default(),
            timeout_seconds: 300,
            delivery_targets: Vec::new(),
            created_at,
            updated_at: created_at,
            last_run_at: None,
            next_run_at: None,
            last_scheduled_minute: None,
        }
    }

    fn local_time(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .expect("test local date should be unambiguous")
    }

    #[test]
    fn five_field_cron_supports_lists_ranges_steps_and_sunday_alias() {
        let cron = parse_cron("*/15 8-10 * 1,7 1-5").unwrap();
        assert!(cron.matches(local_time(2026, 7, 29, 9, 30)));
        assert!(!cron.matches(local_time(2026, 7, 29, 9, 31)));
        assert!(!cron.matches(local_time(2026, 7, 26, 9, 30)));

        let sunday = parse_cron("0 8 * * 7").unwrap();
        assert!(sunday.matches(local_time(2026, 7, 26, 8, 0)));
        assert!(parse_cron("* * * *").is_err());
        assert!(parse_cron("60 * * * *").is_err());
        assert!(parse_cron("*/0 * * * *").is_err());
    }

    #[test]
    fn cron_uses_standard_day_of_month_or_week_semantics() {
        let cron = parse_cron("0 9 1 * 1").unwrap();
        assert!(cron.matches(local_time(2026, 7, 1, 9, 0)));
        assert!(cron.matches(local_time(2026, 7, 6, 9, 0)));
        assert!(!cron.matches(local_time(2026, 7, 7, 9, 0)));
    }

    #[test]
    fn daily_weekly_and_interval_schedules_match_at_minute_precision() {
        let daily_at = local_time(2026, 7, 29, 8, 45);
        let daily = plan(
            AutomationSchedule {
                kind: ScheduleKind::Daily,
                cron: String::new(),
                time: "08:45".to_owned(),
                days_of_week: Vec::new(),
                interval_minutes: 0,
            },
            daily_at.timestamp_millis(),
        );
        assert!(schedule_matches(&daily, daily_at));

        let weekly = plan(
            AutomationSchedule {
                kind: ScheduleKind::Weekly,
                cron: String::new(),
                time: "08:45".to_owned(),
                days_of_week: vec![3],
                interval_minutes: 0,
            },
            daily_at.timestamp_millis(),
        );
        assert!(schedule_matches(&weekly, daily_at));

        let interval = plan(
            AutomationSchedule {
                kind: ScheduleKind::Interval,
                cron: String::new(),
                time: String::new(),
                days_of_week: Vec::new(),
                interval_minutes: 15,
            },
            daily_at.timestamp_millis(),
        );
        assert!(!schedule_matches(&interval, daily_at));
        assert!(schedule_matches(
            &interval,
            daily_at + chrono::Duration::minutes(30)
        ));
        assert!(!schedule_matches(
            &interval,
            daily_at + chrono::Duration::minutes(31)
        ));
    }

    #[test]
    fn persisted_minute_marker_prevents_duplicate_scheduled_trigger() {
        let current = local_time(2026, 7, 29, 8, 45);
        let mut daily = plan(
            AutomationSchedule {
                kind: ScheduleKind::Daily,
                cron: String::new(),
                time: "08:45".to_owned(),
                days_of_week: Vec::new(),
                interval_minutes: 0,
            },
            current.timestamp_millis() - 60_000,
        );
        assert!(should_trigger_scheduled(&daily, current));
        daily.last_scheduled_minute = Some(minute_bucket(current.timestamp_millis()));
        assert!(!should_trigger_scheduled(&daily, current));
    }

    #[test]
    fn next_run_skips_current_minute_and_disabled_plan_has_none() {
        let current = local_time(2026, 7, 29, 8, 45);
        let mut daily = plan(
            AutomationSchedule {
                kind: ScheduleKind::Daily,
                cron: String::new(),
                time: "08:45".to_owned(),
                days_of_week: Vec::new(),
                interval_minutes: 0,
            },
            current.timestamp_millis(),
        );
        let next = next_run_at(&daily, current.timestamp_millis()).unwrap();
        assert_eq!(
            Local
                .timestamp_millis_opt(next)
                .single()
                .unwrap()
                .date_naive(),
            (current + chrono::Duration::days(1)).date_naive()
        );
        daily.enabled = false;
        assert_eq!(next_run_at(&daily, current.timestamp_millis()), None);
    }

    #[test]
    fn stores_are_workspace_isolated_and_replace_json_atomically() {
        let root =
            std::env::temp_dir().join(format!("drpa-automation-test-{}", Uuid::new_v4().simple()));
        let workspace_a = root.join("a");
        let workspace_b = root.join("b");
        let created = now_millis();
        let store_a = PlanStore {
            schema: STORE_SCHEMA,
            plans: vec![plan(
                AutomationSchedule {
                    kind: ScheduleKind::Cron,
                    cron: "0 9 * * *".to_owned(),
                    time: String::new(),
                    days_of_week: Vec::new(),
                    interval_minutes: 0,
                },
                created,
            )],
        };
        save_plan_store(&workspace_a, &store_a).unwrap();
        assert_eq!(load_plan_store(&workspace_a).unwrap().plans.len(), 1);
        assert!(load_plan_store(&workspace_b).unwrap().plans.is_empty());

        let replacement = PlanStore {
            schema: STORE_SCHEMA,
            plans: Vec::new(),
        };
        save_plan_store(&workspace_a, &replacement).unwrap();
        assert!(load_plan_store(&workspace_a).unwrap().plans.is_empty());
        let simulated_crash_backup =
            automation_root(&workspace_a).join(".plans.json.simulated-crash.bak");
        fs::rename(plans_path(&workspace_a), &simulated_crash_backup).unwrap();
        assert!(load_plan_store(&workspace_a).unwrap().plans.is_empty());
        assert!(plans_path(&workspace_a).is_file());
        assert!(
            fs::read_dir(automation_root(&workspace_a))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plan_input_validation_catches_unsafe_or_impossible_values() {
        let mut input = AutomationPlanInput {
            id: String::new(),
            name: "每日报告".to_owned(),
            description: String::new(),
            enabled: true,
            schedule: AutomationSchedule {
                kind: ScheduleKind::Cron,
                cron: "0 9 * * 1-5".to_owned(),
                time: String::new(),
                days_of_week: Vec::new(),
                interval_minutes: 0,
            },
            action: AutomationAction {
                kind: "rpaz-package".to_owned(),
                package_id: "com.drpa.report".to_owned(),
                entrypoint: String::new(),
                parameters: serde_json::json!({"topic": "news"}),
            },
            concurrency_policy: ConcurrencyPolicy::Queue,
            retry_policy: RetryPolicy::default(),
            timeout_seconds: 300,
            delivery_targets: vec![DeliveryTarget {
                id: String::new(),
                kind: "webhook".to_owned(),
                name: "报告频道".to_owned(),
                enabled: true,
                configuration: BTreeMap::new(),
            }],
        };
        assert!(validate_input(&input).is_ok());
        input.schedule.cron = "broken".to_owned();
        assert!(validate_input(&input).is_err());
        input.schedule.cron = "0 9 * * *".to_owned();
        input.action.kind = "shell".to_owned();
        assert!(validate_input(&input).is_err());
    }

    #[test]
    fn retry_delay_honors_exponential_backoff() {
        let policy = RetryPolicy {
            max_attempts: 4,
            delay_seconds: 5,
            backoff_multiplier: 2.0,
        };
        assert_eq!(retry_delay(&policy, 1), 5);
        assert_eq!(retry_delay(&policy, 2), 10);
        assert_eq!(retry_delay(&policy, 3), 20);
    }

    #[test]
    fn running_slots_enforce_exclusive_and_parallel_policies() {
        let manager = SchedulerManager::default();
        assert_eq!(manager.try_acquire("plan-a", false).unwrap(), true);
        assert_eq!(manager.try_acquire("plan-a", false).unwrap(), false);
        assert_eq!(manager.try_acquire("plan-a", true).unwrap(), true);
        assert_eq!(manager.running_count("plan-a").unwrap(), 2);
        manager.release("plan-a");
        manager.release("plan-a");
        assert_eq!(manager.running_count("plan-a").unwrap(), 0);
    }

    #[test]
    fn only_managed_owner_drop_stops_scheduler_state() {
        let owner = SchedulerManager::default();
        let observer = owner.clone();
        let worker = owner.clone();
        drop(worker);
        assert!(!observer.inner.stop.load(Ordering::Acquire));
        drop(owner);
        assert!(observer.inner.stop.load(Ordering::Acquire));
    }
}
