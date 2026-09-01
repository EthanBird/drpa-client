use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use drpa_install::ComponentLeaseGuard;
use drpa_package::{Entrypoint, PackageManifest, safe_relative_path};
use globset::Glob;
use ignore::WalkBuilder;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(test)]
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::{
    AppPaths, RunProcessManager, agent_browser::AgentBrowserSession, agent_config, agent_documents,
    agent_extensions, agent_loop_guard::ToolLoopGuard, agent_runtime::AgentRunControl,
    credential_vault, database, dispatch_run_background, knowledge, knowledge_base, plugins,
};
use drpa_host::HostState;

const DEFAULT_MAX_AGENT_ROUNDS: usize = 64;
const MAX_CONFIGURABLE_AGENT_ROUNDS: usize = 256;
const MAX_HISTORY_MESSAGES: usize = 120;
const MAX_PROVIDER_ATTEMPTS: usize = 4;
const MAX_COMPACTION_SOURCE_BYTES: usize = 256 * 1024;
const MAX_COMPACTION_SUMMARY_BYTES: usize = 32 * 1024;
const MAX_MESSAGE_BYTES: usize = 100_000;
const MAX_TOOL_OUTPUT_BYTES: usize = 20_000;
const MAX_PYTHON_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const PYTHON_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FILE_READ_LINES: usize = 2_000;
const MAX_FILE_SCAN_ENTRIES: usize = 100_000;
const MAX_FILE_TOOL_RESULTS: usize = 500;
const DEFAULT_PYTHON_TIMEOUT_SECONDS: u64 = 300;
const MAX_PYTHON_TIMEOUT_SECONDS: u64 = 86_400;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentTurnRequest {
    pub request_id: String,
    #[serde(default)]
    pub session_id: String,
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_agent_mode")]
    pub mode: String,
    #[serde(default = "default_database_dialect")]
    pub database_dialect: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub provider_ref: Option<AgentProviderRef>,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default = "default_max_agent_rounds")]
    pub max_rounds: usize,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_python_timeout_seconds")]
    pub python_timeout_seconds: u64,
    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls: usize,
    #[serde(default = "default_max_wall_time_seconds")]
    pub max_wall_time_seconds: u64,
    #[serde(default)]
    pub selected_skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_policy: AgentToolPolicy,
    #[serde(default)]
    pub context_checkpoint: Option<AgentContextCheckpoint>,
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentContextCheckpoint {
    pub checkpoint_id: String,
    pub summary: String,
    pub covers_messages: usize,
    pub source_digest: String,
    pub created_at: u64,
    pub estimated_tokens: u64,
    pub method: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentProviderRef {
    pub plugin_id: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct AgentToolPolicy {
    pub enabled: bool,
    pub database_read: bool,
    pub database_connections: bool,
    pub knowledge_base_read: bool,
    pub document_read: bool,
    pub document_write: bool,
    pub document_convert: bool,
    pub arbitrary_file_read: bool,
    pub file_read_scope: AgentFileReadScope,
    pub project_write: bool,
    pub python: bool,
    pub workspace_write: bool,
    pub extensions: bool,
    pub browser: bool,
    pub rpaz_runs: bool,
    pub run_records: bool,
    pub vault_read: bool,
    pub vault_write: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AgentFileReadScope {
    Project,
    System,
}

impl Default for AgentFileReadScope {
    fn default() -> Self {
        Self::System
    }
}

impl Default for AgentToolPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            database_read: true,
            database_connections: true,
            knowledge_base_read: true,
            document_read: true,
            document_write: true,
            document_convert: true,
            arbitrary_file_read: true,
            file_read_scope: AgentFileReadScope::System,
            project_write: true,
            python: true,
            workspace_write: true,
            extensions: true,
            browser: true,
            rpaz_runs: true,
            run_records: true,
            vault_read: true,
            vault_write: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentToolEvent {
    pub call_id: String,
    pub name: String,
    pub status: String,
    pub summary: String,
    pub output: String,
    pub ordinal: usize,
    pub round: usize,
    pub input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum AgentStreamEvent {
    Started {
        run_id: String,
        session_id: String,
    },
    RoundStarted {
        round: usize,
    },
    ContextAssembled {
        round: usize,
        estimated_tokens: u64,
        omitted_messages: usize,
        omitted_tools: usize,
    },
    Retrying {
        round: usize,
        attempt: usize,
        max_attempts: usize,
        delay_ms: u64,
        error: String,
    },
    ContextCompacted {
        round: usize,
        checkpoint: AgentContextCheckpoint,
    },
    Delta {
        content: String,
    },
    ContentReplace {
        content: String,
    },
    Tool {
        tool: AgentToolEvent,
    },
    Completed {
        run_id: String,
        usage: AgentUsage,
        duration_ms: u64,
        stop_reason: String,
    },
    Failed {
        run_id: String,
        error: String,
    },
    Cancelled {
        run_id: String,
    },
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentTurnResult {
    pub message: String,
    pub tools: Vec<AgentToolEvent>,
    pub usage: AgentUsage,
    pub duration_ms: u64,
    pub stop_reason: String,
    pub rounds: usize,
    pub tool_calls: usize,
    pub retry_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_checkpoint: Option<AgentContextCheckpoint>,
}

#[derive(Clone)]
struct AgentContext {
    workspace_root: PathBuf,
    project_root: Option<PathBuf>,
    context_files: Vec<PathBuf>,
    python: PathBuf,
    package_overlay: PathBuf,
    browser: Option<PathBuf>,
    browser_session: Option<AgentBrowserSession>,
    component_leases: Vec<ComponentLeaseGuard>,
    runtime_features: Vec<String>,
    host: Option<AgentHostContext>,
    session_id: String,
    python_timeout: Duration,
    selected_skill_ids: Vec<String>,
    tool_policy: AgentToolPolicy,
    control: AgentRunControl,
}

#[derive(Clone)]
pub(crate) struct AgentHostContext {
    state: HostState,
    paths: AppPaths,
    processes: RunProcessManager,
    vault: credential_vault::CredentialVaultManager,
    agent_scope: Option<AgentHostScope>,
}

#[derive(Clone)]
struct AgentHostScope {
    session_id: String,
    python: PathBuf,
    control: AgentRunControl,
}

impl AgentHostContext {
    pub(crate) fn new(
        state: HostState,
        paths: AppPaths,
        processes: RunProcessManager,
        vault: credential_vault::CredentialVaultManager,
    ) -> Self {
        Self {
            state,
            paths,
            processes,
            vault,
            agent_scope: None,
        }
    }

    pub(crate) fn with_agent_scope(
        mut self,
        session_id: &str,
        python: &Path,
        control: AgentRunControl,
    ) -> Self {
        self.agent_scope = Some(AgentHostScope {
            session_id: session_id.to_owned(),
            python: python.to_path_buf(),
            control,
        });
        self
    }

    pub(crate) fn execute(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        match name {
            "rpaz_list_packages" => {
                let packages = self.state.snapshot().packages;
                Ok(json!({"ok": true, "count": packages.len(), "packages": packages}))
            }
            "rpaz_run_package" => {
                let package_id = argument_string(arguments, "packageId")?;
                let snapshot = self.state.snapshot();
                let package = snapshot
                    .packages
                    .iter()
                    .find(|package| package.id == package_id)
                    .ok_or_else(|| format!("RPAZ 包不存在：{package_id}"))?;
                let requested_profile = argument_optional_string(arguments, "profileId").trim();
                let profile_id = if requested_profile.is_empty() {
                    package
                        .profiles
                        .first()
                        .map(|profile| profile.id.as_str())
                        .unwrap_or("default")
                } else {
                    requested_profile
                };
                let parameters = arguments
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !parameters.is_object() {
                    return Err("parameters 必须是 JSON 对象".to_owned());
                }
                let run_id = dispatch_run_background(
                    self.state.clone(),
                    self.paths.clone(),
                    self.processes.clone(),
                    package_id,
                    profile_id,
                    parameters,
                )?;
                Ok(json!({
                    "ok": true,
                    "runId": run_id,
                    "packageId": package_id,
                    "profileId": profile_id,
                    "message": "任务已通过 DRPA Host 启动，可用 run_get_detail 读取实时事件和调试日志"
                }))
            }
            "run_list" => {
                let limit = arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(50)
                    .clamp(1, 500) as usize;
                let package_id = argument_optional_string(arguments, "packageId").trim();
                let status = argument_optional_string(arguments, "status").trim();
                let runs = self
                    .state
                    .snapshot()
                    .runs
                    .into_iter()
                    .filter(|run| package_id.is_empty() || run.package_id == package_id)
                    .filter(|run| {
                        status.is_empty()
                            || serde_json::to_value(run.status)
                                .ok()
                                .and_then(|value| value.as_str().map(str::to_owned))
                                .is_some_and(|value| value == status)
                    })
                    .take(limit)
                    .collect::<Vec<_>>();
                Ok(json!({"ok": true, "count": runs.len(), "runs": runs}))
            }
            "run_get_detail" => {
                let run_id = argument_string(arguments, "runId")?;
                let detail = self
                    .state
                    .get_run_detail(run_id)
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(detail)
                    .map(|detail| json!({"ok": true, "detail": detail}))
                    .map_err(|error| format!("序列化运行详情失败：{error}"))
            }
            "vault_list_credentials" => {
                let items = self.vault.list_credentials(&self.paths)?;
                serde_json::to_value(items)
                    .map(|items| json!({"ok": true, "items": items}))
                    .map_err(|error| format!("序列化凭据列表失败：{error}"))
            }
            "vault_get_credential" => {
                let id = argument_string(arguments, "id")?;
                let item = self.vault.get_credential(&self.paths, id)?;
                serde_json::to_value(item)
                    .map(|item| json!({"ok": true, "item": item, "sensitive": true}))
                    .map_err(|error| format!("序列化凭据失败：{error}"))
            }
            "vault_upsert_credential" => {
                let input = serde_json::from_value::<credential_vault::VaultCredentialInput>(
                    arguments.clone(),
                )
                .map_err(|error| format!("凭据参数无效：{error}"))?;
                let item = self.vault.save_credential(&self.paths, input)?;
                Ok(
                    json!({"ok": true, "id": item.id, "name": item.name, "updatedAt": item.updated_at}),
                )
            }
            "document_read" => {
                let scope = self
                    .agent_scope
                    .as_ref()
                    .ok_or_else(|| "文档工具缺少 Agent 会话上下文".to_owned())?;
                let document = agent_documents::read_document(
                    &self.paths.workspace_root,
                    &scope.python,
                    &scope.session_id,
                    argument_string(arguments, "documentId")?,
                    &scope.control,
                )?;
                serde_json::to_value(document)
                    .map_err(|error| format!("序列化文档读取结果失败：{error}"))
            }
            "document_create" => {
                let scope = self
                    .agent_scope
                    .as_ref()
                    .ok_or_else(|| "文档工具缺少 Agent 会话上下文".to_owned())?;
                let artifact = agent_documents::create_document(
                    &self.paths.workspace_root,
                    &scope.python,
                    &scope.session_id,
                    argument_string(arguments, "format")?,
                    argument_string(arguments, "title")?,
                    arguments
                        .get("content")
                        .ok_or_else(|| "文档工具缺少 content".to_owned())?,
                    arguments.get("fileName").and_then(Value::as_str),
                    &scope.control,
                )?;
                serde_json::to_value(artifact)
                    .map_err(|error| format!("序列化文档产物失败：{error}"))
            }
            "document_convert" => {
                let scope = self
                    .agent_scope
                    .as_ref()
                    .ok_or_else(|| "文档工具缺少 Agent 会话上下文".to_owned())?;
                let artifact = agent_documents::convert_document(
                    &self.paths.workspace_root,
                    &scope.python,
                    &scope.session_id,
                    argument_string(arguments, "documentId")?,
                    argument_string(arguments, "targetFormat")?,
                    arguments.get("title").and_then(Value::as_str),
                    arguments.get("fileName").and_then(Value::as_str),
                    &scope.control,
                )?;
                serde_json::to_value(artifact)
                    .map_err(|error| format!("序列化文档转换结果失败：{error}"))
            }
            _ => Err(format!("未知 DRPA Host 工具：{name}")),
        }
    }
}

#[derive(Clone)]
struct ToolResult {
    output: Value,
    summary: String,
}

#[derive(Clone)]
struct ToolExecutionRecord {
    fingerprint: String,
    call_id: String,
    result: Result<ToolResult, String>,
}

struct ToolDispatch {
    result: Result<ToolResult, String>,
    reused_from: Option<String>,
}

#[derive(Default)]
struct ToolExecutionLedger {
    last: Option<ToolExecutionRecord>,
}

impl ToolExecutionLedger {
    fn dispatch<F>(&mut self, fingerprint: String, call_id: &str, execute: F) -> ToolDispatch
    where
        F: FnOnce() -> Result<ToolResult, String>,
    {
        if let Some(previous) = self
            .last
            .as_ref()
            .filter(|previous| previous.fingerprint == fingerprint)
        {
            return ToolDispatch {
                result: previous.result.clone(),
                reused_from: Some(previous.call_id.clone()),
            };
        }
        let result = execute();
        self.last = Some(ToolExecutionRecord {
            fingerprint,
            call_id: call_id.to_owned(),
            result: result.clone(),
        });
        ToolDispatch {
            result,
            reused_from: None,
        }
    }
}

struct ToolRegistry<'a> {
    context: &'a AgentContext,
    definitions: Vec<Value>,
    descriptors: HashMap<String, crate::agent_tools::ToolCapabilityDescriptor>,
}

impl<'a> ToolRegistry<'a> {
    fn discover(context: &'a AgentContext) -> Result<Self, String> {
        let definitions = agent_tool_definitions(
            &context.workspace_root,
            context.project_root.is_some(),
            context
                .project_root
                .as_ref()
                .is_some_and(|root| root.join("manifest.yaml").is_file()),
            &context.selected_skill_ids,
            context.python_timeout.as_secs(),
            &context.tool_policy,
            &context.runtime_features,
        )?;
        let descriptors = definitions
            .iter()
            .map(crate::agent_tools::ToolCapabilityDescriptor::from_openai_definition)
            .map(|result| result.map(|descriptor| (descriptor.name.clone(), descriptor)))
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(Self {
            context,
            definitions,
            descriptors,
        })
    }

    fn execute(&self, name: &str, arguments: &Value) -> Result<ToolResult, String> {
        self.context.control.check()?;
        let descriptor = self
            .descriptors
            .get(name)
            .ok_or_else(|| format!("工具 {name} 未在当前 Run 中注册"))?;
        crate::agent_tools::CapabilityAuthority::new(&self.context.tool_policy)
            .authorize(descriptor)?;
        descriptor.validate_arguments(arguments)?;
        let result = execute_tool(self.context, name, arguments);
        self.context.control.check()?;
        result
    }

    fn redact_persistent_output(&self, name: &str) -> bool {
        self.descriptors
            .get(name)
            .is_some_and(crate::agent_tools::ToolCapabilityDescriptor::redact_persistent_output)
    }
}

trait ProviderAdapter {
    fn complete(
        &self,
        payload: &Value,
        stream: bool,
        on_delta: &mut dyn FnMut(String),
    ) -> Result<Value, crate::provider::ProviderError>;
}

struct OpenAiCompatibleAdapter {
    profile: crate::provider::ProviderProfile,
    control: AgentRunControl,
}

impl OpenAiCompatibleAdapter {
    fn new(base_url: &str, api_key: &str, control: AgentRunControl) -> Result<Self, String> {
        Ok(Self {
            profile: crate::provider::ProviderProfile::openai(base_url, api_key)?,
            control,
        })
    }
}

impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn complete(
        &self,
        payload: &Value,
        stream: bool,
        on_delta: &mut dyn FnMut(String),
    ) -> Result<Value, crate::provider::ProviderError> {
        crate::provider::complete_cancellable_detailed(
            &self.profile,
            payload,
            stream,
            &self.control,
            on_delta,
        )
    }
}

fn wait_before_retry(control: &AgentRunControl, delay: Duration) -> Result<(), String> {
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        control.check()?;
        thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50)),
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn complete_with_retry<F>(
    provider: &dyn ProviderAdapter,
    payload: &Value,
    stream: bool,
    round: usize,
    control: &AgentRunControl,
    stable_content: &mut String,
    retry_count: &mut usize,
    emit: &mut F,
) -> Result<Value, String>
where
    F: FnMut(AgentStreamEvent),
{
    for attempt in 1..=MAX_PROVIDER_ATTEMPTS {
        control.check()?;
        let mut attempt_content = String::new();
        let result = provider.complete(payload, stream, &mut |content| {
            attempt_content.push_str(&content);
            emit(AgentStreamEvent::Delta { content });
        });
        match result {
            Ok(response) => {
                stable_content.push_str(&attempt_content);
                return Ok(response);
            }
            Err(error) if error.retryable && attempt < MAX_PROVIDER_ATTEMPTS => {
                *retry_count = retry_count.saturating_add(1);
                emit(AgentStreamEvent::ContentReplace {
                    content: stable_content.clone(),
                });
                let exponential = 500_u64.saturating_mul(1_u64 << (attempt - 1).min(3));
                let jitter = u64::from(rand::random::<u16>() % 251);
                let delay = Duration::from_millis(exponential.saturating_add(jitter));
                emit(AgentStreamEvent::Retrying {
                    round,
                    attempt: attempt + 1,
                    max_attempts: MAX_PROVIDER_ATTEMPTS,
                    delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    error: error.message,
                });
                wait_before_retry(control, delay)?;
            }
            Err(error) => return Err(error.message),
        }
    }
    Err("Provider 重试预算已耗尽".to_owned())
}

fn message_prefix_digest(messages: &[AgentMessage], count: usize) -> String {
    let mut digest = Sha256::new();
    for message in messages.iter().take(count) {
        digest.update(message.role.as_bytes());
        digest.update([0]);
        digest.update(message.content.as_bytes());
        digest.update([0xff]);
    }
    format!("{:x}", digest.finalize())
}

fn valid_context_checkpoint<'a>(
    checkpoint: Option<&'a AgentContextCheckpoint>,
    messages: &[AgentMessage],
) -> Option<&'a AgentContextCheckpoint> {
    checkpoint.filter(|checkpoint| {
        checkpoint.covers_messages <= messages.len()
            && !checkpoint.summary.trim().is_empty()
            && checkpoint.source_digest
                == message_prefix_digest(messages, checkpoint.covers_messages)
    })
}

