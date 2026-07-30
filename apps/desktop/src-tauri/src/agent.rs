use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use drpa_package::{Entrypoint, PackageManifest, safe_relative_path, validate_package_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(test)]
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::{agent_config, agent_documents, database, knowledge, knowledge_base, plugins};

const DEFAULT_MAX_AGENT_ROUNDS: usize = 64;
const MAX_CONFIGURABLE_AGENT_ROUNDS: usize = 256;
const MAX_HISTORY_MESSAGES: usize = 120;
const MAX_MESSAGE_BYTES: usize = 100_000;
const MAX_TOOL_OUTPUT_BYTES: usize = 20_000;
const MAX_PYTHON_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const PYTHON_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
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
    #[serde(default)]
    pub selected_skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_policy: AgentToolPolicy,
    pub messages: Vec<AgentMessage>,
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
    pub project_write: bool,
    pub python: bool,
    pub workspace_write: bool,
    pub extensions: bool,
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
            project_write: true,
            python: true,
            workspace_write: true,
            extensions: true,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum AgentStreamEvent {
    RoundStarted { round: usize },
    Delta { content: String },
    Tool { tool: AgentToolEvent },
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
}

struct AgentContext {
    workspace_root: PathBuf,
    project_root: Option<PathBuf>,
    python: PathBuf,
    session_id: String,
    python_timeout: Duration,
    selected_skill_ids: Vec<String>,
    tool_policy: AgentToolPolicy,
}

struct ToolResult {
    output: Value,
    summary: String,
}

struct ToolRegistry<'a> {
    context: &'a AgentContext,
    definitions: Vec<Value>,
}

impl<'a> ToolRegistry<'a> {
    fn discover(context: &'a AgentContext) -> Result<Self, String> {
        Ok(Self {
            context,
            definitions: agent_tool_definitions(
                &context.workspace_root,
                context.project_root.is_some(),
                context
                    .project_root
                    .as_ref()
                    .is_some_and(|root| root.join("manifest.yaml").is_file()),
                &context.selected_skill_ids,
                context.python_timeout.as_secs(),
                &context.tool_policy,
            )?,
        })
    }

    fn execute(&self, name: &str, arguments: &Value) -> Result<ToolResult, String> {
        ensure_tool_allowed(&self.context.tool_policy, name)?;
        execute_tool(self.context, name, arguments)
    }
}

trait ProviderAdapter {
    fn complete(
        &self,
        payload: &Value,
        stream: bool,
        on_delta: &mut dyn FnMut(String),
    ) -> Result<Value, String>;
}

struct OpenAiCompatibleAdapter {
    endpoint: String,
    api_key: String,
}

impl OpenAiCompatibleAdapter {
    fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        Ok(Self {
            endpoint: chat_completions_endpoint(base_url)?,
            api_key: api_key.to_owned(),
        })
    }
}

impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn complete(
        &self,
        payload: &Value,
        stream: bool,
        on_delta: &mut dyn FnMut(String),
    ) -> Result<Value, String> {
        if stream {
            call_chat_completions_stream(&self.endpoint, &self.api_key, payload, on_delta)
        } else {
            call_chat_completions(&self.endpoint, &self.api_key, payload)
        }
    }
}

