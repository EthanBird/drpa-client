use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use drpa_package::{Entrypoint, PackageManifest, safe_relative_path, validate_package_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::{agent_config, knowledge};

const MAX_AGENT_ROUNDS: usize = 8;
const MAX_HISTORY_MESSAGES: usize = 120;
const MAX_MESSAGE_BYTES: usize = 100_000;
const MAX_TOOL_OUTPUT_BYTES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const PYTHON_TIMEOUT: Duration = Duration::from_secs(30);

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
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    pub messages: Vec<AgentMessage>,
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
}

struct ToolResult {
    output: Value,
    summary: String,
}

pub(crate) fn run_agent_turn<F>(
    request: AgentTurnRequest,
    workspace_root: PathBuf,
    python: PathBuf,
    mut emit: F,
) -> Result<AgentTurnResult, String>
where
    F: FnMut(AgentStreamEvent),
{
    validate_request(&request)?;
    let started = Instant::now();
    let endpoint = chat_completions_endpoint(&request.base_url)?;
    let project_root = if request.project_id.trim().is_empty() {
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
    };

    let injected_context = agent_config::render_agent_context(
        &context.workspace_root,
        context.project_root.as_deref(),
    )?;
    let system = system_prompt(context.project_root.is_some(), &injected_context);
    let tools = agent_tool_definitions(context.project_root.is_some());
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

    for round in 0..MAX_AGENT_ROUNDS {
        if request.stream {
            emit(AgentStreamEvent::RoundStarted { round: round + 1 });
        }
        let mut payload = json!({
            "model": request.model.trim(),
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": request.max_output_tokens,
        });
        if !tools.is_empty() {
            payload["tools"] = Value::Array(tools.clone());
            payload["tool_choice"] = Value::String("auto".to_owned());
        }
        let response = if request.stream {
            payload["stream"] = Value::Bool(true);
            call_chat_completions_stream(&endpoint, &request.api_key, &payload, |content| {
                emit(AgentStreamEvent::Delta { content });
            })?
        } else {
            call_chat_completions(&endpoint, &request.api_key, &payload)?
        };
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
            let executed = execute_tool(&context, &name, &arguments);
            let (status, summary, output) = match executed {
                Ok(result) => ("completed".to_owned(), result.summary, result.output),
                Err(error) => (
                    "failed".to_owned(),
                    error.clone(),
                    json!({"ok": false, "error": error}),
                ),
            };
            let output_text = truncate_text(&output.to_string(), MAX_TOOL_OUTPUT_BYTES);
            let tool_event = AgentToolEvent {
                call_id: call_id.clone(),
                name: name.clone(),
                status,
                summary,
                output: output_text.clone(),
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
        "Agent 工具调用超过 {MAX_AGENT_ROUNDS} 轮，请缩小任务范围后重试"
    ))
}

const fn default_context_window() -> u32 {
    128_000
}

const fn default_max_output_tokens() -> u32 {
    4_096
}

const fn default_temperature() -> f32 {
    0.2
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
    if !request.temperature.is_finite() || !(0.0..=2.0).contains(&request.temperature) {
        return Err("Temperature 必须在 0 到 2 之间".to_owned());
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
        "当前已绑定一个开发工作室项目，可以使用 RPAZ 工具读取、修改、校验、构建项目，也可以运行限时 Python 辅助分析。"
    } else {
        "当前未绑定开发项目，项目文件和 Python 工具暂不可用，但仍可读取或维护本地知识文档。"
    };
    format!(
        "你是 DRPA Next 内置的轻量 RPAZ 开发 Agent。{context}\n\
         RPAZ 是根目录含 manifest.yaml 的 ZIP，当前 schema 为 2；Python 入口实现 main(ctx)，\
         参数来自 ctx.params，产物使用 ctx.output_file，进度使用 ctx.progress。\n\
         你可以使用 knowledge 工具读取和维护本地 Markdown 知识库。只处理 RPAZ 项目开发，\
         不假装使用未提供的终端、浏览器或网络工具。\n\
         修改文件后应调用 rpaz_validate；需要交付归档时调用 rpaz_build。\n\
         回答使用简体中文，先给结论，再列出实际完成的文件与验证结果。\
         {injected_context}"
    )
}