fn compaction_source(
    previous: Option<&AgentContextCheckpoint>,
    messages: &[AgentMessage],
    start: usize,
    end: usize,
) -> String {
    let mut records = Vec::new();
    let mut bytes = previous.map_or(0, |checkpoint| checkpoint.summary.len());
    for (index, message) in messages[start..end].iter().enumerate().rev() {
        let content = truncate_text(&message.content, 8_000);
        let record = format!(
            "[消息 {} · {}]\n{}",
            start + index + 1,
            message.role,
            content
        );
        if !records.is_empty() && bytes.saturating_add(record.len()) > MAX_COMPACTION_SOURCE_BYTES {
            break;
        }
        bytes = bytes.saturating_add(record.len());
        records.push(record);
    }
    records.reverse();
    let previous = previous
        .map(|checkpoint| format!("上一版结构化检查点：\n{}\n\n", checkpoint.summary))
        .unwrap_or_default();
    format!(
        "{previous}需要合并的新对话与执行证据：\n{}",
        records.join("\n\n")
    )
}

fn fallback_compaction_summary(source: &str) -> String {
    let source = truncate_text(source, MAX_COMPACTION_SUMMARY_BYTES.saturating_sub(512));
    format!(
        "## 会话恢复检查点（确定性降级）\n\n模型压缩暂时不可用。以下是经过大小限制的原始历史尾部，后续运行不得假定被省略部分已完成：\n\n{source}"
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_context_checkpoint<F>(
    request: &AgentTurnRequest,
    provider: &dyn ProviderAdapter,
    history_start: usize,
    control: &AgentRunControl,
    stable_content: &mut String,
    retry_count: &mut usize,
    emit: &mut F,
) -> (Option<AgentContextCheckpoint>, usize)
where
    F: FnMut(AgentStreamEvent),
{
    if history_start == 0 {
        return (None, 0);
    }
    let previous = valid_context_checkpoint(request.context_checkpoint.as_ref(), &request.messages);
    if let Some(checkpoint) = previous
        && checkpoint.covers_messages >= history_start
    {
        return (Some(checkpoint.clone()), checkpoint.covers_messages);
    }
    let source_start = previous.map_or(0, |checkpoint| checkpoint.covers_messages);
    let source = compaction_source(previous, &request.messages, source_start, history_start);
    let max_tokens =
        u32::try_from((request.context_window / 32).clamp(1_024, 8_192)).unwrap_or(8_192);
    let payload = json!({
        "model": request.model.trim(),
        "messages": [
            {
                "role": "system",
                "content": "你是 Agent Harness 的上下文压缩器。只根据给定历史生成结构化恢复检查点，必须保留：用户目标与约束、已确认事实、关键决定、已完成工作、文件/数据/工具证据、失败及原因、未完成事项和下一步。不要继续任务，不要虚构。使用简洁 Markdown。"
            },
            {"role": "user", "content": source.clone()}
        ],
        "temperature": 0.1,
        "max_tokens": max_tokens,
        "stream": false
    });
    let (summary, method) = match complete_with_retry(
        provider,
        &payload,
        false,
        1,
        control,
        stable_content,
        retry_count,
        emit,
    )
    .and_then(|response| {
        let assistant = response
            .pointer("/choices/0/message")
            .ok_or_else(|| "上下文压缩响应缺少 choices[0].message".to_owned())?;
        let summary = message_content(assistant.get("content"));
        if summary.trim().is_empty() {
            Err("上下文压缩返回空检查点".to_owned())
        } else {
            Ok(truncate_text(&summary, MAX_COMPACTION_SUMMARY_BYTES))
        }
    }) {
        Ok(summary) => (summary, "model".to_owned()),
        Err(_) => (
            fallback_compaction_summary(&source),
            "deterministic-fallback".to_owned(),
        ),
    };
    let source_digest = message_prefix_digest(&request.messages, history_start);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let checkpoint = AgentContextCheckpoint {
        checkpoint_id: format!("ctx-{}-{now}", &source_digest[..16]),
        estimated_tokens: u64::try_from(summary.len().saturating_add(3) / 4).unwrap_or(u64::MAX),
        summary,
        covers_messages: history_start,
        source_digest,
        created_at: u64::try_from(now).unwrap_or(u64::MAX),
        method,
    };
    emit(AgentStreamEvent::ContextCompacted {
        round: 1,
        checkpoint: checkpoint.clone(),
    });
    (Some(checkpoint), history_start)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_agent_turn<F>(
    mut request: AgentTurnRequest,
    workspace_root: PathBuf,
    resource_dir: Option<PathBuf>,
    python: PathBuf,
    package_overlay: PathBuf,
    browser: Option<PathBuf>,
    browser_session: Option<AgentBrowserSession>,
    component_leases: Vec<ComponentLeaseGuard>,
    runtime_features: Vec<String>,
    host: AgentHostContext,
    control: AgentRunControl,
    mut emit: F,
) -> Result<AgentTurnResult, String>
where
    F: FnMut(AgentStreamEvent),
{
    if let Some(provider_ref) = &request.provider_ref {
        let provider = plugins::resolve_agent_provider(
            &workspace_root,
            &provider_ref.plugin_id,
            &provider_ref.provider_id,
        )?;
        request.base_url = provider.base_url;
        request.model = provider.model;
        request.api_key = provider.api_key;
    }
    validate_request(&request)?;
    control.check()?;
    if request.mode == "developer" {
        return crate::jcode::run_turn(
            &request,
            &workspace_root,
            resource_dir.as_deref(),
            &python,
            browser.as_deref(),
            browser_session.as_ref(),
            host,
            control,
            emit,
        );
    }
    let started = Instant::now();
    let provider =
        OpenAiCompatibleAdapter::new(&request.base_url, &request.api_key, control.clone())?;
    let sql_mode = request.mode == "sql";
    let project_context = if sql_mode || request.project_id.trim().is_empty() {
        None
    } else {
        crate::agent_sessions::resolve_agent_project_context(
            &workspace_root,
            &request.project_id,
            &request.session_id,
        )?
    };
    let project_root = project_context.as_ref().map(|project| project.root.clone());
    let context_files = project_context
        .map(|project| project.context_files)
        .unwrap_or_default();
    let context = AgentContext {
        workspace_root,
        project_root,
        context_files,
        python,
        package_overlay,
        browser,
        browser_session,
        component_leases,
        runtime_features,
        host: Some(host),
        session_id: request.session_id.clone(),
        python_timeout: Duration::from_secs(request.python_timeout_seconds),
        selected_skill_ids: request.selected_skill_ids.clone(),
        tool_policy: request.tool_policy.clone(),
        control: control.clone(),
    };

    let tool_registry = if sql_mode || !request.tool_policy.enabled {
        None
    } else {
        Some(ToolRegistry::discover(&context)?)
    };
    let (system, tools) = if sql_mode {
        (sql_system_prompt(&request.database_dialect), Vec::new())
    } else {
        let mut injected_context = agent_config::render_agent_context(
            &context.workspace_root,
            context.project_root.as_deref(),
            &request.selected_skill_ids,
        )?;
        injected_context.push_str(&render_session_project_context(
            context.project_root.as_deref(),
            &context.context_files,
        ));
        (
            system_prompt(
                context.project_root.is_some(),
                context.tool_policy.file_read_scope == AgentFileReadScope::System,
                &injected_context,
            ),
            tool_registry
                .as_ref()
                .map(|registry| registry.definitions.clone())
                .unwrap_or_default(),
        )
    };
    let history_start = select_history_start(&request, &system, &tools)?;
    let mut retry_count = 0usize;
    let mut stable_stream_content = String::new();
    let (context_checkpoint, effective_history_start) = prepare_context_checkpoint(
        &request,
        &provider,
        history_start,
        &control,
        &mut stable_stream_content,
        &mut retry_count,
        &mut emit,
    );
    let mut messages = vec![json!({
        "role": "system",
        "content": system,
    })];
    if let Some(checkpoint) = &context_checkpoint {
        messages.push(json!({
            "role": "system",
            "content": format!(
                "以下是较早会话的结构化恢复检查点。它只概括检查点覆盖范围；后续原始消息优先级更高。\n\n{}",
                checkpoint.summary
            )
        }));
    }
    for message in &request.messages[effective_history_start..] {
        messages.push(json!({"role": message.role, "content": message.content}));
    }

    let mut events = Vec::new();
    let mut usage = AgentUsage::default();
    let mut tool_calls_count = 0usize;
    let mut tool_event_ordinal = 0usize;
    let mut tool_loop_guard = ToolLoopGuard::default();
    let mut tool_ledger = ToolExecutionLedger::default();
    let mut rounds_completed = 0usize;
    let mut stop_reason = "round-limit".to_owned();

    'rounds: for round in 0..request.max_rounds {
        control.check()?;
        rounds_completed = round + 1;
        emit(AgentStreamEvent::RoundStarted { round: round + 1 });
        let assembled = crate::agent_context::assemble_round_context(
            &messages,
            &tools,
            request.context_window,
            request.max_output_tokens,
        );
        emit(AgentStreamEvent::ContextAssembled {
            round: round + 1,
            estimated_tokens: assembled.estimated_tokens,
            omitted_messages: assembled.omitted_messages,
            omitted_tools: assembled.omitted_tools,
        });
        let active_tools = assembled.tools;
        let mut payload = json!({
            "model": request.model.trim(),
            "messages": assembled.messages,
            "temperature": request.temperature,
            "max_tokens": request.max_output_tokens,
        });
        if !request.session_id.trim().is_empty() {
            payload["user"] = Value::String(request.session_id.trim().to_owned());
        }
        if !active_tools.is_empty() {
            payload["tools"] = Value::Array(active_tools);
            payload["tool_choice"] = Value::String("auto".to_owned());
        }
        if request.stream {
            payload["stream"] = Value::Bool(true);
        }
        let response = complete_with_retry(
            &provider,
            &payload,
            request.stream,
            round + 1,
            &control,
            &mut stable_stream_content,
            &mut retry_count,
            &mut emit,
        )?;
        control.check()?;
        accumulate_usage(&mut usage, response.get("usage"));
        let assistant = response
            .pointer("/choices/0/message")
            .cloned()
            .ok_or_else(|| "模型响应缺少 choices[0].message".to_owned())?;
        let tool_calls = assistant
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        messages.push(assistant.clone());

        if tool_calls.is_empty() {
            let message = message_content(assistant.get("content"));
            if message.trim().is_empty() {
                return Err("模型返回了空消息".to_owned());
            }
            return Ok(AgentTurnResult {
                message,
                tools: events,
                usage,
                duration_ms: elapsed_ms(started),
                stop_reason: "completed".to_owned(),
                rounds: rounds_completed,
                tool_calls: tool_calls_count,
                retry_count,
                context_checkpoint,
            });
        }

        let mut stop_after_tool_batch = false;
        for call in tool_calls {
            control.check()?;
            tool_event_ordinal = tool_event_ordinal.saturating_add(1);
            let ordinal = tool_event_ordinal;
            let tool_round = round + 1;
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_owned();
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            if stop_after_tool_batch {
                let error = format!("本轮工具循环已停止，未执行后续调用：{name}");
                let (tool_event, output_text) =
                    failed_tool_call(&call_id, &name, error, ordinal, tool_round, String::new());
                events.push(tool_event.clone());
                emit(AgentStreamEvent::Tool { tool: tool_event });
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output_text,
                }));
                continue;
            }
            if tool_calls_count >= request.max_tool_calls {
                stop_reason = "tool-call-limit".to_owned();
                stop_after_tool_batch = true;
                let error = format!(
                    "Agent 工具调用已达到上限 {}，未执行：{name}",
                    request.max_tool_calls
                );
                let (tool_event, output_text) =
                    failed_tool_call(&call_id, &name, error, ordinal, tool_round, String::new());
                events.push(tool_event.clone());
                emit(AgentStreamEvent::Tool { tool: tool_event });
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output_text,
                }));
                continue;
            }
            let arguments = parse_tool_arguments(call.pointer("/function/arguments"))?;
            let persistent_input = persistent_tool_input(&name, &arguments);
            tool_calls_count = tool_calls_count.saturating_add(1);
            let canonical_arguments = canonical_tool_arguments(&arguments).to_string();
            if tool_loop_guard.should_block(&name, &canonical_arguments) {
                let error = format!("检测到工具连续返回相同结果，已停止重复调用：{name}");
                let (tool_event, output_text) = failed_tool_call(
                    &call_id,
                    &name,
                    error,
                    ordinal,
                    tool_round,
                    persistent_input,
                );
                events.push(tool_event.clone());
                emit(AgentStreamEvent::Tool { tool: tool_event });
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output_text,
                }));
                stop_reason = "repeated-tool-call".to_owned();
                stop_after_tool_batch = true;
                continue;
            }
            emit(AgentStreamEvent::Tool {
                tool: AgentToolEvent {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    status: "running".to_owned(),
                    summary: "正在执行操作".to_owned(),
                    output: String::new(),
                    ordinal,
                    round: tool_round,
                    input: persistent_input.clone(),
                    duration_ms: None,
                },
            });
            let tool_started = Instant::now();
            let fingerprint = tool_call_fingerprint(&name, &arguments);
            let dispatched = tool_ledger.dispatch(fingerprint, &call_id, || {
                tool_registry
                    .as_ref()
                    .ok_or_else(|| "当前 Agent 模式没有工具注册表".to_owned())?
                    .execute(&name, &arguments)
            });
            let reused_from = dispatched.reused_from;
            let (status, summary, output) = match dispatched.result {
                Ok(result) => {
                    let summary = if let Some(previous) = reused_from.as_deref() {
                        format!(
                            "重复调用已跳过；沿用 {previous} 的成功结果：{}",
                            result.summary
                        )
                    } else {
                        result.summary
                    };
                    ("completed".to_owned(), summary, result.output)
                }
                Err(error) => (
                    "failed".to_owned(),
                    if let Some(previous) = reused_from.as_deref() {
                        format!("重复失败调用已跳过；沿用 {previous} 的错误：{error}")
                    } else {
                        error.clone()
                    },
                    json!({"ok": false, "error": error}),
                ),
            };
            let output_text = tool_context_output(
                &name,
                &call_id,
                &status,
                &summary,
                &output,
                reused_from.as_deref(),
            );
            let guard_output = truncate_text(&output.to_string(), MAX_TOOL_OUTPUT_BYTES);
            tool_loop_guard.record(&name, &canonical_arguments, &status, &guard_output);
            let event_output = if tool_registry
                .as_ref()
                .is_some_and(|registry| registry.redact_persistent_output(&name))
                && status == "completed"
            {
                json!({
                    "ok": true,
                    "redacted": true,
                    "message": if name == "vault_get_credential" {
                        "凭据内容只传给当前模型回合，不写入持久化工具事件"
                    } else {
                        "文档正文只传给当前模型回合，不写入持久化工具事件"
                    }
                })
                .to_string()
            } else {
                output_text.clone()
            };
            let tool_event = AgentToolEvent {
                call_id: call_id.clone(),
                name: name.clone(),
                status,
                summary,
                output: event_output,
                ordinal,
                round: tool_round,
                input: persistent_input,
                duration_ms: Some(elapsed_ms(tool_started)),
            };
            events.push(tool_event.clone());
            emit(AgentStreamEvent::Tool { tool: tool_event });
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output_text,
            }));
        }
        if stop_after_tool_batch {
            break 'rounds;
        }
    }

    control.check()?;
    let final_instruction = if stop_reason == "repeated-tool-call" {
        "检测到工具在没有产生新结果的情况下重复调用，运行已自动熔断。请直接基于已有证据给出最终答复，不再调用工具，并说明尚未完成的事项。"
    } else {
        "工具预算已经结束。请基于已有工具证据直接给出最终答复，不再调用任何工具；说明已完成内容与仍待处理项。"
    };
    messages.push(json!({
        "role": "system",
        "content": final_instruction
    }));
    let final_round = rounds_completed.saturating_add(1);
    let assembled = crate::agent_context::assemble_round_context(
        &messages,
        &[],
        request.context_window,
        request.max_output_tokens,
    );
    emit(AgentStreamEvent::RoundStarted { round: final_round });
    emit(AgentStreamEvent::ContextAssembled {
        round: final_round,
        estimated_tokens: assembled.estimated_tokens,
        omitted_messages: assembled.omitted_messages,
        omitted_tools: assembled.omitted_tools,
    });
    let mut payload = json!({
        "model": request.model.trim(),
        "messages": assembled.messages,
        "temperature": request.temperature,
        "max_tokens": request.max_output_tokens,
    });
    if !request.session_id.trim().is_empty() {
        payload["user"] = Value::String(request.session_id.trim().to_owned());
    }
    if request.stream {
        payload["stream"] = Value::Bool(true);
    }
    let response = complete_with_retry(
        &provider,
        &payload,
        request.stream,
        final_round,
        &control,
        &mut stable_stream_content,
        &mut retry_count,
        &mut emit,
    )?;
    control.check()?;
    accumulate_usage(&mut usage, response.get("usage"));
    let assistant = response
        .pointer("/choices/0/message")
        .ok_or_else(|| "模型最终答复缺少 choices[0].message".to_owned())?;
    let message = message_content(assistant.get("content"));
    if message.trim().is_empty() {
        return Err("模型最终答复为空".to_owned());
    }
    Ok(AgentTurnResult {
        message,
        tools: events,
        usage,
        duration_ms: elapsed_ms(started),
        stop_reason,
        rounds: rounds_completed.saturating_add(1),
        tool_calls: tool_calls_count,
        retry_count,
        context_checkpoint,
    })
}