pub(crate) fn run_agent_turn<F>(
    mut request: AgentTurnRequest,
    workspace_root: PathBuf,
    python: PathBuf,
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
    let started = Instant::now();
    let provider = OpenAiCompatibleAdapter::new(&request.base_url, &request.api_key)?;
    let sql_mode = request.mode == "sql";
    let project_root = if sql_mode || request.project_id.trim().is_empty() {
        None
    } else {
        validate_project_id(&request.project_id)?;
        let root = workspace_root.join("projects").join(&request.project_id);
        if !root.is_dir() {
            return Err(format!("Agent 项目不存在：{}", request.project_id));
        }
        Some(root)
    };
    let context = AgentContext {
        workspace_root,
        project_root,
        python,
        session_id: request.session_id.clone(),
        python_timeout: Duration::from_secs(request.python_timeout_seconds),
        selected_skill_ids: request.selected_skill_ids.clone(),
        tool_policy: request.tool_policy.clone(),
    };

    let tool_registry = if sql_mode || !request.tool_policy.enabled {
        None
    } else {
        Some(ToolRegistry::discover(&context)?)
    };
    let (system, tools) = if sql_mode {
        (sql_system_prompt(&request.database_dialect), Vec::new())
    } else {
        let injected_context = agent_config::render_agent_context(
            &context.workspace_root,
            context.project_root.as_deref(),
            &request.selected_skill_ids,
        )?;
        (
            system_prompt(context.project_root.is_some(), &injected_context),
            tool_registry
                .as_ref()
                .map(|registry| registry.definitions.clone())
                .unwrap_or_default(),
        )
    };
    let history_start = select_history_start(&request, &system, &tools)?;
    let mut messages = vec![json!({
        "role": "system",
        "content": system,
    })];
    for message in &request.messages[history_start..] {
        messages.push(json!({"role": message.role, "content": message.content}));
    }

    let mut events = Vec::new();
    let mut usage = AgentUsage::default();

    for round in 0..request.max_rounds {
        if request.stream {
            emit(AgentStreamEvent::RoundStarted { round: round + 1 });
        }
        let mut payload = json!({
            "model": request.model.trim(),
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": request.max_output_tokens,
        });
        if !request.session_id.trim().is_empty() {
            payload["user"] = Value::String(request.session_id.trim().to_owned());
        }
        if !tools.is_empty() {
            payload["tools"] = Value::Array(tools.clone());
            payload["tool_choice"] = Value::String("auto".to_owned());
        }
        if request.stream {
            payload["stream"] = Value::Bool(true);
        }
        let response = provider.complete(&payload, request.stream, &mut |content| {
            emit(AgentStreamEvent::Delta { content });
        })?;
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
            });
        }

        for call in tool_calls {
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
            let arguments = parse_tool_arguments(call.pointer("/function/arguments"))?;
            let executed = tool_registry
                .as_ref()
                .ok_or_else(|| "当前 Agent 模式没有工具注册表".to_owned())?
                .execute(&name, &arguments);
            let (status, summary, output) = match executed {
                Ok(result) => ("completed".to_owned(), result.summary, result.output),
                Err(error) => (
                    "failed".to_owned(),
                    error.clone(),
                    json!({"ok": false, "error": error}),
                ),
            };
            let output_text = truncate_text(&output.to_string(), MAX_TOOL_OUTPUT_BYTES);
            let event_output = if name == "document_read" && status == "completed" {
                json!({
                    "ok": true,
                    "redacted": true,
                    "message": "文档正文只传给当前模型回合，不写入持久化工具事件"
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
            };
            events.push(tool_event.clone());
            if request.stream {
                emit(AgentStreamEvent::Tool { tool: tool_event });
            }
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output_text,
            }));
        }
    }

    Err(format!(
        "Agent 模型/工具循环达到本次配置上限 {} 轮；可在 Agent 设置中提高上限后继续",
        request.max_rounds
    ))
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
    if !matches!(request.mode.as_str(), "rpaz" | "sql") {
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

pub(crate) fn chat_completions_endpoint(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.len() > 2048
        || trimmed.contains(char::is_whitespace)
        || !(trimmed.starts_with("https://") || trimmed.starts_with("http://"))
    {
        return Err("OpenAI 兼容 URL 必须是有效的 http(s) 地址".to_owned());
    }
    if trimmed.ends_with("/chat/completions") {
        Ok(trimmed.to_owned())
    } else if trimmed.ends_with("/v1") {
        Ok(format!("{trimmed}/chat/completions"))
    } else {
        Ok(format!("{trimmed}/v1/chat/completions"))
    }
}

fn call_chat_completions(endpoint: &str, api_key: &str, payload: &Value) -> Result<Value, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let http = ureq::Agent::new_with_config(config);
    let mut request = http
        .post(endpoint)
        .header("Accept", "application/json")
        .header("User-Agent", "DRPA-Next-Agent/0.2");
    if !api_key.trim().is_empty() {
        let authorization = format!("Bearer {}", api_key.trim());
        request = request.header("Authorization", &authorization);
    }
    let mut response = request
        .send_json(payload)
        .map_err(|error| format!("模型接口请求失败：{error}"))?;
    response
        .body_mut()
        .read_json::<Value>()
        .map_err(|error| format!("模型接口返回的 JSON 无效：{error}"))
}