fn agent_tool_definitions(has_project: bool) -> Vec<Value> {
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
            "agent_write_skill",
            "仅在用户要求保存可复用流程时创建或更新一个标准 SKILL.md。name 使用小写字母、数字和中划线。",
            json!({"type":"object","properties":{"name":{"type":"string"},"content":{"type":"string"}},"required":["name","content"],"additionalProperties":false}),
        ),
        tool_definition(
            "agent_read_memory",
            "读取本地 Agent 长期记忆 MEMORY.md。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "agent_write_memory",
            "仅在用户明确要求记住稳定事实或偏好时更新完整 MEMORY.md；不要写入密钥。",
            json!({"type":"object","properties":{"content":{"type":"string"}},"required":["content"],"additionalProperties":false}),
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
        tool_definition(
            "knowledge_write_document",
            "创建或覆盖本地知识库中的 Markdown 文档；父目录会按安全相对路径创建。",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),
        ),
    ];
    if !has_project {
        return tools;
    }
    tools.extend([
        tool_definition(
            "rpaz_list_files",
            "列出当前 RPAZ 开发项目的文件。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_read_file",
            "读取当前 RPAZ 项目中的 UTF-8 文本文件。",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_write_file",
            "创建或覆盖当前 RPAZ 项目中的 UTF-8 文本文件。",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),
        ),
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
        tool_definition(
            "rpaz_python",
            "在当前项目目录用 DRPA 内置 Python 执行最多 30 秒的辅助代码，适合检查 JSON、生成模板或验证纯 Python 逻辑。",
            json!({"type":"object","properties":{"code":{"type":"string"}},"required":["code"],"additionalProperties":false}),
        ),
    ]);
    tools
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
        _ => {}
    }
    let project_root = context
        .project_root
        .as_deref()
        .ok_or_else(|| "尚未选择 RPAZ 开发项目".to_owned())?;
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
            let relative = argument_string(arguments, "path")?;
            let path = resolve_project_file(project_root, relative, true)?;
            let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
            if metadata.len() > MAX_FILE_BYTES {
                return Err(format!(
                    "文件超过 {} MiB 读取限制",
                    MAX_FILE_BYTES / 1024 / 1024
                ));
            }
            let content = fs::read_to_string(&path)
                .map_err(|error| format!("读取 {relative} 失败：{error}"))?;
            Ok(ToolResult {
                output: json!({"ok": true, "path": relative, "content": content}),
                summary: format!("已读取 {relative}（{} 字符）", content.chars().count()),
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
            return Err("文件路径超出当前 RPAZ 项目".to_owned());
        }
        target.clone()
    };
    if !checked.starts_with(&canonical_root) {
        return Err("文件路径超出当前 RPAZ 项目".to_owned());
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
    let temporary_root = context.workspace_root.join("agent").join("tmp");
    fs::create_dir_all(&temporary_root).map_err(|error| error.to_string())?;
    let id = Uuid::new_v4().simple().to_string();
    let stdout_path = temporary_root.join(format!("{id}.stdout"));
    let stderr_path = temporary_root.join(format!("{id}.stderr"));
    let stdout = File::create(&stdout_path).map_err(|error| error.to_string())?;
    let stderr = File::create(&stderr_path).map_err(|error| error.to_string())?;
    let mut command = Command::new(&context.python);
    command
        .args(["-I", "-c", code])
        .current_dir(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    hide_child_window(&mut command);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动内置 Python 失败：{error}"))?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= PYTHON_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            cleanup_file(&stdout_path);
            cleanup_file(&stderr_path);
            return Err("Python 辅助代码执行超过 30 秒".to_owned());
        }
        thread::sleep(Duration::from_millis(40));
    };
    let stdout = read_limited(&stdout_path, MAX_TOOL_OUTPUT_BYTES);
    let stderr = read_limited(&stderr_path, MAX_TOOL_OUTPUT_BYTES);
    cleanup_file(&stdout_path);
    cleanup_file(&stderr_path);
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

fn read_limited(path: &Path, limit: usize) -> String {
    match fs::read(path) {
        Ok(content) => truncate_text(&String::from_utf8_lossy(&content), limit),
        Err(error) => format!("读取输出失败：{error}"),
    }
}

fn cleanup_file(path: &Path) {
    let _ = fs::remove_file(path);
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
fn hide_child_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_child_window(_command: &mut Command) {}

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
            base_url: "http://localhost:11434/v1".to_owned(),
            model: "local".to_owned(),
            api_key: String::new(),
            project_id: String::new(),
            stream: true,
            context_window: 2_000,
            max_output_tokens: 512,
            temperature: 0.2,
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
        let context = AgentContext {
            workspace_root: workspace.clone(),
            project_root: Some(project.clone()),
            python: PathBuf::from("python"),
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