const fn default_context_window() -> u32 {
    393_216
}

fn default_agent_mode() -> String {
    "rpaz".to_owned()
}

fn default_database_dialect() -> String {
    "sqlite".to_owned()
}

const fn default_max_output_tokens() -> u32 {
    98_304
}

const fn default_max_agent_rounds() -> usize {
    DEFAULT_MAX_AGENT_ROUNDS
}

const fn default_temperature() -> f32 {
    0.2
}

const fn default_python_timeout_seconds() -> u64 {
    DEFAULT_PYTHON_TIMEOUT_SECONDS
}

const fn default_max_tool_calls() -> usize {
    128
}

const fn default_max_wall_time_seconds() -> u64 {
    900
}

pub(crate) fn agent_stream_event_name(request_id: &str) -> Result<String, String> {
    if request_id.len() < 4
        || request_id.len() > 96
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("Agent 请求标识无效".to_owned());
    }
    Ok(format!("agent-stream-{request_id}"))
}

fn validate_request(request: &AgentTurnRequest) -> Result<(), String> {
    agent_stream_event_name(&request.request_id)?;
    if !matches!(request.mode.as_str(), "rpaz" | "sql" | "developer") {
        return Err("Agent 模式无效".to_owned());
    }
    if !matches!(
        request.database_dialect.as_str(),
        "sqlite" | "postgresql" | "mysql"
    ) {
        return Err("数据库方言无效".to_owned());
    }
    if request.model.trim().is_empty() || request.model.len() > 200 {
        return Err("请填写有效的模型名称".to_owned());
    }
    if !(1_024..=2_000_000).contains(&request.context_window) {
        return Err("上下文窗口必须在 1024 到 2000000 tokens 之间".to_owned());
    }
    if !(64..=131_072).contains(&request.max_output_tokens)
        || request.max_output_tokens >= request.context_window
    {
        return Err("最大输出 tokens 必须小于上下文窗口，且位于 64 到 131072 之间".to_owned());
    }
    if !(1..=MAX_CONFIGURABLE_AGENT_ROUNDS).contains(&request.max_rounds) {
        return Err(format!(
            "Agent 最大模型/工具循环必须在 1 到 {MAX_CONFIGURABLE_AGENT_ROUNDS} 之间"
        ));
    }
    if !request.temperature.is_finite() || !(0.0..=2.0).contains(&request.temperature) {
        return Err("Temperature 必须在 0 到 2 之间".to_owned());
    }
    if !(1..=MAX_PYTHON_TIMEOUT_SECONDS).contains(&request.python_timeout_seconds) {
        return Err(format!(
            "Python 超时必须在 1 到 {MAX_PYTHON_TIMEOUT_SECONDS} 秒之间"
        ));
    }
    if !(1..=4_096).contains(&request.max_tool_calls) {
        return Err("Agent 最大工具调用次数必须在 1 到 4096 之间".to_owned());
    }
    if !(10..=86_400).contains(&request.max_wall_time_seconds) {
        return Err("Agent 最大运行时长必须在 10 到 86400 秒之间".to_owned());
    }
    if request.selected_skill_ids.len() > 32
        || request.selected_skill_ids.iter().any(|name| {
            name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err("所选 Skills 无效或超过 32 个".to_owned());
    }
    if request.messages.is_empty() {
        return Err("Agent 消息不能为空".to_owned());
    }
    for message in &request.messages {
        if !matches!(message.role.as_str(), "user" | "assistant") {
            return Err("Agent 历史只接受 user/assistant 消息".to_owned());
        }
        if message.content.len() > MAX_MESSAGE_BYTES {
            return Err("单条 Agent 消息过长".to_owned());
        }
    }
    Ok(())
}

fn select_history_start(
    request: &AgentTurnRequest,
    system: &str,
    tools: &[Value],
) -> Result<usize, String> {
    let overhead = estimate_tokens(system)
        .saturating_add(estimate_tokens(&Value::Array(tools.to_vec()).to_string()))
        .saturating_add(512);
    let available = (request.context_window as usize)
        .saturating_sub(request.max_output_tokens as usize)
        .saturating_sub(overhead);
    let minimum_start = request.messages.len().saturating_sub(MAX_HISTORY_MESSAGES);
    let mut used = 0usize;
    let mut start = request.messages.len();
    for index in (minimum_start..request.messages.len()).rev() {
        let cost = estimate_tokens(&request.messages[index].content).saturating_add(8);
        if used.saturating_add(cost) > available {
            if start == request.messages.len() {
                return Err(format!(
                    "上下文窗口不足以容纳最新消息；请增大上下文窗口或减小最大输出 tokens（当前可用约 {available} tokens）"
                ));
            }
            break;
        }
        used = used.saturating_add(cost);
        start = index;
    }
    Ok(start)
}

fn estimate_tokens(value: &str) -> usize {
    let mut ascii = 0usize;
    let mut non_ascii = 0usize;
    for character in value.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else {
            non_ascii += 1;
        }
    }
    ascii.div_ceil(4).saturating_add(non_ascii).max(1)
}

fn message_content(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_tool_arguments(value: Option<&Value>) -> Result<Value, String> {
    match value {
        Some(Value::String(source)) if source.trim().is_empty() => Ok(json!({})),
        Some(Value::String(source)) => {
            serde_json::from_str(source).map_err(|error| format!("工具参数 JSON 无效：{error}"))
        }
        Some(Value::Object(_)) => Ok(value.cloned().unwrap_or_else(|| json!({}))),
        None | Some(Value::Null) => Ok(json!({})),
        _ => Err("工具参数必须是 JSON 对象".to_owned()),
    }
}

fn failed_tool_call(
    call_id: &str,
    name: &str,
    error: String,
    ordinal: usize,
    round: usize,
    input: String,
) -> (AgentToolEvent, String) {
    let output = json!({"ok": false, "error": error.clone()}).to_string();
    (
        AgentToolEvent {
            call_id: call_id.to_owned(),
            name: name.to_owned(),
            status: "failed".to_owned(),
            summary: error,
            output: output.clone(),
            ordinal,
            round,
            input,
            duration_ms: None,
        },
        output,
    )
}

pub(crate) fn persistent_tool_input(name: &str, arguments: &Value) -> String {
    if matches!(
        name,
        "vault_get_credential" | "vault_upsert_credential" | "document_create"
    ) {
        return json!({"redacted": true, "message": "敏感输入未写入运行记录"}).to_string();
    }
    truncate_text(&redact_argument_value(None, arguments).to_string(), 2_000)
}

fn redact_argument_value(key: Option<&str>, value: &Value) -> Value {
    let sensitive_key = key.is_some_and(|key| {
        let key = key.to_ascii_lowercase();
        [
            "password",
            "secret",
            "token",
            "api_key",
            "apikey",
            "credential",
            "authorization",
        ]
        .iter()
        .any(|candidate| key.contains(candidate))
    });
    if sensitive_key {
        return Value::String("[已隐藏]".to_owned());
    }
    match value {
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), redact_argument_value(Some(key), value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(32)
                .map(|value| redact_argument_value(None, value))
                .collect(),
        ),
        Value::String(value) => Value::String(truncate_text(value, 500)),
        _ => value.clone(),
    }
}

fn canonical_tool_arguments(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(name, _)| *name);
            Value::Object(
                entries
                    .into_iter()
                    .map(|(name, value)| (name.clone(), canonical_tool_arguments(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_tool_arguments).collect()),
        _ => value.clone(),
    }
}

fn tool_call_fingerprint(name: &str, arguments: &Value) -> String {
    format!("{name}:{}", canonical_tool_arguments(arguments))
}

fn tool_output_is_empty(output: &Value) -> bool {
    match output {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(values) => values.is_empty(),
        Value::Object(object) => {
            if object.contains_key("stdout") || object.contains_key("stderr") {
                return ["stdout", "stderr"].iter().all(|name| {
                    object
                        .get(*name)
                        .and_then(Value::as_str)
                        .is_none_or(|value| value.trim().is_empty())
                });
            }
            object.is_empty()
        }
        _ => false,
    }
}

fn tool_context_output(
    name: &str,
    call_id: &str,
    status: &str,
    summary: &str,
    output: &Value,
    reused_from: Option<&str>,
) -> String {
    let serialized = output.to_string();
    let bounded_result = if serialized.len() <= MAX_TOOL_OUTPUT_BYTES.saturating_sub(2_000) {
        output.clone()
    } else {
        json!({
            "truncated": true,
            "text": truncate_text(&serialized, MAX_TOOL_OUTPUT_BYTES.saturating_sub(2_000)),
        })
    };
    let empty_output = tool_output_is_empty(output);
    let mut envelope = json!({
        "tool": name,
        "callId": call_id,
        "status": status,
        "summary": summary,
        "executionPerformed": reused_from.is_none(),
        "emptyOutput": empty_output,
        "result": bounded_result,
    });
    if let Some(previous) = reused_from {
        envelope["reusedFromCallId"] = Value::String(previous.to_owned());
    }
    if status == "completed" && empty_output {
        envelope["observation"] = Value::String(
            "工具已成功完成；空 stdout/stderr 是有效结果，不代表工具尚未执行。请直接继续下一步，避免自动重复调用。"
                .to_owned(),
        );
    }
    truncate_text(&envelope.to_string(), MAX_TOOL_OUTPUT_BYTES)
}