#[derive(Default)]
struct StreamToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct StreamAccumulator {
    content: String,
    tool_calls: BTreeMap<usize, StreamToolCall>,
    prompt_tokens: u64,
    completion_tokens: u64,
}

fn call_chat_completions_stream<F>(
    endpoint: &str,
    api_key: &str,
    payload: &Value,
    on_delta: F,
) -> Result<Value, String>
where
    F: FnMut(String),
{
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let http = ureq::Agent::new_with_config(config);
    let mut request = http
        .post(endpoint)
        .header("Accept", "text/event-stream")
        .header("User-Agent", "DRPA-Next-Agent/0.3");
    if !api_key.trim().is_empty() {
        let authorization = format!("Bearer {}", api_key.trim());
        request = request.header("Authorization", &authorization);
    }
    let mut response = request
        .send_json(payload)
        .map_err(|error| format!("模型流式接口请求失败：{error}"))?;
    let reader = BufReader::new(response.body_mut().as_reader());
    parse_chat_completion_stream(reader, on_delta)
}

fn parse_chat_completion_stream<R, F>(reader: R, mut on_delta: F) -> Result<Value, String>
where
    R: BufRead,
    F: FnMut(String),
{
    let mut accumulator = StreamAccumulator::default();
    let mut event_data = Vec::<String>::new();
    let mut raw_json = String::new();
    let mut saw_sse = false;
    let mut done = false;

    for line in reader.lines() {
        let line = line.map_err(|error| format!("读取模型流式响应失败：{error}"))?;
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !event_data.is_empty() {
                done =
                    consume_stream_event(&event_data.join("\n"), &mut accumulator, &mut on_delta)?;
                event_data.clear();
                if done {
                    break;
                }
            }
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            saw_sse = true;
            event_data.push(data.trim_start().to_owned());
        } else if !line.starts_with(':') && !line.starts_with("event:") && !saw_sse {
            raw_json.push_str(line);
            raw_json.push('\n');
        }
    }
    if !done && !event_data.is_empty() {
        consume_stream_event(&event_data.join("\n"), &mut accumulator, &mut on_delta)?;
    }

    if !saw_sse {
        let response: Value = serde_json::from_str(raw_json.trim())
            .map_err(|error| format!("模型接口既未返回 SSE，也未返回有效 JSON：{error}"))?;
        let content = message_content(response.pointer("/choices/0/message/content"));
        if !content.is_empty() {
            on_delta(content);
        }
        return Ok(response);
    }

    let tool_calls = accumulator
        .tool_calls
        .into_iter()
        .map(|(index, call)| {
            json!({
                "id": if call.id.is_empty() { format!("tool-call-{index}") } else { call.id },
                "type": "function",
                "function": { "name": call.name, "arguments": call.arguments },
            })
        })
        .collect::<Vec<_>>();
    let mut assistant = json!({"role": "assistant", "content": accumulator.content});
    if !tool_calls.is_empty() {
        assistant["tool_calls"] = Value::Array(tool_calls);
    }
    Ok(json!({
        "choices": [{"message": assistant}],
        "usage": {
            "prompt_tokens": accumulator.prompt_tokens,
            "completion_tokens": accumulator.completion_tokens,
        }
    }))
}