fn accumulate_usage(target: &mut AgentUsage, value: Option<&Value>) {
    let Some(value) = value else { return };
    target.prompt_tokens = target.prompt_tokens.saturating_add(
        value
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    target.completion_tokens = target.completion_tokens.saturating_add(
        value
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
}

fn system_prompt(has_project: bool, system_file_read: bool, injected_context: &str) -> String {
    let context = if has_project {
        "当前已绑定一个通用开发项目；可在项目目录运行 Python，写入始终严格限制在当前项目内。若项目包含 manifest.yaml，则它也是可校验和构建的规范化 RPAZ 项目。"
    } else {
        "当前未绑定项目，项目文件写入和 Python 工具暂不可用。"
    };
    let read_scope = if system_file_read {
        "文件只读范围已配置为整个操作系统：read_file、find_files、search_text 可以使用绝对路径读取当前系统账户有权访问的位置；相对路径仍从当前项目解析，未绑定项目时从 DRPA 工作区解析。"
    } else {
        "文件只读范围已配置为仅当前项目：read_file、find_files、search_text 不得越过项目目录。"
    };
    format!(
        "你是 DRPA Next 内置的通用开发与自动化 Agent。{context}{read_scope}\n\
         RPAZ 是根目录含 manifest.yaml 的 ZIP，当前 schema 为 2；Python 入口实现 main(ctx)，\
         参数来自 ctx.params，产物使用 ctx.output_file，进度使用 ctx.progress。\n\
         你可以使用 data 工具列出数据工作台连接、读取结构并执行 Host 强制的只读查询；\
         data_create_connection 只保存连接元数据，不保存密码。知识文档是可编辑 Markdown，不向量化；\
         知识库是独立的只读混合向量索引，可用 knowledge_base_search 查询。\
         对话附件使用不透明 attachmentId，文档产物严格写入当前工作区的 Agent 产物目录。\
         项目探索优先使用 find_files/search_text，再用 read_file 分段读取所需行；\
         小范围修改优先使用只在项目内生效的 edit_file。你也可以按策略使用 knowledge、document 与扩展工具。\
         浏览器任务使用 browser_* 工具，它连接 DRPA 内置 Chrome 并与 RPAZ ctx.browser() 复用同一持久会话；\
         已安装 RPAZ 包必须用 rpaz_run_package 启动，并用 run_get_detail 查看实时事件与 debug 日志。\n\
         凭据工具只在用户已用 Google Authenticator 验证并解锁保险箱后工作；先列出不含密文的摘要，再按需读取或写入。\n\
         只有规范化 RPAZ 项目才调用 rpaz_validate；需要交付 RPAZ 归档时调用 rpaz_build。\n\
         工具返回 completed 后即代表调用完成；stdout/stderr 为空也是有效结果，不要因此重复相同调用。\
         若宿主返回 executionPerformed=false，直接沿用 reusedFromCallId 的结果继续下一步。\n\
         回答使用简体中文，先给结论，再列出实际完成的文件与验证结果。\
         {injected_context}"
    )
}

pub(crate) fn render_session_project_context(
    project_root: Option<&Path>,
    context_files: &[PathBuf],
) -> String {
    let Some(project_root) = project_root else {
        return String::new();
    };
    let files = context_files
        .iter()
        .map(|path| {
            path.strip_prefix(project_root)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "projectRoot": project_root.to_string_lossy(),
        "initialContextFiles": files,
        "instruction": "把 initialContextFiles 作为本会话的首要文件线索；先按需读取相关区段，再扩展检索同目录项目。"
    });
    format!("\n<session_project_context>{payload}</session_project_context>\n")
}

fn sql_system_prompt(dialect: &str) -> String {
    let (database, identifier) = match dialect {
        "postgresql" => ("PostgreSQL", "双引号"),
        "mysql" => ("MySQL", "反引号"),
        _ => ("SQLite 3", "双引号"),
    };
    format!(
        "你是 DRPA 数据工作台内置的 {database} SQL 助手。只依据用户消息中提供的数据库结构和需求编写 SQL；\
         不虚构表、视图或字段。回答使用简体中文，先给一句简短说明，再给且只给一个标记为 sql 的 Markdown 代码块。\
         SQL 应兼容 {database}，标识符按需使用{identifier}，查询默认添加合理的 LIMIT。\
         涉及 UPDATE、DELETE、DROP、ALTER 等修改或破坏性语句时，必须在说明中明确影响范围，但不要执行 SQL。"
    )
}

fn agent_tool_definitions(
    workspace_root: &Path,
    has_project: bool,
    is_rpaz_project: bool,
    selected_skill_ids: &[String],
    python_timeout_seconds: u64,
    policy: &AgentToolPolicy,
    runtime_features: &[String],
) -> Result<Vec<Value>, String> {
    if !policy.enabled {
        return Ok(Vec::new());
    }
    let mut tools = vec![
        tool_definition(
            "browser_open",
            "在 DRPA 内置 Chrome 中打开 URL。浏览器使用与 RPAZ ctx.browser() 相同的持久化端口和用户目录，任务结束后继续保留。",
            json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"],"additionalProperties":false}),
        ),
        tool_definition(
            "browser_back",
            "让当前 DRPA Chrome 页面后退，并返回新的标题和 URL。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "browser_reload",
            "刷新当前 DRPA Chrome 页面。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "browser_snapshot",
            "读取当前 Chrome 页面的标题、URL、可见文本和可交互元素引用。后续点击和输入使用返回的 ref。",
            json!({"type":"object","properties":{"maxChars":{"type":"integer","minimum":1000,"maximum":100000}},"additionalProperties":false}),
        ),
        tool_definition(
            "browser_click",
            "点击 browser_snapshot 返回的元素引用。",
            json!({"type":"object","properties":{"ref":{"type":"string"}},"required":["ref"],"additionalProperties":false}),
        ),
        tool_definition(
            "browser_type",
            "向 browser_snapshot 返回的输入元素写入文本并触发 input/change 事件。",
            json!({"type":"object","properties":{"ref":{"type":"string"},"text":{"type":"string"},"submit":{"type":"boolean"}},"required":["ref","text"],"additionalProperties":false}),
        ),
        tool_definition(
            "browser_select",
            "在 browser_snapshot 返回的 select 元素中按 value 或可见文字选择选项。",
            json!({"type":"object","properties":{"ref":{"type":"string"},"value":{"type":"string"}},"required":["ref","value"],"additionalProperties":false}),
        ),
        tool_definition(
            "browser_scroll",
            "按指定方向滚动当前页面。",
            json!({"type":"object","properties":{"direction":{"type":"string","enum":["up","down","left","right"]},"amount":{"type":"integer","minimum":100,"maximum":5000}},"additionalProperties":false}),
        ),
        tool_definition(
            "browser_wait",
            "等待页面加载或指定文本出现。",
            json!({"type":"object","properties":{"seconds":{"type":"number","minimum":0,"maximum":120},"text":{"type":"string"}},"additionalProperties":false}),
        ),
        tool_definition(
            "browser_screenshot",
            "截取当前 Chrome 页面并保存到 Agent 会话产物目录。",
            json!({"type":"object","properties":{"fullPage":{"type":"boolean"}},"additionalProperties":false}),
        ),
        tool_definition(
            "browser_status",
            "查看 DRPA Chrome Bridge 的调试端口、当前 URL 和标题。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_list_packages",
            "列出已安装的 RPAZ 包、任务配置和参数定义。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_run_package",
            "通过 DRPA Host 正式运行已安装的 RPAZ 包；运行会进入运行记录并产生实时事件。profileId 省略时使用包的第一个任务配置。",
            json!({"type":"object","properties":{"packageId":{"type":"string"},"profileId":{"type":"string"},"parameters":{"type":"object"}},"required":["packageId"],"additionalProperties":false}),
        ),
        tool_definition(
            "run_list",
            "列出 DRPA 运行记录，可按 RPAZ 包或状态筛选。",
            json!({"type":"object","properties":{"packageId":{"type":"string"},"status":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":500}},"additionalProperties":false}),
        ),
        tool_definition(
            "run_get_detail",
            "读取一条运行记录的完整详情、进度、产物、结构化事件和 debug 日志。",
            json!({"type":"object","properties":{"runId":{"type":"string"}},"required":["runId"],"additionalProperties":false}),
        ),
        tool_definition(
            "vault_list_credentials",
            "列出已解锁的本地凭据保险箱条目摘要，不返回 secret。保险箱锁定时 Host 会要求用户先验证。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "vault_get_credential",
            "按 id 读取一条本地凭据的完整内容。返回值包含敏感 secret，只在完成一次 TOTP 验证后的 24 小时运行期会话中可用。",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}),
        ),
        tool_definition(
            "vault_upsert_credential",
            "创建或更新本地加密凭据。更新时传入 id；支持 login、apiKey、token、database、ssh、secureNote。",
            json!({"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"kind":{"type":"string","enum":["login","apiKey","token","database","ssh","secureNote"]},"username":{"type":"string"},"secret":{"type":"string"},"uri":{"type":"string"},"notes":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}},"favorite":{"type":"boolean"}},"required":["name","kind","secret"],"additionalProperties":false}),
        ),
        tool_definition(
            "agent_list_skills",
            "列出本地 Skills 库的名称和描述。Skill 正文按需读取，不要一次读取全部。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "agent_read_skill",
            "任务与某个 Skill 的描述匹配时，读取该 Skill 的完整 SKILL.md。",
            json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"],"additionalProperties":false}),
        ),
        tool_definition(
            "agent_read_memory",
            "读取本地 Agent 长期记忆 MEMORY.md。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "knowledge_list_documents",
            "列出 DRPA 本地知识库中的 Markdown 文档和目录。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "knowledge_read_document",
            "读取本地知识库中的一篇 UTF-8 Markdown 文档。",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        ),
    ];
    if policy.workspace_write {
        tools.extend([
            tool_definition(
                "agent_write_skill",
                "仅在用户要求保存可复用流程时创建或更新一个标准 SKILL.md。name 使用小写字母、数字和中划线。",
                json!({"type":"object","properties":{"name":{"type":"string"},"content":{"type":"string"}},"required":["name","content"],"additionalProperties":false}),
            ),
            tool_definition(
                "agent_write_memory",
                "仅在用户明确要求记住稳定事实或偏好时更新完整 MEMORY.md；不要写入密钥。",
                json!({"type":"object","properties":{"content":{"type":"string"}},"required":["content"],"additionalProperties":false}),
            ),
            tool_definition(
                "agent_remember",
                "把一条稳定事实、偏好或项目约定追加到结构化记忆事件账本；优先使用该工具，不覆盖整份 MEMORY.md。不要写入密钥。",
                json!({"type":"object","properties":{"category":{"type":"string","enum":["preference","fact","project","workflow"]},"key":{"type":"string"},"value":{"type":"string"},"source":{"type":"string"}},"required":["category","key","value"],"additionalProperties":false}),
            ),
            tool_definition(
                "knowledge_write_document",
                "创建或覆盖本地知识库中的 Markdown 文档；父目录会按安全相对路径创建。",
                json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),
            ),
        ]);
    }
    if policy.database_read {
        tools.extend([
            tool_definition(
                "data_list_connections",
                "列出数据工作台的工作区 SQLite 和已保存连接。连接配置不包含密码。",
                json!({"type":"object","properties":{},"additionalProperties":false}),
            ),
            tool_definition(
                "data_get_schema",
                "只读获取一个数据库连接的结构。connectionId 使用 data_list_connections 返回的 id；远程连接可按需提供 password。",
                json!({"type":"object","properties":{"connectionId":{"type":"string"},"password":{"type":"string"}},"required":["connectionId"],"additionalProperties":false}),
            ),
            tool_definition(
                "data_query",
                "在只读事务中查询数据库。只接受 SELECT、WITH、EXPLAIN、SHOW、DESCRIBE 或 VALUES；Host 会拒绝所有修改数据或结构的 SQL。",
                json!({"type":"object","properties":{"connectionId":{"type":"string"},"sql":{"type":"string"},"password":{"type":"string"}},"required":["connectionId","sql"],"additionalProperties":false}),
            ),
        ]);
    }
    if policy.database_connections {
        tools.push(tool_definition(
            "data_create_connection",
            "在数据工作台创建连接配置，支持 postgresql、mysql、mariadb、sqlite、excel、csv、json；只保存连接元数据，不保存密码，也不测试或修改数据。",
            json!({"type":"object","properties":{"name":{"type":"string"},"engine":{"type":"string","enum":["postgresql","mysql","mariadb","sqlite","excel","csv","json"]},"host":{"type":"string"},"port":{"type":"integer","minimum":0,"maximum":65535},"database":{"type":"string","description":"远程数据库名，或 SQLite/Excel/CSV/JSON 文件绝对路径"},"username":{"type":"string"},"tlsMode":{"type":"string","enum":["disable","prefer","require"]}},"required":["name","engine","database"],"additionalProperties":false}),
        ));
    }
    if policy.knowledge_base_read {
        tools.extend([
            tool_definition(
                "knowledge_base_list",
                "列出当前隔离工作区中的向量知识库。知识库与可编辑知识文档相互独立。",
                json!({"type":"object","properties":{},"additionalProperties":false}),
            ),
            tool_definition(
                "knowledge_base_search",
                "使用本地向量与关键词混合检索查询一个或多个知识库，返回带引用的原文片段。该工具只读，不能导入或修改知识库。",
                json!({"type":"object","properties":{"query":{"type":"string"},"knowledgeBaseIds":{"type":"array","items":{"type":"string"}},"topK":{"type":"integer","minimum":1,"maximum":30}},"required":["query"],"additionalProperties":false}),
            ),
        ]);
    }
    if policy.document_read {
        tools.push(tool_definition(
            "document_read",
            "读取用户已附加到当前对话的 PDF、Word、Excel 或 PowerPoint 文档。只接受附件或当前对话产物的不透明 documentId。",
            json!({"type":"object","properties":{"documentId":{"type":"string"}},"required":["documentId"],"additionalProperties":false}),
        ));
    }
    if policy.document_write {
        tools.push(tool_definition(
            "document_create",
            "根据结构化内容创建 PDF、DOCX、XLSX 或 PPTX；产物只会写入当前工作区的当前对话产物目录。",
            json!({"type":"object","properties":{"format":{"type":"string","enum":["pdf","docx","xlsx","pptx"]},"title":{"type":"string"},"content":{},"fileName":{"type":"string"}},"required":["format","title","content"],"additionalProperties":false}),
        ));
    }
    if policy.document_convert {
        tools.push(tool_definition(
            "document_convert",
            "把当前对话的附件或产物转换为 PDF、DOCX、XLSX 或 PPTX；不会覆盖原文件。",
            json!({"type":"object","properties":{"documentId":{"type":"string"},"targetFormat":{"type":"string","enum":["pdf","docx","xlsx","pptx"]},"title":{"type":"string"},"fileName":{"type":"string"}},"required":["documentId","targetFormat"],"additionalProperties":false}),
        ));
    }
    if policy.arbitrary_file_read
        && (has_project || policy.file_read_scope == AgentFileReadScope::System)
    {
        let system_scope = policy.file_read_scope == AgentFileReadScope::System;
        let read_description = if system_scope {
            "按行读取 UTF-8 文本文件。相对路径从当前项目解析（未绑定项目时从 DRPA 工作区解析），绝对路径可读取当前系统账户有权访问的本机文件；可指定起始行，一次严格不超过 2000 行。"
        } else {
            "按行读取当前项目内的 UTF-8 文本文件；相对或绝对路径都不能越过项目目录。可指定起始行，一次严格不超过 2000 行。"
        };
        let find_description = if system_scope {
            "按 glob 快速查找文件。path 可使用本机绝对目录，省略时从当前项目开始；不跟随符号链接，最多返回 500 项。"
        } else {
            "在当前项目中按 glob 快速查找文件，遵循 .gitignore；最多返回 500 项。"
        };
        let search_description = if system_scope {
            "在文本文件中进行正则或字面量检索。path 可使用本机绝对目录，省略时从当前项目开始；不跟随符号链接，返回文件、行号和匹配行。"
        } else {
            "在当前项目文本文件中进行正则或字面量检索，遵循 .gitignore；返回文件、行号和匹配行。"
        };
        tools.extend([
            tool_definition(
                "read_file",
                read_description,
                json!({"type":"object","properties":{"path":{"type":"string"},"startLine":{"type":"integer","minimum":1},"lineCount":{"type":"integer","minimum":1,"maximum":MAX_FILE_READ_LINES}},"required":["path"],"additionalProperties":false}),
            ),
            tool_definition(
                "find_files",
                find_description,
                json!({"type":"object","properties":{"pattern":{"type":"string","description":"例如 **/*.rs 或 manifest.*"},"path":{"type":"string","description":"起始目录；整个系统范围下允许绝对路径，默认 ."},"limit":{"type":"integer","minimum":1,"maximum":MAX_FILE_TOOL_RESULTS}},"required":["pattern"],"additionalProperties":false}),
            ),
            tool_definition(
                "search_text",
                search_description,
                json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string","description":"起始目录；整个系统范围下允许绝对路径，默认 ."},"glob":{"type":"string","description":"可选文件 glob，例如 **/*.rs"},"literal":{"type":"boolean"},"ignoreCase":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":MAX_FILE_TOOL_RESULTS}},"required":["pattern"],"additionalProperties":false}),
            ),
        ]);
    }
    if has_project {
        tools.push(tool_definition(
            "rpaz_list_files",
            "列出当前通用开发项目的文件。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ));
    }
    if has_project && policy.project_write {
        tools.extend([
            tool_definition(
                "edit_file",
                "在当前项目内精确编辑 UTF-8 文件。oldText 必须且只能匹配一次；Host 禁止项目外写入。",
                json!({"type":"object","properties":{"path":{"type":"string"},"oldText":{"type":"string"},"newText":{"type":"string"}},"required":["path","oldText","newText"],"additionalProperties":false}),
            ),
            tool_definition(
                "rpaz_write_file",
                "创建或覆盖当前项目中的 UTF-8 文本文件。Host 严格禁止写入项目目录之外。",
                json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),
            ),
        ]);
        if is_rpaz_project {
            tools.extend([
                tool_definition(
                    "rpaz_validate",
                    "按 DRPA schema 2 校验 manifest.yaml 和入口文件。",
                    json!({"type":"object","properties":{},"additionalProperties":false}),
                ),
                tool_definition(
                    "rpaz_build",
                    "校验并构建当前项目的 .rpaz 归档。",
                    json!({"type":"object","properties":{},"additionalProperties":false}),
                ),
            ]);
        }
    }
    if has_project && policy.python {
        tools.push(tool_definition(
            "rpaz_python",
            &format!(
                "在当前通用项目目录用 DRPA 内置 Python 执行辅助代码，本次超时为 {python_timeout_seconds} 秒。通用项目无需 RPAZ manifest。"
            ),
            json!({"type":"object","properties":{"code":{"type":"string"}},"required":["code"],"additionalProperties":false}),
        ));
    }
    if policy.extensions {
        tools.extend(agent_config::skill_tool_definitions(
            workspace_root,
            selected_skill_ids,
        )?);
        tools.extend(plugins::plugin_tool_definitions(workspace_root)?);
        tools.extend(agent_extensions::tool_definitions(workspace_root)?);
    }
    if !runtime_features.is_empty()
        && !runtime_features
            .iter()
            .any(|feature| feature == "browser.drissionpage")
    {
        tools.retain(|tool| {
            !tool
                .pointer("/function/name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.starts_with("browser_"))
        });
    }
    if !runtime_features.is_empty()
        && !runtime_features
            .iter()
            .any(|feature| feature == "documents")
    {
        tools.retain(|tool| {
            !tool
                .pointer("/function/name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.starts_with("document_"))
        });
    }
    let authority = crate::agent_tools::CapabilityAuthority::new(policy);
    tools.retain(|definition| {
        definition
            .pointer("/function/name")
            .and_then(Value::as_str)
            .is_some_and(|name| authority.authorize_name(name).is_ok())
    });
    Ok(tools)
}

fn tool_definition(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

fn execute_tool(
    context: &AgentContext,
    name: &str,
    arguments: &Value,
) -> Result<ToolResult, String> {
    if name.starts_with("ext__") {
        let host_context = context.clone();
        let hostcall = Arc::new(move |host_name: &str, host_arguments: &Value| {
            if host_name.starts_with("ext__") {
                return Err("扩展 hostcall 不能递归调用另一个 QuickJS 扩展".to_owned());
            }
            crate::agent_tools::CapabilityAuthority::new(&host_context.tool_policy)
                .authorize_name(host_name)?;
            execute_tool(&host_context, host_name, host_arguments).map(|result| result.output)
        });
        let context_payload = json!({
            "workspace": context.workspace_root,
            "project": context.project_root,
            "sessionId": context.session_id,
        });
        let executed = agent_extensions::execute_tool(
            &context.workspace_root,
            name,
            "drpa-extension-call",
            arguments,
            context_payload,
            hostcall,
        )
        .ok_or_else(|| format!("未知扩展工具：{name}"))??;
        return Ok(ToolResult {
            output: executed.output,
            summary: executed.summary,
        });
    }
    if let Some((skill_name, _)) = name
        .strip_prefix("skill_")
        .and_then(|value| value.split_once("__"))
        && !context.selected_skill_ids.is_empty()
        && !context
            .selected_skill_ids
            .iter()
            .any(|selected| selected == skill_name)
    {
        return Err(format!("Skill {skill_name} 未在当前会话中启用"));
    }
    if let Some(executed) = agent_config::execute_skill_tool(
        &context.workspace_root,
        context.project_root.as_deref(),
        &context.python,
        name,
        arguments,
        &context.control,
    ) {
        let executed = executed?;
        return Ok(ToolResult {
            output: executed.output,
            summary: executed.summary,
        });
    }
    if let Some(executed) = plugins::execute_plugin_tool(
        &context.workspace_root,
        context.project_root.as_deref(),
        &context.python,
        name,
        arguments,
    ) {
        let executed = executed?;
        return Ok(ToolResult {
            output: executed.output,
            summary: executed.summary,
        });
    }
    match name {
        name if name.starts_with("browser_") => {
            return execute_browser_tool(context, name, arguments);
        }
        "rpaz_list_packages"
        | "rpaz_run_package"
        | "run_list"
        | "run_get_detail"
        | "vault_list_credentials"
        | "vault_get_credential"
        | "vault_upsert_credential" => {
            let output = context
                .host
                .as_ref()
                .ok_or_else(|| "DRPA Host 工具桥尚未初始化".to_owned())?
                .execute(name, arguments)?;
            let summary = match name {
                "rpaz_list_packages" => "已读取已安装 RPAZ 包".to_owned(),
                "rpaz_run_package" => format!(
                    "已启动 RPAZ 任务 {}",
                    output
                        .get("runId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                ),
                "run_list" => "已读取运行记录".to_owned(),
                "vault_list_credentials" => "已读取凭据保险箱摘要".to_owned(),
                "vault_get_credential" => "已向当前模型回合提供一条敏感凭据".to_owned(),
                "vault_upsert_credential" => "已更新本地加密凭据".to_owned(),
                _ => "已读取完整运行详情和调试日志".to_owned(),
            };
            return Ok(ToolResult { output, summary });
        }
        "agent_list_skills" => {
            let skills = agent_config::list_skills_for_agent(&context.workspace_root)?;
            let count = skills.len();
            return Ok(ToolResult {
                output: json!({"ok": true, "skills": skills}),
                summary: format!("已列出 {count} 个 Skill"),
            });
        }
        "agent_read_skill" => {
            let name = argument_string(arguments, "name")?;
            let content = agent_config::read_skill_for_agent(&context.workspace_root, name)?;
            return Ok(ToolResult {
                output: json!({"ok": true, "name": name, "content": content}),
                summary: format!("已按需读取 Skill {name}"),
            });
        }
        "agent_write_skill" => {
            let name = argument_string(arguments, "name")?;
            let content = argument_string(arguments, "content")?;
            agent_config::write_skill_for_agent(&context.workspace_root, name, content)?;
            return Ok(ToolResult {
                output: json!({"ok": true, "name": name, "bytes": content.len()}),
                summary: format!("已写入 Skill {name}"),
            });
        }
        "agent_read_memory" => {
            let content = agent_config::read_memory_for_agent(&context.workspace_root)?;
            return Ok(ToolResult {
                output: json!({"ok": true, "content": content}),
                summary: "已读取 Agent 长期记忆".to_owned(),
            });
        }
        "agent_write_memory" => {
            let content = argument_string(arguments, "content")?;
            agent_config::write_memory_for_agent(&context.workspace_root, content)?;
            return Ok(ToolResult {
                output: json!({"ok": true, "bytes": content.len()}),
                summary: "已更新 Agent 长期记忆".to_owned(),
            });
        }
        "agent_remember" => {
            let category = argument_string(arguments, "category")?;
            let key = argument_string(arguments, "key")?;
            let value = argument_string(arguments, "value")?;
            let source = argument_optional_string(arguments, "source");
            let entry = agent_config::append_memory_entry(
                &context.workspace_root,
                category,
                key,
                value,
                source,
            )?;
            return Ok(ToolResult {
                output: entry,
                summary: format!("已追加结构化记忆 {category}/{key}"),
            });
        }
        "knowledge_list_documents" => {
            let entries = knowledge::list_for_agent(&context.workspace_root)?;
            let count = entries.len();
            return Ok(ToolResult {
                output: json!({"ok": true, "entries": entries}),
                summary: format!("已列出 {count} 个知识条目"),
            });
        }
        "knowledge_read_document" => {
            let relative = argument_string(arguments, "path")?;
            let content = knowledge::read_for_agent(&context.workspace_root, relative)?;
            return Ok(ToolResult {
                output: json!({"ok": true, "path": relative, "content": content}),
                summary: format!(
                    "已读取知识文档 {relative}（{} 字符）",
                    content.chars().count()
                ),
            });
        }
        "knowledge_write_document" => {
            let relative = argument_string(arguments, "path")?;
            let content = argument_string(arguments, "content")?;
            knowledge::write_for_agent(&context.workspace_root, relative, content)?;
            return Ok(ToolResult {
                output: json!({"ok": true, "path": relative, "bytes": content.len()}),
                summary: format!("已写入知识文档 {relative}（{} 字节）", content.len()),
            });
        }
        "knowledge_base_list" => {
            let knowledge_bases = knowledge_base::list_for_agent(&context.workspace_root)?;
            let count = knowledge_bases.len();
            return Ok(ToolResult {
                output: json!({"ok": true, "knowledgeBases": knowledge_bases}),
                summary: format!("已列出 {count} 个向量知识库"),
            });
        }
        "knowledge_base_search" => {
            let query = argument_string(arguments, "query")?;
            let knowledge_base_ids = arguments
                .get("knowledgeBaseIds")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let top_k = arguments
                .get("topK")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .clamp(1, 30) as usize;
            let results = knowledge_base::search_for_agent(
                &context.workspace_root,
                &knowledge_base_ids,
                query,
                top_k,
            )?;
            let count = results.len();
            return Ok(ToolResult {
                output: json!({"ok": true, "query": query, "results": results}),
                summary: format!("向量知识库检索完成，返回 {count} 个引用片段"),
            });
        }
        "document_read" => {
            let document_id = argument_string(arguments, "documentId")?;
            let read = agent_documents::read_document(
                &context.workspace_root,
                &context.python,
                &context.session_id,
                document_id,
                &context.control,
            )?;
            return Ok(ToolResult {
                output: serde_json::to_value(&read)
                    .map_err(|error| format!("序列化文档读取结果失败：{error}"))?,
                summary: format!("已读取对话文档 {document_id}"),
            });
        }
        "document_create" => {
            let format = argument_string(arguments, "format")?;
            let title = argument_string(arguments, "title")?;
            let content = arguments
                .get("content")
                .ok_or_else(|| "工具参数缺少 content".to_owned())?;
            let artifact = agent_documents::create_document(
                &context.workspace_root,
                &context.python,
                &context.session_id,
                format,
                title,
                content,
                arguments.get("fileName").and_then(Value::as_str),
                &context.control,
            )?;
            let artifact_name = artifact.name.clone();
            return Ok(ToolResult {
                output: serde_json::to_value(&artifact)
                    .map_err(|error| format!("序列化文档产物失败：{error}"))?,
                summary: format!("已创建文档产物 {artifact_name}"),
            });
        }
        "document_convert" => {
            let document_id = argument_string(arguments, "documentId")?;
            let target_format = argument_string(arguments, "targetFormat")?;
            let artifact = agent_documents::convert_document(
                &context.workspace_root,
                &context.python,
                &context.session_id,
                document_id,
                target_format,
                arguments.get("title").and_then(Value::as_str),
                arguments.get("fileName").and_then(Value::as_str),
                &context.control,
            )?;
            let artifact_name = artifact.name.clone();
            return Ok(ToolResult {
                output: serde_json::to_value(&artifact)
                    .map_err(|error| format!("序列化文档转换结果失败：{error}"))?,
                summary: format!("已转换文档并生成 {artifact_name}"),
            });
        }
        "data_list_connections" => {
            let profiles = database::agent_list_database_profiles(&context.workspace_root)?;
            let count = profiles.len() + 1;
            let mut connections = vec![json!({
                "id": "workspace",
                "name": "工作区数据库",
                "engine": "sqlite",
                "database": context.workspace_root.join("databases/workspace.sqlite3"),
                "readOnly": true
            })];
            connections.extend(profiles.into_iter().map(|profile| {
                json!({
                    "id": profile.id,
                    "name": profile.name,
                    "engine": profile.engine,
                    "host": profile.host,
                    "port": profile.port,
                    "database": profile.database,
                    "username": profile.username,
                    "tlsMode": profile.tls_mode,
                    "readOnly": true
                })
            }));
            return Ok(ToolResult {
                output: json!({"ok": true, "connections": connections}),
                summary: format!("已列出 {count} 个只读数据连接"),
            });
        }
        "data_get_schema" => {
            let connection_id = argument_string(arguments, "connectionId")?;
            let password = argument_optional_string(arguments, "password");
            let schema = tauri::async_runtime::block_on(database::agent_get_database_schema(
                &context.workspace_root,
                connection_id,
                password,
            ))?;
            return Ok(ToolResult {
                output: json!({"ok": true, "connectionId": connection_id, "schema": schema, "readOnly": true}),
                summary: format!("已只读获取连接 {connection_id} 的数据库结构"),
            });
        }
        "data_query" => {
            let connection_id = argument_string(arguments, "connectionId")?;
            let sql = argument_string(arguments, "sql")?;
            let password = argument_optional_string(arguments, "password");
            let result = tauri::async_runtime::block_on(database::agent_execute_read_only_query(
                &context.workspace_root,
                connection_id,
                password,
                sql,
            ))?;
            let row_count = result.rows.len();
            return Ok(ToolResult {
                output: serde_json::to_value(&result)
                    .map_err(|error| format!("序列化查询结果失败：{error}"))?,
                summary: format!("只读查询完成，返回 {row_count} 行"),
            });
        }
        "data_create_connection" => {
            let engine = argument_string(arguments, "engine")?;
            let default_port = match engine {
                "postgresql" => 5432,
                "mysql" | "mariadb" => 3306,
                _ => 0,
            };
            let port = arguments
                .get("port")
                .and_then(Value::as_u64)
                .unwrap_or(default_port)
                .try_into()
                .map_err(|_| "数据库端口无效".to_owned())?;
            let host = argument_optional_string(arguments, "host").trim();
            let saved = database::agent_save_database_profile(
                &context.workspace_root,
                database::RemoteDatabaseProfile {
                    id: String::new(),
                    name: argument_string(arguments, "name")?.to_owned(),
                    engine: engine.to_owned(),
                    host: if host.is_empty() {
                        "localhost".to_owned()
                    } else {
                        host.to_owned()
                    },
                    port,
                    database: argument_string(arguments, "database")?.to_owned(),
                    username: argument_optional_string(arguments, "username").to_owned(),
                    tls_mode: arguments
                        .get("tlsMode")
                        .and_then(Value::as_str)
                        .unwrap_or("prefer")
                        .to_owned(),
                },
            )?;
            let saved_name = saved.name.clone();
            return Ok(ToolResult {
                output: json!({"ok": true, "connection": saved, "passwordStored": false}),
                summary: format!("已创建 {saved_name} 连接配置（未保存密码）"),
            });
        }
        _ => {}
    }
    let allow_system_read = context.tool_policy.file_read_scope == AgentFileReadScope::System;
    let file_root = context
        .project_root
        .as_deref()
        .unwrap_or(context.workspace_root.as_path());
    if name == "read_file" {
        let requested = argument_string(arguments, "path")?;
        let path = resolve_agent_read_file(file_root, requested, allow_system_read)?;
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(format!(
                "文件超过 {} MiB 读取限制",
                MAX_FILE_BYTES / 1024 / 1024
            ));
        }
        let content =
            fs::read_to_string(&path).map_err(|error| format!("读取 {requested} 失败：{error}"))?;
        let start_line = bounded_positive_argument(arguments, "startLine", 1, usize::MAX)?;
        let line_count = bounded_positive_argument(
            arguments,
            "lineCount",
            MAX_FILE_READ_LINES,
            MAX_FILE_READ_LINES,
        )?;
        let all_lines = content.lines().collect::<Vec<_>>();
        let start_index = start_line.saturating_sub(1).min(all_lines.len());
        let end_index = start_index.saturating_add(line_count).min(all_lines.len());
        let selected = all_lines[start_index..end_index]
            .iter()
            .enumerate()
            .map(|(index, line)| format!("{:>6}\t{line}", start_index + index + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let next_start_line = (end_index < all_lines.len()).then_some(end_index + 1);
        return Ok(ToolResult {
            output: json!({
                "ok": true,
                "path": path,
                "content": selected,
                "startLine": start_index.saturating_add(1),
                "endLine": end_index,
                "totalLines": all_lines.len(),
                "nextStartLine": next_start_line,
                "truncated": next_start_line.is_some(),
                "readOnly": true
            }),
            summary: format!(
                "已只读读取 {requested} 第 {}–{} 行（共 {} 行）",
                start_index.saturating_add(1),
                end_index,
                all_lines.len()
            ),
        });
    }
    if name == "find_files" {
        return find_project_files(file_root, arguments, allow_system_read);
    }
    if name == "search_text" {
        return search_project_text(file_root, arguments, allow_system_read);
    }
    let project_root = context
        .project_root
        .as_deref()
        .ok_or_else(|| "尚未选择开发项目".to_owned())?;
    match name {
        "rpaz_list_files" => {
            let mut files = Vec::new();
            collect_project_files(project_root, project_root, &mut files)?;
            files.sort();
            let count = files.len();
            Ok(ToolResult {
                output: json!({"ok": true, "files": files}),
                summary: format!("已列出 {count} 个项目文件"),
            })
        }
        "edit_file" => edit_project_file(project_root, arguments),
        "rpaz_write_file" => {
            let relative = argument_string(arguments, "path")?;
            let content = argument_string(arguments, "content")?;
            if content.len() as u64 > MAX_FILE_BYTES {
                return Err(format!(
                    "文件超过 {} MiB 写入限制",
                    MAX_FILE_BYTES / 1024 / 1024
                ));
            }
            let path = resolve_project_file(project_root, relative, false)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(&path, content.as_bytes())
                .map_err(|error| format!("写入 {relative} 失败：{error}"))?;
            Ok(ToolResult {
                output: json!({"ok": true, "path": relative, "bytes": content.len()}),
                summary: format!("已写入 {relative}（{} 字节）", content.len()),
            })
        }
        "rpaz_validate" => validate_project(project_root),
        "rpaz_build" => build_project(context, project_root),
        "rpaz_python" => run_python(context, project_root, argument_string(arguments, "code")?),
        _ => Err(format!("未知 RPAZ 工具：{name}")),
    }
}

fn execute_browser_tool(
    context: &AgentContext,
    name: &str,
    arguments: &Value,
) -> Result<ToolResult, String> {
    if context.python.as_os_str().is_empty() {
        return Err("DRPA Chrome Bridge 需要内置 Python 运行时".to_owned());
    }
    let browser_session = context
        .browser_session
        .as_ref()
        .ok_or_else(|| "Agent 浏览器会话尚未初始化".to_owned())?;
    let value = browser_session.call(
        &context.python,
        context.browser.as_deref(),
        context.component_leases.clone(),
        name,
        arguments,
        context.python_timeout.min(Duration::from_secs(180)),
        || context.control.check(),
    )?;
    Ok(ToolResult {
        summary: value
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("Chrome Bridge 操作完成")
            .to_owned(),
        output: value,
    })
}

fn argument_string<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("工具参数缺少字符串字段 {name}"))
}

fn argument_optional_string<'a>(arguments: &'a Value, name: &str) -> &'a str {
    arguments.get(name).and_then(Value::as_str).unwrap_or("")
}

fn bounded_positive_argument(
    arguments: &Value,
    name: &str,
    default: usize,
    maximum: usize,
) -> Result<usize, String> {
    let Some(value) = arguments.get(name) else {
        return Ok(default);
    };
    let value = value
        .as_u64()
        .ok_or_else(|| format!("工具参数 {name} 必须是正整数"))?;
    let value = usize::try_from(value).map_err(|_| format!("工具参数 {name} 过大"))?;
    if value == 0 || value > maximum {
        return Err(format!("工具参数 {name} 必须在 1–{maximum} 之间"));
    }
    Ok(value)
}

fn resolve_project_search_path(
    project_root: &Path,
    value: &str,
    allow_system_read: bool,
) -> Result<PathBuf, String> {
    let canonical_root = fs::canonicalize(project_root).map_err(|error| error.to_string())?;
    let target = if value.trim().is_empty() || value.trim() == "." {
        canonical_root.clone()
    } else {
        let requested = Path::new(value);
        if requested.is_absolute() {
            fs::canonicalize(requested)
                .map_err(|error| format!("定位检索路径 {value} 失败：{error}"))?
        } else {
            let relative = safe_relative_path(value).map_err(|error| error.to_string())?;
            fs::canonicalize(project_root.join(relative))
                .map_err(|error| format!("定位检索路径 {value} 失败：{error}"))?
        }
    };
    if !allow_system_read && !target.starts_with(&canonical_root) {
        return Err("当前读取范围仅允许检索项目目录".to_owned());
    }
    if !target.is_dir() && !target.is_file() {
        return Err(format!("检索路径不存在：{value}"));
    }
    Ok(target)
}

fn find_project_files(
    project_root: &Path,
    arguments: &Value,
    allow_system_read: bool,
) -> Result<ToolResult, String> {
    let pattern = argument_string(arguments, "pattern")?.trim();
    if pattern.is_empty() {
        return Err("文件查找 pattern 不能为空".to_owned());
    }
    let search_root = resolve_project_search_path(
        project_root,
        argument_optional_string(arguments, "path"),
        allow_system_read,
    )?;
    let matcher = Glob::new(pattern)
        .map_err(|error| format!("文件 glob 无效：{error}"))?
        .compile_matcher();
    let limit = bounded_positive_argument(arguments, "limit", 200, MAX_FILE_TOOL_RESULTS)?;
    let canonical_project = fs::canonicalize(project_root).map_err(|error| error.to_string())?;
    let mut scanned = 0usize;
    let mut matches = Vec::new();
    let mut scan_truncated = false;
    let mut builder = WalkBuilder::new(search_root);
    builder.follow_links(false).standard_filters(true);
    for entry in builder.build().flatten() {
        scanned = scanned.saturating_add(1);
        if scanned > MAX_FILE_SCAN_ENTRIES {
            scan_truncated = true;
            break;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&canonical_project)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if matcher.is_match(&relative) || matcher.is_match(Path::new(entry.file_name())) {
            matches.push(relative);
        }
    }
    matches.sort();
    let result_truncated = matches.len() > limit;
    matches.truncate(limit);
    let count = matches.len();
    Ok(ToolResult {
        output: json!({
            "ok": true,
            "files": matches,
            "count": count,
            "scanned": scanned.min(MAX_FILE_SCAN_ENTRIES),
            "truncated": scan_truncated || result_truncated
        }),
        summary: format!("文件查找完成，返回 {count} 项"),
    })
}

fn build_search_regex(pattern: &str, literal: bool, ignore_case: bool) -> Result<Regex, String> {
    if pattern.is_empty() {
        return Err("文本检索 pattern 不能为空".to_owned());
    }
    let pattern = if literal {
        regex::escape(pattern)
    } else {
        pattern.to_owned()
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|error| format!("检索正则无效：{error}"))
}

fn search_project_text(
    project_root: &Path,
    arguments: &Value,
    allow_system_read: bool,
) -> Result<ToolResult, String> {
    let pattern = argument_string(arguments, "pattern")?;
    let regex = build_search_regex(
        pattern,
        arguments
            .get("literal")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        arguments
            .get("ignoreCase")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    )?;
    let file_matcher = arguments
        .get("glob")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|pattern| {
            Glob::new(pattern)
                .map(|glob| glob.compile_matcher())
                .map_err(|error| format!("文件 glob 无效：{error}"))
        })
        .transpose()?;
    let search_root = resolve_project_search_path(
        project_root,
        argument_optional_string(arguments, "path"),
        allow_system_read,
    )?;
    let limit = bounded_positive_argument(arguments, "limit", 100, MAX_FILE_TOOL_RESULTS)?;
    let canonical_project = fs::canonicalize(project_root).map_err(|error| error.to_string())?;
    let mut scanned = 0usize;
    let mut matches = Vec::new();
    let mut truncated = false;
    let mut builder = WalkBuilder::new(search_root);
    builder.follow_links(false).standard_filters(true);
    'entries: for entry in builder.build().flatten() {
        scanned = scanned.saturating_add(1);
        if scanned > MAX_FILE_SCAN_ENTRIES {
            truncated = true;
            break;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&canonical_project)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if file_matcher.as_ref().is_some_and(|matcher| {
            !matcher.is_match(&relative) && !matcher.is_match(Path::new(entry.file_name()))
        }) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(content) = fs::read_to_string(entry.path()) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if !regex.is_match(line) {
                continue;
            }
            let preview = line.chars().take(500).collect::<String>();
            matches.push(json!({"path": relative, "line": index + 1, "text": preview}));
            if matches.len() >= limit {
                truncated = true;
                break 'entries;
            }
        }
    }
    let count = matches.len();
    Ok(ToolResult {
        output: json!({
            "ok": true,
            "matches": matches,
            "count": count,
            "scanned": scanned.min(MAX_FILE_SCAN_ENTRIES),
            "truncated": truncated
        }),
        summary: format!("文本检索完成，返回 {count} 条匹配"),
    })
}

fn edit_project_file(project_root: &Path, arguments: &Value) -> Result<ToolResult, String> {
    let relative = argument_string(arguments, "path")?;
    let old_text = argument_string(arguments, "oldText")?;
    let new_text = argument_string(arguments, "newText")?;
    if old_text.is_empty() {
        return Err("edit_file 的 oldText 不能为空".to_owned());
    }
    if old_text == new_text {
        return Err("edit_file 的 oldText 与 newText 完全相同".to_owned());
    }
    let path = resolve_project_file(project_root, relative, true)?;
    let file_type = fs::symlink_metadata(&path)
        .map_err(|error| format!("检查 {relative} 失败：{error}"))?
        .file_type();
    if file_type.is_symlink() || !file_type.is_file() {
        return Err("edit_file 只允许编辑项目内普通文件，不能编辑符号链接".to_owned());
    }
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "文件超过 {} MiB 编辑限制",
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }
    let content =
        fs::read_to_string(&path).map_err(|error| format!("读取 {relative} 失败：{error}"))?;
    let occurrences = content.match_indices(old_text).count();
    if occurrences == 0 {
        return Err("oldText 在目标文件中没有匹配".to_owned());
    }
    if occurrences > 1 {
        return Err(format!(
            "oldText 在目标文件中匹配 {occurrences} 次；请提供更具体的上下文"
        ));
    }
    let updated = content.replacen(old_text, new_text, 1);
    if updated.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "编辑后文件超过 {} MiB 限制",
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }
    fs::write(&path, updated.as_bytes())
        .map_err(|error| format!("写入 {relative} 失败：{error}"))?;
    Ok(ToolResult {
        output: json!({
            "ok": true,
            "path": relative,
            "oldBytes": content.len(),
            "newBytes": updated.len(),
            "replacements": 1
        }),
        summary: format!("已精确编辑 {relative}（1 处替换）"),
    })
}

fn resolve_agent_read_file(
    project_root: &Path,
    value: &str,
    allow_system_read: bool,
) -> Result<PathBuf, String> {
    let requested = Path::new(value);
    let target = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        resolve_project_file(project_root, value, true)?
    };
    let canonical =
        fs::canonicalize(&target).map_err(|error| format!("定位只读文件 {value} 失败：{error}"))?;
    if !canonical.is_file() {
        return Err(format!("只读文件不存在：{value}"));
    }
    if !allow_system_read {
        let canonical_root = fs::canonicalize(project_root).map_err(|error| error.to_string())?;
        if !canonical.starts_with(canonical_root) {
            return Err("当前读取范围仅允许访问项目目录".to_owned());
        }
    }
    Ok(canonical)
}

fn resolve_project_file(root: &Path, value: &str, must_exist: bool) -> Result<PathBuf, String> {
    let relative = safe_relative_path(value).map_err(|error| error.to_string())?;
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let target = root.join(relative);
    let checked = if target.exists() {
        fs::canonicalize(&target).map_err(|error| error.to_string())?
    } else {
        let parent = target
            .parent()
            .ok_or_else(|| "文件路径缺少父目录".to_owned())?;
        let existing_parent = nearest_existing_parent(parent)?;
        let canonical_parent =
            fs::canonicalize(existing_parent).map_err(|error| error.to_string())?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err("文件路径超出当前项目".to_owned());
        }
        target.clone()
    };
    if !checked.starts_with(&canonical_root) {
        return Err("文件路径超出当前项目".to_owned());
    }
    if must_exist && !checked.is_file() {
        return Err(format!("项目文件不存在：{value}"));
    }
    Ok(target)
}

fn nearest_existing_parent(mut path: &Path) -> Result<&Path, String> {
    while !path.exists() {
        path = path.parent().ok_or_else(|| "文件路径无效".to_owned())?;
    }
    Ok(path)
}

fn validate_project(project_root: &Path) -> Result<ToolResult, String> {
    let source = fs::read_to_string(project_root.join("manifest.yaml"))
        .map_err(|error| format!("读取 manifest.yaml 失败：{error}"))?;
    let manifest = PackageManifest::from_yaml(&source).map_err(|error| error.to_string())?;
    let entrypoint = match &manifest.entrypoint {
        Entrypoint::Python { module, callable } => {
            if !project_root.join(module).is_file() {
                return Err(format!("入口文件不存在：{module}"));
            }
            format!("{module}:{callable}")
        }
        Entrypoint::Command { executable, .. } => {
            if !project_root.join(executable).is_file() {
                return Err(format!("入口文件不存在：{executable}"));
            }
            executable.clone()
        }
    };
    Ok(ToolResult {
        output: json!({
            "ok": true,
            "id": manifest.id,
            "name": manifest.name,
            "version": manifest.version,
            "entrypoint": entrypoint,
            "parameters": manifest.parameters.len(),
        }),
        summary: format!(
            "manifest.yaml 校验通过：{} {}",
            manifest.id, manifest.version
        ),
    })
}

fn build_project(context: &AgentContext, project_root: &Path) -> Result<ToolResult, String> {
    let validation = validate_project(project_root)?;
    let source = fs::read_to_string(project_root.join("manifest.yaml"))
        .map_err(|error| error.to_string())?;
    let manifest = PackageManifest::from_yaml(&source).map_err(|error| error.to_string())?;
    let output_root = context.workspace_root.join("build");
    fs::create_dir_all(&output_root).map_err(|error| error.to_string())?;
    let output = output_root.join(format!("{}-{}.rpaz", manifest.id, manifest.version));
    let file = File::create(&output).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut files = Vec::new();
    collect_project_files(project_root, project_root, &mut files)?;
    files.sort();
    for relative in &files {
        archive
            .start_file(relative, options)
            .map_err(|error| error.to_string())?;
        let mut content = Vec::new();
        File::open(project_root.join(relative))
            .and_then(|mut file| file.read_to_end(&mut content))
            .map_err(|error| error.to_string())?;
        archive
            .write_all(&content)
            .map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    Ok(ToolResult {
        output: json!({
            "ok": true,
            "path": output.to_string_lossy(),
            "files": files.len(),
            "validation": validation.output,
        }),
        summary: format!("已构建 {}（{} 个文件）", output.display(), files.len()),
    })
}

fn collect_project_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), ".git" | "__pycache__" | ".pytest_cache") {
            continue;
        }
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_project_files(root, &path, output)?;
        } else if kind.is_file() && !name.ends_with(".pyc") {
            output.push(
                path.strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        if output.len() > 4096 {
            return Err("项目文件超过 4096 个".to_owned());
        }
    }
    Ok(())
}

fn run_python(
    context: &AgentContext,
    project_root: &Path,
    code: &str,
) -> Result<ToolResult, String> {
    if code.len() > 50_000 {
        return Err("Python 辅助代码超过 50000 字符".to_owned());
    }
    const RUNNER: &str = r#"import os, site, sys
overlay = os.environ.get('DRPA_PYTHON_PACKAGE_PATH', '').strip()
if overlay:
    site.addsitedir(overlay)
    if overlay in sys.path:
        sys.path.remove(overlay)
    sys.path.insert(0, overlay)
source = sys.stdin.read()
exec(compile(source, '<drpa-agent-python>', 'exec'), {'__name__': '__main__'})
"#;
    let mut command = Command::new(&context.python);
    command
        .args(["-I", "-c", RUNNER])
        .current_dir(project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("DRPA_PYTHON_PACKAGE_PATH", &context.package_overlay);
    configure_agent_python_process(&mut command);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动内置 Python 失败：{error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "内置 Python 输入通道未建立".to_owned())?
        .write_all(code.as_bytes())
        .map_err(|error| format!("写入内置 Python 代码失败：{error}"))?;
    let mut process_tree = AgentPythonProcessTree::attach(&mut child)?;
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let output_bytes = Arc::new(AtomicUsize::new(0));
    let stdout_reader = capture_python_stream(
        child
            .stdout
            .take()
            .ok_or_else(|| "无法捕获 Python 标准输出".to_owned())?,
        output_exceeded.clone(),
        output_bytes.clone(),
    );
    let stderr_reader = capture_python_stream(
        child
            .stderr
            .take()
            .ok_or_else(|| "无法捕获 Python 错误输出".to_owned())?,
        output_exceeded.clone(),
        output_bytes,
    );
    let captured = wait_for_python_process(
        &mut child,
        &mut process_tree,
        stdout_reader,
        stderr_reader,
        output_exceeded,
        started,
        context.python_timeout,
        &context.control,
    )?;
    let status = captured.status;
    let stdout = captured.stdout;
    let stderr = captured.stderr;
    let exit_code = status.code().unwrap_or(-1);
    let output = json!({
        "ok": status.success(),
        "exitCode": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "durationMs": elapsed_ms(started),
    });
    if status.success() {
        Ok(ToolResult {
            output,
            summary: format!("Python 执行完成，退出码 {exit_code}"),
        })
    } else {
        Err(format!("Python 执行失败，退出码 {exit_code}：{stderr}"))
    }
}

#[derive(Debug)]
struct CapturedPythonProcess {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

#[allow(clippy::too_many_arguments)]
fn wait_for_python_process(
    child: &mut Child,
    process_tree: &mut AgentPythonProcessTree,
    stdout_reader: PythonStreamCapture,
    stderr_reader: PythonStreamCapture,
    output_exceeded: Arc<AtomicBool>,
    started: Instant,
    timeout: Duration,
    control: &AgentRunControl,
) -> Result<CapturedPythonProcess, String> {
    let status = loop {
        if let Err(error) = control.check() {
            terminate_python_process_tree(child, process_tree);
            drain_python_streams(stdout_reader, stderr_reader);
            return Err(error);
        }
        if output_exceeded.load(Ordering::Relaxed) {
            terminate_python_process_tree(child, process_tree);
            drain_python_streams(stdout_reader, stderr_reader);
            return Err(format!(
                "Python 输出超过 {} MiB 安全上限，进程树已终止",
                MAX_PYTHON_OUTPUT_BYTES / 1024 / 1024
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_python_process_tree(child, process_tree);
                drain_python_streams(stdout_reader, stderr_reader);
                return Err(format!("读取 Python 进程状态失败：{error}"));
            }
        }
        if started.elapsed() >= timeout {
            terminate_python_process_tree(child, process_tree);
            drain_python_streams(stdout_reader, stderr_reader);
            return Err(format!(
                "Python 辅助代码执行超过 {} 秒，进程树已终止",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(40));
    };

    // A direct Python process can exit after spawning a descendant that inherited
    // stdout/stderr. Tear down the managed tree before waiting for EOF so those
    // inherited pipe handles cannot keep the reader threads blocked forever.
    process_tree.terminate();
    let output_deadline = Instant::now() + PYTHON_OUTPUT_DRAIN_TIMEOUT;
    let stdout = stdout_reader.finish(output_deadline, "标准输出");
    let stderr = stderr_reader.finish(output_deadline, "错误输出");
    let stdout = stdout?;
    let stderr = stderr?;
    if output_exceeded.load(Ordering::Relaxed) {
        return Err(format!(
            "Python 输出超过 {} MiB 安全上限",
            MAX_PYTHON_OUTPUT_BYTES / 1024 / 1024
        ));
    }
    Ok(CapturedPythonProcess {
        status,
        stdout,
        stderr,
    })
}

struct PythonStreamCapture {
    receiver: mpsc::Receiver<String>,
}

impl PythonStreamCapture {
    fn finish(self, deadline: Instant, label: &str) -> Result<String, String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        self.receiver
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    format!("读取 Python {label}超过截止时间，后台管道已放弃")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    format!("读取 Python {label}的线程意外退出")
                }
            })
    }
}

fn capture_python_stream<R>(
    mut reader: R,
    output_exceeded: Arc<AtomicBool>,
    output_bytes: Arc<AtomicUsize>,
) -> PythonStreamCapture
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut retained = Vec::with_capacity(MAX_TOOL_OUTPUT_BYTES);
        let mut total = 0usize;
        let mut buffer = [0u8; 8192];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) => {
                    let _ = sender.send(format!("读取进程输出失败：{error}"));
                    return;
                }
            };
            total = total.saturating_add(count);
            let remaining = MAX_TOOL_OUTPUT_BYTES.saturating_sub(retained.len());
            retained.extend_from_slice(&buffer[..count.min(remaining)]);
            let aggregate = output_bytes
                .fetch_add(count, Ordering::Relaxed)
                .saturating_add(count);
            if aggregate > MAX_PYTHON_OUTPUT_BYTES {
                output_exceeded.store(true, Ordering::Relaxed);
                break;
            }
        }
        let mut text = String::from_utf8_lossy(&retained).into_owned();
        if total > retained.len() {
            text.push_str("\n…输出已截断…");
        }
        let _ = sender.send(text);
    });
    PythonStreamCapture { receiver }
}

fn drain_python_streams(stdout: PythonStreamCapture, stderr: PythonStreamCapture) {
    let deadline = Instant::now() + PYTHON_OUTPUT_DRAIN_TIMEOUT;
    let _ = stdout.finish(deadline, "标准输出");
    let _ = stderr.finish(deadline, "错误输出");
}

fn terminate_python_process_tree(child: &mut Child, process_tree: &mut AgentPythonProcessTree) {
    process_tree.terminate();
    let _ = child.kill();
    let _ = child.wait();
}

fn truncate_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…输出已截断…", &value[..end])
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(windows)]
fn configure_agent_python_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn configure_agent_python_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(any(windows, unix)))]
fn configure_agent_python_process(_command: &mut Command) {}

#[cfg(windows)]
struct AgentPythonProcessTree {
    job: Option<*mut std::ffi::c_void>,
}

#[cfg(windows)]
impl AgentPythonProcessTree {
    fn attach(child: &mut Child) -> Result<Self, String> {
        use std::mem::{size_of, zeroed};
        use std::os::windows::io::AsRawHandle;

        // SAFETY: null security attributes and name request an unnamed job owned
        // exclusively by this process.
        let job = unsafe { windows_job::CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if job.is_null() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "创建 Python Job Object 失败：{}",
                std::io::Error::last_os_error()
            ));
        }

        // SAFETY: this structure is plain C data and zero is the documented
        // default for all fields except the limit flag set below.
        let mut limits: windows_job::JobObjectExtendedLimitInformation = unsafe { zeroed() };
        limits.basic_limit_information.limit_flags =
            windows_job::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: job is valid and limits has the exact structure required by
        // JobObjectExtendedLimitInformation.
        let configured = unsafe {
            windows_job::SetInformationJobObject(
                job,
                windows_job::JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&mut limits as *mut windows_job::JobObjectExtendedLimitInformation).cast(),
                size_of::<windows_job::JobObjectExtendedLimitInformation>() as u32,
            )
        };
        // SAFETY: Child exposes its live process HANDLE for the duration of this call.
        let assigned = configured != 0
            && unsafe { windows_job::AssignProcessToJobObject(job, child.as_raw_handle().cast()) }
                != 0;
        if !assigned {
            let error = std::io::Error::last_os_error();
            // KILL_ON_JOB_CLOSE is best effort here because configuration may
            // itself have failed; explicitly kill and reap the direct child too.
            unsafe {
                let _ = windows_job::TerminateJobObject(job, 1);
                let _ = windows_job::CloseHandle(job);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("托管 Python 进程树失败：{error}"));
        }
        Ok(Self { job: Some(job) })
    }

    fn terminate(&mut self) {
        let Some(job) = self.job.take() else {
            return;
        };
        // SAFETY: job is owned by this value. Termination removes every process
        // still in the job; closing enforces KILL_ON_JOB_CLOSE and releases it.
        unsafe {
            let _ = windows_job::TerminateJobObject(job, 1);
            let _ = windows_job::CloseHandle(job);
        }
    }
}

#[cfg(windows)]
impl Drop for AgentPythonProcessTree {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(windows)]
mod windows_job {
    use std::ffi::c_void;

    pub(super) const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    pub(super) const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;

    #[repr(C)]
    pub(super) struct JobObjectBasicLimitInformation {
        pub per_process_user_time_limit: i64,
        pub per_job_user_time_limit: i64,
        pub limit_flags: u32,
        pub minimum_working_set_size: usize,
        pub maximum_working_set_size: usize,
        pub active_process_limit: u32,
        pub affinity: usize,
        pub priority_class: u32,
        pub scheduling_class: u32,
    }

    #[repr(C)]
    pub(super) struct IoCounters {
        pub read_operation_count: u64,
        pub write_operation_count: u64,
        pub other_operation_count: u64,
        pub read_transfer_count: u64,
        pub write_transfer_count: u64,
        pub other_transfer_count: u64,
    }

    #[repr(C)]
    pub(super) struct JobObjectExtendedLimitInformation {
        pub basic_limit_information: JobObjectBasicLimitInformation,
        pub io_info: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory_used: usize,
        pub peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(super) fn CreateJobObjectW(
            job_attributes: *mut c_void,
            name: *const u16,
        ) -> *mut c_void;
        pub(super) fn SetInformationJobObject(
            job: *mut c_void,
            information_class: i32,
            information: *mut c_void,
            information_length: u32,
        ) -> i32;
        pub(super) fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        pub(super) fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
        pub(super) fn CloseHandle(handle: *mut c_void) -> i32;
    }
}

#[cfg(unix)]
struct AgentPythonProcessTree {
    process_group: Option<i32>,
}

#[cfg(unix)]
impl AgentPythonProcessTree {
    fn attach(child: &mut Child) -> Result<Self, String> {
        let process_group = i32::try_from(child.id())
            .map_err(|_| "Python 进程 ID 超出 Unix process group 范围".to_owned())?;
        Ok(Self {
            process_group: Some(process_group),
        })
    }

    fn terminate(&mut self) {
        let Some(process_group) = self.process_group.take() else {
            return;
        };
        // SAFETY: Python was started with process_group(0), so its PID is the
        // dedicated process-group ID and negative kill targets only that tree.
        let _ = unsafe { libc::kill(-process_group, libc::SIGKILL) };
    }
}

#[cfg(unix)]
impl Drop for AgentPythonProcessTree {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(not(any(windows, unix)))]
struct AgentPythonProcessTree;

#[cfg(not(any(windows, unix)))]
impl AgentPythonProcessTree {
    fn attach(_child: &mut Child) -> Result<Self, String> {
        Ok(Self)
    }

    fn terminate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_openai_compatible_endpoints() {
        assert_eq!(
            crate::provider::chat_completions_endpoint("http://localhost:11434/v1").unwrap(),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            crate::provider::chat_completions_endpoint("https://gateway.example/api").unwrap(),
            "https://gateway.example/api/v1/chat/completions"
        );
        assert!(crate::provider::chat_completions_endpoint("file:///tmp/model").is_err());
        assert_eq!(
            agent_stream_event_name("req-1234").unwrap(),
            "agent-stream-req-1234"
        );
        assert!(agent_stream_event_name("bad/request").is_err());
    }

    #[test]
    fn context_checkpoint_is_reused_only_for_the_exact_history_prefix() {
        let messages = vec![
            AgentMessage {
                role: "user".to_owned(),
                content: "inspect the project".to_owned(),
            },
            AgentMessage {
                role: "assistant".to_owned(),
                content: "read manifest.yaml".to_owned(),
            },
            AgentMessage {
                role: "user".to_owned(),
                content: "continue".to_owned(),
            },
        ];
        let checkpoint = AgentContextCheckpoint {
            checkpoint_id: "ctx-test".to_owned(),
            summary: "The manifest was inspected.".to_owned(),
            covers_messages: 2,
            source_digest: message_prefix_digest(&messages, 2),
            created_at: 1,
            estimated_tokens: 8,
            method: "model".to_owned(),
        };

        assert!(valid_context_checkpoint(Some(&checkpoint), &messages).is_some());

        let mut edited = messages.clone();
        edited[0].content = "inspect another project".to_owned();
        assert!(valid_context_checkpoint(Some(&checkpoint), &edited).is_none());
    }

    #[test]
    fn python_stream_capture_is_memory_bounded_and_flags_excess_output() {
        let exceeded = Arc::new(AtomicBool::new(false));
        let output_bytes = Arc::new(AtomicUsize::new(0));
        let reader = std::io::Cursor::new(vec![b'x'; MAX_PYTHON_OUTPUT_BYTES + 1]);
        let captured = capture_python_stream(reader, exceeded.clone(), output_bytes)
            .finish(Instant::now() + Duration::from_secs(2), "测试输出")
            .unwrap();
        assert!(exceeded.load(Ordering::Relaxed));
        assert!(captured.len() <= MAX_TOOL_OUTPUT_BYTES + 64);
        assert!(captured.contains("输出已截断"));
    }

    #[test]
    fn python_stream_capture_obeys_its_deadline() {
        struct BlockingReader(mpsc::Receiver<()>);
        impl Read for BlockingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
                let _ = self.0.recv();
                Ok(0)
            }
        }

        let (release, blocked) = mpsc::channel();
        let capture = capture_python_stream(
            BlockingReader(blocked),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicUsize::new(0)),
        );
        let started = Instant::now();
        let error = capture
            .finish(Instant::now() + Duration::from_millis(60), "截止时间测试")
            .unwrap_err();
        assert!(error.contains("超过截止时间"));
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = release.send(());
    }

    const PROCESS_TREE_HELPER_ENV: &str = "DRPA_AGENT_PROCESS_TREE_HELPER";
    const PROCESS_TREE_HELPER_TEST: &str = "agent::tests::python_process_tree_test_helper";

    #[test]
    #[allow(clippy::zombie_processes)]
    fn python_process_tree_test_helper() {
        match std::env::var(PROCESS_TREE_HELPER_ENV).as_deref() {
            Ok("parent") => {
                // Give the supervising test enough time to attach this process to
                // its Job Object/process group before creating the descendant.
                thread::sleep(Duration::from_millis(200));
                // The supervising test owns and terminates the complete process tree.
                // Waiting here would prevent this helper from exercising descendant cleanup.
                let descendant = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", PROCESS_TREE_HELPER_TEST, "--nocapture"])
                    .env(PROCESS_TREE_HELPER_ENV, "descendant")
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
                println!("helper-parent spawned {}", descendant.id());
            }
            Ok("descendant") => {
                println!("helper-descendant-ready");
                std::io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(30));
            }
            Ok("flood") => {
                thread::sleep(Duration::from_millis(200));
                let chunk = vec![b'x'; 64 * 1024];
                for _ in 0..40 {
                    std::io::stdout().write_all(&chunk).unwrap();
                }
                std::io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(30));
            }
            _ => {}
        }
    }

    #[test]
    fn parent_exit_cleans_descendants_that_inherit_output_pipes() {
        let (mut child, mut tree, stdout, stderr, exceeded) =
            spawn_managed_process_helper("parent");
        let started = Instant::now();
        let captured = wait_for_python_process(
            &mut child,
            &mut tree,
            stdout,
            stderr,
            exceeded,
            started,
            Duration::from_secs(5),
            &AgentRunControl::for_tests(),
        )
        .unwrap();

        assert!(captured.status.success());
        assert!(captured.stdout.contains("helper-parent spawned"));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn python_timeout_cleans_the_managed_process_tree() {
        let (mut child, mut tree, stdout, stderr, exceeded) =
            spawn_managed_process_helper("descendant");
        let started = Instant::now();
        let error = wait_for_python_process(
            &mut child,
            &mut tree,
            stdout,
            stderr,
            exceeded,
            started,
            Duration::from_millis(120),
            &AgentRunControl::for_tests(),
        )
        .unwrap_err();

        assert!(error.contains("执行超过"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn python_output_limit_cleans_the_managed_process_tree() {
        let (mut child, mut tree, stdout, stderr, exceeded) = spawn_managed_process_helper("flood");
        let started = Instant::now();
        let error = wait_for_python_process(
            &mut child,
            &mut tree,
            stdout,
            stderr,
            exceeded,
            started,
            Duration::from_secs(5),
            &AgentRunControl::for_tests(),
        )
        .unwrap_err();

        assert!(error.contains("安全上限"));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    fn spawn_managed_process_helper(
        mode: &str,
    ) -> (
        Child,
        AgentPythonProcessTree,
        PythonStreamCapture,
        PythonStreamCapture,
        Arc<AtomicBool>,
    ) {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", PROCESS_TREE_HELPER_TEST, "--nocapture"])
            .env(PROCESS_TREE_HELPER_ENV, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_agent_python_process(&mut command);
        let mut child = command.spawn().unwrap();
        let tree = AgentPythonProcessTree::attach(&mut child).unwrap();
        let exceeded = Arc::new(AtomicBool::new(false));
        let bytes = Arc::new(AtomicUsize::new(0));
        let stdout = capture_python_stream(
            child.stdout.take().unwrap(),
            exceeded.clone(),
            bytes.clone(),
        );
        let stderr = capture_python_stream(child.stderr.take().unwrap(), exceeded.clone(), bytes);
        (child, tree, stdout, stderr, exceeded)
    }

    #[test]
    fn selected_skills_are_enforced_when_dispatching_tools() {
        let context = AgentContext {
            workspace_root: PathBuf::from("unused"),
            project_root: None,
            context_files: Vec::new(),
            python: PathBuf::from("python"),
            package_overlay: PathBuf::new(),
            browser: None,
            browser_session: None,
            component_leases: Vec::new(),
            runtime_features: Vec::new(),
            host: None,
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: vec!["data-analysis".to_owned()],
            tool_policy: AgentToolPolicy::default(),
            control: AgentRunControl::for_tests(),
        };
        let error = execute_tool(&context, "skill_other__run", &json!({}))
            .err()
            .unwrap();
        assert!(error.contains("未在当前会话中启用"));
    }

    #[test]
    fn assembles_openai_sse_text_usage_and_tool_call_fragments() {
        let source = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"# 标题\\n\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"完成\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"_file\",\"arguments\":\"\\\"main.py\\\"}\"}}]}}],\"usage\":{\"prompt_tokens\":25,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n",
        );
        let mut deltas = Vec::new();
        let response = crate::provider::parse_chat_completion_stream(
            std::io::Cursor::new(source.as_bytes()),
            |delta| deltas.push(delta),
        )
        .unwrap();

        assert_eq!(deltas, vec!["# 标题\n", "完成"]);
        assert_eq!(
            response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str),
            Some("# 标题\n完成")
        );
        assert_eq!(
            response
                .pointer("/choices/0/message/tool_calls/0/function/name")
                .and_then(Value::as_str),
            Some("read_file")
        );
        assert_eq!(
            response
                .pointer("/choices/0/message/tool_calls/0/function/arguments")
                .and_then(Value::as_str),
            Some("{\"path\":\"main.py\"}")
        );
        assert_eq!(
            response
                .pointer("/usage/prompt_tokens")
                .and_then(Value::as_u64),
            Some(25)
        );
    }

    #[test]
    fn consecutive_identical_tool_calls_execute_once_and_reuse_empty_success() {
        let mut ledger = ToolExecutionLedger::default();
        let arguments = json!({"code": "from pathlib import Path; Path('done').touch()"});
        let fingerprint = tool_call_fingerprint("rpaz_python", &arguments);
        let mut executions = 0usize;
        let first = ledger.dispatch(fingerprint.clone(), "call-1", || {
            executions += 1;
            Ok(ToolResult {
                output: json!({
                    "ok": true,
                    "exitCode": 0,
                    "stdout": "",
                    "stderr": "",
                    "durationMs": 8,
                }),
                summary: "Python 执行完成，退出码 0".to_owned(),
            })
        });
        let second = ledger.dispatch(fingerprint, "call-2", || {
            executions += 1;
            unreachable!("identical consecutive call must reuse the previous result")
        });

        assert_eq!(executions, 1);
        assert!(first.reused_from.is_none());
        assert_eq!(second.reused_from.as_deref(), Some("call-1"));
        let result = second.result.unwrap();
        let context = tool_context_output(
            "rpaz_python",
            "call-2",
            "completed",
            "重复调用已跳过",
            &result.output,
            second.reused_from.as_deref(),
        );
        let context: Value = serde_json::from_str(&context).unwrap();
        assert_eq!(context["executionPerformed"], false);
        assert_eq!(context["emptyOutput"], true);
        assert_eq!(context["reusedFromCallId"], "call-1");
        assert!(
            context["observation"]
                .as_str()
                .unwrap()
                .contains("成功完成")
        );
    }

    #[test]
    fn tool_fingerprint_is_stable_but_a_different_call_breaks_consecutive_reuse() {
        assert_eq!(
            tool_call_fingerprint("bash", &json!({"command":"echo ok","timeout":1000})),
            tool_call_fingerprint("bash", &json!({"timeout":1000,"command":"echo ok"})),
        );

        let mut ledger = ToolExecutionLedger::default();
        let mut executions = 0usize;
        for (call_id, arguments) in [
            ("call-1", json!({"command":"echo ok"})),
            ("call-2", json!({"path":"README.md"})),
            ("call-3", json!({"command":"echo ok"})),
        ] {
            let name = if call_id == "call-2" {
                "read_file"
            } else {
                "bash"
            };
            let dispatched =
                ledger.dispatch(tool_call_fingerprint(name, &arguments), call_id, || {
                    executions += 1;
                    Ok(ToolResult {
                        output: json!({"ok":true}),
                        summary: "done".to_owned(),
                    })
                });
            assert!(dispatched.reused_from.is_none());
        }
        assert_eq!(executions, 3);
    }

    #[test]
    fn context_budget_keeps_the_latest_turn_and_drops_oversized_older_history() {
        let request = AgentTurnRequest {
            request_id: "req-context".to_owned(),
            session_id: "session-context".to_owned(),
            base_url: "http://localhost:11434/v1".to_owned(),
            model: "local".to_owned(),
            mode: "rpaz".to_owned(),
            database_dialect: "sqlite".to_owned(),
            api_key: String::new(),
            provider_ref: None,
            project_id: String::new(),
            stream: true,
            context_window: 2_000,
            max_output_tokens: 512,
            max_rounds: DEFAULT_MAX_AGENT_ROUNDS,
            temperature: 0.2,
            python_timeout_seconds: DEFAULT_PYTHON_TIMEOUT_SECONDS,
            max_tool_calls: default_max_tool_calls(),
            max_wall_time_seconds: default_max_wall_time_seconds(),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
            context_checkpoint: None,
            messages: vec![
                AgentMessage {
                    role: "user".to_owned(),
                    content: "x".repeat(10_000),
                },
                AgentMessage {
                    role: "user".to_owned(),
                    content: "最新问题".to_owned(),
                },
            ],
        };

        assert_eq!(
            select_history_start(&request, "short system", &[]).unwrap(),
            1
        );
    }

    #[test]
    fn sql_mode_has_a_dedicated_prompt_and_no_tools() {
        let prompt = sql_system_prompt("postgresql");
        assert!(prompt.contains("PostgreSQL SQL 助手"));
        assert!(prompt.contains("Markdown 代码块"));
        assert!(!prompt.contains("RPAZ"));
    }

    #[test]
    fn older_agent_requests_default_to_rpaz_and_sqlite() {
        let request: AgentTurnRequest = serde_json::from_value(json!({
            "requestId": "req-defaults",
            "baseUrl": "http://localhost:11434/v1",
            "model": "local",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .unwrap();
        assert_eq!(request.mode, "rpaz");
        assert_eq!(request.database_dialect, "sqlite");
        assert_eq!(request.max_rounds, DEFAULT_MAX_AGENT_ROUNDS);
        assert_eq!(request.context_window, 393_216);
        assert_eq!(request.max_output_tokens, 98_304);
        assert_eq!(
            request.python_timeout_seconds,
            DEFAULT_PYTHON_TIMEOUT_SECONDS
        );
        assert!(request.selected_skill_ids.is_empty());
    }

    #[test]
    fn agent_round_limit_is_configurable_and_host_bounded() {
        let mut request: AgentTurnRequest = serde_json::from_value(json!({
            "requestId": "req-rounds",
            "baseUrl": "http://localhost:11434/v1",
            "model": "local",
            "maxRounds": 128,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .unwrap();
        assert_eq!(request.max_rounds, 128);
        assert!(validate_request(&request).is_ok());

        request.max_rounds = MAX_CONFIGURABLE_AGENT_ROUNDS + 1;
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn rpaz_tools_reject_parent_traversal_and_validate_a_project() {
        let workspace = std::env::temp_dir().join(format!("drpa-agent-{}", Uuid::new_v4()));
        let project = workspace
            .join("projects")
            .join("project-000000000000000000000001");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("manifest.yaml"),
            "schema: 2\nid: com.example.agent\nname: Agent Test\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\nruntime:\n  python: '3.11.*'\n",
        )
        .unwrap();
        fs::write(project.join("main.py"), "def main(ctx):\n    pass\n").unwrap();
        let outside = workspace.join("outside.txt");
        fs::write(&outside, "outside read").unwrap();
        let context = AgentContext {
            workspace_root: workspace.clone(),
            project_root: Some(project.clone()),
            context_files: Vec::new(),
            python: PathBuf::from("python"),
            package_overlay: PathBuf::new(),
            browser: None,
            browser_session: None,
            component_leases: Vec::new(),
            runtime_features: Vec::new(),
            host: None,
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
            control: AgentRunControl::for_tests(),
        };

        assert!(execute_tool(&context, "rpaz_validate", &json!({})).is_ok());
        assert!(execute_tool(&context, "read_file", &json!({"path": "../outside.txt"})).is_err());
        let read = execute_tool(
            &context,
            "read_file",
            &json!({"path": outside.to_string_lossy()}),
        )
        .unwrap();
        assert!(
            read.output["content"]
                .as_str()
                .unwrap()
                .contains("outside read")
        );
        assert_eq!(read.output["totalLines"], 1);
        assert!(
            execute_tool(
                &context,
                "read_file",
                &json!({"path": "main.py", "lineCount": MAX_FILE_READ_LINES + 1})
            )
            .is_err()
        );
        let found = execute_tool(&context, "find_files", &json!({"pattern": "**/*.py"})).unwrap();
        assert_eq!(found.output["files"][0], "main.py");
        let searched = execute_tool(
            &context,
            "search_text",
            &json!({"pattern": "def main", "literal": true}),
        )
        .unwrap();
        assert_eq!(searched.output["matches"][0]["line"], 1);
        execute_tool(
            &context,
            "edit_file",
            &json!({"path": "main.py", "oldText": "pass", "newText": "return None"}),
        )
        .unwrap();
        assert!(
            fs::read_to_string(project.join("main.py"))
                .unwrap()
                .contains("return None")
        );
        assert!(
            execute_tool(
                &context,
                "rpaz_write_file",
                &json!({"path": outside.to_string_lossy(), "content": "blocked"})
            )
            .is_err()
        );
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn tool_policy_filters_and_blocks_disabled_tools() {
        let workspace = std::env::temp_dir().join(format!("drpa-agent-policy-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let policy = AgentToolPolicy {
            database_read: false,
            arbitrary_file_read: false,
            project_write: false,
            python: false,
            workspace_write: false,
            extensions: false,
            ..AgentToolPolicy::default()
        };
        let definitions = agent_tool_definitions(
            &workspace,
            true,
            true,
            &[],
            DEFAULT_PYTHON_TIMEOUT_SECONDS,
            &policy,
            &[],
        )
        .unwrap();
        let names = definitions
            .iter()
            .filter_map(|value| value.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(!names.contains(&"data_query"));
        assert!(!names.contains(&"read_file"));
        assert!(!names.contains(&"rpaz_write_file"));
        assert!(names.contains(&"browser_open"));
        assert!(names.contains(&"rpaz_run_package"));
        assert!(names.contains(&"run_get_detail"));
        let authority = crate::agent_tools::CapabilityAuthority::new(&policy);
        assert!(authority.authorize_name("data_query").is_err());
        assert!(authority.authorize_name("rpaz_write_file").is_err());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn general_projects_get_python_without_rpaz_only_tools() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-agent-general-tools-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let definitions = agent_tool_definitions(
            &workspace,
            true,
            false,
            &[],
            DEFAULT_PYTHON_TIMEOUT_SECONDS,
            &AgentToolPolicy::default(),
            &[],
        )
        .unwrap();
        let names = definitions
            .iter()
            .filter_map(|value| value.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"rpaz_python"));
        assert!(names.contains(&"rpaz_write_file"));
        assert!(!names.contains(&"rpaz_validate"));
        assert!(!names.contains(&"rpaz_build"));
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn system_file_read_works_without_a_project_and_project_scope_blocks_escape() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-agent-read-workspace-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("drpa-agent-read-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("system.txt");
        fs::write(&outside_file, "system scope").unwrap();

        let system_policy = AgentToolPolicy::default();
        let definitions = agent_tool_definitions(
            &workspace,
            false,
            false,
            &[],
            DEFAULT_PYTHON_TIMEOUT_SECONDS,
            &system_policy,
            &[],
        )
        .unwrap();
        let names = definitions
            .iter()
            .filter_map(|value| value.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"find_files"));
        assert!(names.contains(&"search_text"));

        let mut context = AgentContext {
            workspace_root: workspace.clone(),
            project_root: None,
            context_files: Vec::new(),
            python: PathBuf::from("python"),
            package_overlay: PathBuf::new(),
            browser: None,
            browser_session: None,
            component_leases: Vec::new(),
            runtime_features: Vec::new(),
            host: None,
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: system_policy,
            control: AgentRunControl::for_tests(),
        };
        assert!(
            execute_tool(
                &context,
                "read_file",
                &json!({"path": outside_file.to_string_lossy()})
            )
            .unwrap()
            .output["content"]
                .as_str()
                .unwrap()
                .contains("system scope")
        );
        let found = execute_tool(
            &context,
            "find_files",
            &json!({"path": outside.to_string_lossy(), "pattern": "*.txt"}),
        )
        .unwrap();
        assert_eq!(found.output["count"], 1);

        context.tool_policy.file_read_scope = AgentFileReadScope::Project;
        assert!(
            execute_tool(
                &context,
                "read_file",
                &json!({"path": outside_file.to_string_lossy()})
            )
            .is_err()
        );
        assert!(
            execute_tool(
                &context,
                "find_files",
                &json!({"path": outside.to_string_lossy(), "pattern": "*.txt"})
            )
            .is_err()
        );

        fs::remove_dir_all(workspace).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn minimal_runtime_hides_unavailable_browser_and_document_tools() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-agent-minimal-tools-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let definitions = agent_tool_definitions(
            &workspace,
            true,
            false,
            &[],
            DEFAULT_PYTHON_TIMEOUT_SECONDS,
            &AgentToolPolicy::default(),
            &["agent.python".to_owned(), "stdlib".to_owned()],
        )
        .unwrap();
        let names = definitions
            .iter()
            .filter_map(|value| value.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(!names.iter().any(|name| name.starts_with("browser_")));
        assert!(!names.iter().any(|name| name.starts_with("document_")));
        assert!(names.contains(&"rpaz_python"));
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn database_agent_tool_executes_select_but_never_update() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-agent-database-{}", Uuid::new_v4()));
        let database_path = workspace.join("databases/workspace.sqlite3");
        fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);\
                 INSERT INTO items(name) VALUES ('first');",
            )
            .unwrap();
        drop(connection);
        let context = AgentContext {
            workspace_root: workspace.clone(),
            project_root: None,
            context_files: Vec::new(),
            python: PathBuf::from("python"),
            package_overlay: PathBuf::new(),
            browser: None,
            browser_session: None,
            component_leases: Vec::new(),
            runtime_features: Vec::new(),
            host: None,
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
            control: AgentRunControl::for_tests(),
        };

        let selected = execute_tool(
            &context,
            "data_query",
            &json!({"connectionId": "workspace", "sql": "SELECT name FROM items"}),
        )
        .unwrap();
        assert_eq!(selected.output["rows"][0][0], "first");
        assert!(
            execute_tool(
                &context,
                "data_query",
                &json!({"connectionId": "workspace", "sql": "UPDATE items SET name = 'blocked'"})
            )
            .is_err()
        );
        let unchanged: String = rusqlite::Connection::open(&database_path)
            .unwrap()
            .query_row("SELECT name FROM items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(unchanged, "first");
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn database_agent_tool_creates_sqlite_and_excel_profiles_without_passwords() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-agent-connections-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let context = AgentContext {
            workspace_root: workspace.clone(),
            project_root: None,
            context_files: Vec::new(),
            python: PathBuf::from("python"),
            package_overlay: PathBuf::new(),
            browser: None,
            browser_session: None,
            component_leases: Vec::new(),
            runtime_features: Vec::new(),
            host: None,
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
            control: AgentRunControl::for_tests(),
        };

        let sqlite = execute_tool(
            &context,
            "data_create_connection",
            &json!({
                "name": "Local analytics",
                "engine": "sqlite",
                "database": workspace.join("analytics.sqlite3").to_string_lossy(),
                "password": "must-not-be-saved"
            }),
        )
        .unwrap();
        assert_eq!(sqlite.output["connection"]["engine"], "sqlite");
        assert_eq!(sqlite.output["passwordStored"], false);

        let excel = execute_tool(
            &context,
            "data_create_connection",
            &json!({
                "name": "Quarterly workbook",
                "engine": "excel",
                "database": workspace.join("quarterly.xlsx").to_string_lossy()
            }),
        )
        .unwrap();
        assert_eq!(excel.output["connection"]["engine"], "excel");

        let listed = execute_tool(&context, "data_list_connections", &json!({})).unwrap();
        assert_eq!(listed.output["connections"].as_array().unwrap().len(), 3);
        let stored = fs::read_to_string(workspace.join("databases/connections.json")).unwrap();
        assert!(!stored.contains("must-not-be-saved"));
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn knowledge_tools_work_without_a_bound_project() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-agent-knowledge-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let context = AgentContext {
            workspace_root: workspace.clone(),
            project_root: None,
            context_files: Vec::new(),
            python: PathBuf::from("python"),
            package_overlay: PathBuf::new(),
            browser: None,
            browser_session: None,
            component_leases: Vec::new(),
            runtime_features: Vec::new(),
            host: None,
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
            control: AgentRunControl::for_tests(),
        };

        execute_tool(
            &context,
            "knowledge_write_document",
            &json!({"path": "业务/说明.md", "content": "# 说明\n"}),
        )
        .unwrap();
        let read = execute_tool(
            &context,
            "knowledge_read_document",
            &json!({"path": "业务/说明.md"}),
        )
        .unwrap();
        assert_eq!(read.output["content"], "# 说明\n");
        assert!(
            execute_tool(
                &context,
                "knowledge_write_document",
                &json!({"path": "../outside.md", "content": "bad"}),
            )
            .is_err()
        );
        fs::remove_dir_all(workspace).unwrap();
    }
}