fn consume_stream_event<F>(
    data: &str,
    accumulator: &mut StreamAccumulator,
    on_delta: &mut F,
) -> Result<bool, String>
where
    F: FnMut(String),
{
    if data.trim() == "[DONE]" {
        return Ok(true);
    }
    let chunk: Value =
        serde_json::from_str(data).map_err(|error| format!("模型 SSE 数据无效：{error}"))?;
    if let Some(error) = chunk.get("error") {
        return Err(format!("模型流式接口返回错误：{error}"));
    }
    if let Some(usage) = chunk.get("usage") {
        accumulator.prompt_tokens = usage
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(accumulator.prompt_tokens);
        accumulator.completion_tokens = usage
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(accumulator.completion_tokens);
    }
    let Some(delta) = chunk
        .pointer("/choices/0/delta")
        .or_else(|| chunk.pointer("/choices/0/message"))
    else {
        return Ok(false);
    };
    let content = message_content(delta.get("content"));
    if !content.is_empty() {
        accumulator.content.push_str(&content);
        on_delta(content);
    }
    if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
        for (position, call) in tool_calls.iter().enumerate() {
            let index = call
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(position);
            let target = accumulator.tool_calls.entry(index).or_default();
            if let Some(id) = call.get("id").and_then(Value::as_str)
                && !id.is_empty()
            {
                if target.id.is_empty() || id.starts_with(&target.id) {
                    target.id = id.to_owned();
                } else if !target.id.ends_with(id) {
                    target.id.push_str(id);
                }
            }
            if let Some(name) = call.pointer("/function/name").and_then(Value::as_str)
                && !name.is_empty()
            {
                if target.name.is_empty() || name.starts_with(&target.name) {
                    target.name = name.to_owned();
                } else {
                    target.name.push_str(name);
                }
            }
            if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str) {
                target.arguments.push_str(arguments);
            }
        }
    }
    Ok(false)
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

fn system_prompt(has_project: bool, injected_context: &str) -> String {
    let context = if has_project {
        "当前已绑定一个通用开发项目；可在项目目录运行 Python，并按设置读取项目相对路径或任意绝对路径，写入始终严格限制在当前项目内。若项目包含 manifest.yaml，则它也是可校验和构建的规范化 RPAZ 项目。"
    } else {
        "当前未绑定项目，项目文件写入和 Python 工具暂不可用。"
    };
    format!(
        "你是 DRPA Next 内置的通用开发与自动化 Agent。{context}\n\
         RPAZ 是根目录含 manifest.yaml 的 ZIP，当前 schema 为 2；Python 入口实现 main(ctx)，\
         参数来自 ctx.params，产物使用 ctx.output_file，进度使用 ctx.progress。\n\
         你可以使用 data 工具列出数据工作台连接、读取结构并执行 Host 强制的只读查询；\
         data_create_connection 只保存连接元数据，不保存密码。知识文档是可编辑 Markdown，不向量化；\
         知识库是独立的只读混合向量索引，可用 knowledge_base_search 查询。\
         对话附件使用不透明 attachmentId，文档产物严格写入当前工作区的 Agent 产物目录。\
         你也可以按设置使用 knowledge、document 与扩展工具。\
         不假装使用未提供的终端、浏览器或网络工具。\n\
         只有规范化 RPAZ 项目才调用 rpaz_validate；需要交付 RPAZ 归档时调用 rpaz_build。\n\
         回答使用简体中文，先给结论，再列出实际完成的文件与验证结果。\
         {injected_context}"
    )
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
) -> Result<Vec<Value>, String> {
    if !policy.enabled {
        return Ok(Vec::new());
    }
    let mut tools = vec![
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
            "在数据工作台创建连接配置，支持 postgresql、mysql、sqlite、excel；只保存连接元数据，不保存密码，也不测试或修改数据。",
            json!({"type":"object","properties":{"name":{"type":"string"},"engine":{"type":"string","enum":["postgresql","mysql","sqlite","excel"]},"host":{"type":"string"},"port":{"type":"integer","minimum":0,"maximum":65535},"database":{"type":"string","description":"PostgreSQL/MySQL 数据库名，或 SQLite/Excel 文件绝对路径"},"username":{"type":"string"},"tlsMode":{"type":"string","enum":["disable","prefer","require"]}},"required":["name","engine","database"],"additionalProperties":false}),
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
    if has_project && policy.arbitrary_file_read {
        tools.push(tool_definition(
            "rpaz_read_file",
            "读取 UTF-8 文本文件。相对路径从当前项目解析；绝对路径可读取计算机上的任意现有文件。最多 2 MiB，只读且不会修改文件。",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        ));
    }
    if has_project {
        tools.push(tool_definition(
            "rpaz_list_files",
            "列出当前通用开发项目的文件。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ));
    }
    if has_project && policy.project_write {
        tools.push(tool_definition(
            "rpaz_write_file",
            "创建或覆盖当前项目中的 UTF-8 文本文件。Host 严格禁止写入项目目录之外。",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),
        ));
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
    }
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

fn ensure_tool_allowed(policy: &AgentToolPolicy, name: &str) -> Result<(), String> {
    if !policy.enabled {
        return Err("AI Agent 工具已在设置中关闭".to_owned());
    }
    let allowed = match name {
        "agent_list_skills"
        | "agent_read_skill"
        | "agent_read_memory"
        | "knowledge_list_documents"
        | "knowledge_read_document"
        | "rpaz_list_files" => true,
        "agent_write_skill" | "agent_write_memory" | "knowledge_write_document" => {
            policy.workspace_write
        }
        "data_list_connections" | "data_get_schema" | "data_query" => policy.database_read,
        "data_create_connection" => policy.database_connections,
        "knowledge_base_list" | "knowledge_base_search" => policy.knowledge_base_read,
        "document_read" => policy.document_read,
        "document_create" => policy.document_write,
        "document_convert" => policy.document_convert,
        "rpaz_read_file" => policy.arbitrary_file_read,
        "rpaz_write_file" | "rpaz_validate" | "rpaz_build" => policy.project_write,
        "rpaz_python" => policy.python,
        _ => policy.extensions,
    };
    if allowed {
        Ok(())
    } else {
        Err(format!("工具 {name} 已在设置中关闭"))
    }
}

fn execute_tool(
    context: &AgentContext,
    name: &str,
    arguments: &Value,
) -> Result<ToolResult, String> {
    if let Some((skill_name, _)) = name
        .strip_prefix("skill_")
        .and_then(|value| value.split_once("__"))
    {
        if !context.selected_skill_ids.is_empty()
            && !context
                .selected_skill_ids
                .iter()
                .any(|selected| selected == skill_name)
        {
            return Err(format!("Skill {skill_name} 未在当前会话中启用"));
        }
    }
    if let Some(executed) = agent_config::execute_skill_tool(
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
                "mysql" => 3306,
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
        "rpaz_read_file" => {
            let requested = argument_string(arguments, "path")?;
            let path = resolve_agent_read_file(project_root, requested)?;
            let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
            if metadata.len() > MAX_FILE_BYTES {
                return Err(format!(
                    "文件超过 {} MiB 读取限制",
                    MAX_FILE_BYTES / 1024 / 1024
                ));
            }
            let content = fs::read_to_string(&path)
                .map_err(|error| format!("读取 {requested} 失败：{error}"))?;
            Ok(ToolResult {
                output: json!({"ok": true, "path": path, "content": content, "readOnly": true}),
                summary: format!("已只读读取 {requested}（{} 字符）", content.chars().count()),
            })
        }
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

fn argument_string<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("工具参数缺少字符串字段 {name}"))
}

fn argument_optional_string<'a>(arguments: &'a Value, name: &str) -> &'a str {
    arguments.get(name).and_then(Value::as_str).unwrap_or("")
}

fn resolve_agent_read_file(project_root: &Path, value: &str) -> Result<PathBuf, String> {
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
    let mut command = Command::new(&context.python);
    command
        .args(["-I", "-c", code])
        .current_dir(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    configure_agent_python_process(&mut command);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动内置 Python 失败：{error}"))?;
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

fn wait_for_python_process(
    child: &mut Child,
    process_tree: &mut AgentPythonProcessTree,
    stdout_reader: PythonStreamCapture,
    stderr_reader: PythonStreamCapture,
    output_exceeded: Arc<AtomicBool>,
    started: Instant,
    timeout: Duration,
) -> Result<CapturedPythonProcess, String> {
    let status = loop {
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

fn validate_project_id(value: &str) -> Result<(), String> {
    let generated = value
        .strip_prefix("project-")
        .is_some_and(|hash| hash.len() == 24 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if generated || validate_package_id(value).is_ok() {
        Ok(())
    } else {
        Err("开发项目 ID 无效".to_owned())
    }
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
            chat_completions_endpoint("http://localhost:11434/v1").unwrap(),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("https://gateway.example/api").unwrap(),
            "https://gateway.example/api/v1/chat/completions"
        );
        assert!(chat_completions_endpoint("file:///tmp/model").is_err());
        assert_eq!(
            agent_stream_event_name("req-1234").unwrap(),
            "agent-stream-req-1234"
        );
        assert!(agent_stream_event_name("bad/request").is_err());
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
    fn python_process_tree_test_helper() {
        match std::env::var(PROCESS_TREE_HELPER_ENV).as_deref() {
            Ok("parent") => {
                // Give the supervising test enough time to attach this process to
                // its Job Object/process group before creating the descendant.
                thread::sleep(Duration::from_millis(200));
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
            python: PathBuf::from("python"),
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: vec!["data-analysis".to_owned()],
            tool_policy: AgentToolPolicy::default(),
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
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"rpaz_read\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"_file\",\"arguments\":\"\\\"main.py\\\"}\"}}]}}],\"usage\":{\"prompt_tokens\":25,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n",
        );
        let mut deltas = Vec::new();
        let response =
            parse_chat_completion_stream(std::io::Cursor::new(source.as_bytes()), |delta| {
                deltas.push(delta)
            })
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
            Some("rpaz_read_file")
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
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
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
            python: PathBuf::from("python"),
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
        };

        assert!(execute_tool(&context, "rpaz_validate", &json!({})).is_ok());
        assert!(
            execute_tool(
                &context,
                "rpaz_read_file",
                &json!({"path": "../outside.txt"})
            )
            .is_err()
        );
        let read = execute_tool(
            &context,
            "rpaz_read_file",
            &json!({"path": outside.to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(read.output["content"], "outside read");
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
        )
        .unwrap();
        let names = definitions
            .iter()
            .filter_map(|value| value.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(!names.contains(&"data_query"));
        assert!(!names.contains(&"rpaz_read_file"));
        assert!(!names.contains(&"rpaz_write_file"));
        assert!(ensure_tool_allowed(&policy, "data_query").is_err());
        assert!(ensure_tool_allowed(&policy, "rpaz_write_file").is_err());
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
            python: PathBuf::from("python"),
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
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
            python: PathBuf::from("python"),
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
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
            python: PathBuf::from("python"),
            session_id: "test-session".to_owned(),
            python_timeout: Duration::from_secs(DEFAULT_PYTHON_TIMEOUT_SECONDS),
            selected_skill_ids: Vec::new(),
            tool_policy: AgentToolPolicy::default(),
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
