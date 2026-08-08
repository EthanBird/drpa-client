use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::agent_runtime::AgentRunControl;

#[derive(Debug, Clone)]
pub(crate) struct ProviderProfile {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) timeout: Duration,
    pub(crate) user_agent: String,
    pub(crate) headers: BTreeMap<String, String>,
}

impl ProviderProfile {
    pub(crate) fn openai(base_url: &str, api_key: &str) -> Result<Self, String> {
        chat_completions_endpoint(base_url)?;
        Ok(Self {
            base_url: base_url.to_owned(),
            api_key: api_key.to_owned(),
            timeout: Duration::from_secs(120),
            user_agent: "DRPA-Next-Provider/1.0".to_owned(),
            headers: BTreeMap::new(),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ProviderErrorKind {
    Configuration,
    Transport,
    Protocol,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderError {
    pub(crate) kind: ProviderErrorKind,
    pub(crate) message: String,
    pub(crate) retryable: bool,
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

enum WorkerEvent {
    Delta(String),
    Finished(Result<Value, ProviderError>),
}

pub(crate) fn complete_cancellable<F>(
    profile: &ProviderProfile,
    payload: &Value,
    stream: bool,
    control: &AgentRunControl,
    mut on_delta: F,
) -> Result<Value, String>
where
    F: FnMut(String),
{
    control.check()?;
    let profile = profile.clone();
    let payload = payload.clone();
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name(format!("drpa-provider-{}", control.request_id()))
        .spawn(move || {
            let delta_sender = sender.clone();
            let result = complete_blocking(&profile, &payload, stream, |content| {
                let _ = delta_sender.send(WorkerEvent::Delta(content));
            });
            let _ = sender.send(WorkerEvent::Finished(result));
        })
        .map_err(|error| format!("启动 Provider 请求线程失败：{error}"))?;
    loop {
        control.check()?;
        match receiver.recv_timeout(Duration::from_millis(40)) {
            Ok(WorkerEvent::Delta(content)) => on_delta(content),
            Ok(WorkerEvent::Finished(result)) => return result.map_err(|error| error.message),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Provider 请求线程意外退出".to_owned());
            }
        }
    }
}

pub(crate) fn complete_blocking<F>(
    profile: &ProviderProfile,
    payload: &Value,
    stream: bool,
    on_delta: F,
) -> Result<Value, ProviderError>
where
    F: FnMut(String),
{
    let endpoint =
        chat_completions_endpoint(&profile.base_url).map_err(|message| ProviderError {
            kind: ProviderErrorKind::Configuration,
            message,
            retryable: false,
        })?;
    let http = pooled_agent(profile.timeout);
    let mut request = http
        .post(&endpoint)
        .header(
            "Accept",
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .header("User-Agent", &profile.user_agent);
    if !profile.api_key.trim().is_empty() {
        request = request.header(
            "Authorization",
            &format!("Bearer {}", profile.api_key.trim()),
        );
    }
    for (name, value) in &profile.headers {
        request = request.header(name, value);
    }
    let mut response = request.send_json(payload).map_err(|error| ProviderError {
        kind: ProviderErrorKind::Transport,
        message: format!("Provider 请求失败：{error}"),
        retryable: true,
    })?;
    if stream {
        parse_chat_completion_stream(BufReader::new(response.body_mut().as_reader()), on_delta)
            .map_err(|message| ProviderError {
                kind: ProviderErrorKind::Protocol,
                message,
                retryable: false,
            })
    } else {
        response
            .body_mut()
            .read_json::<Value>()
            .map_err(|error| ProviderError {
                kind: ProviderErrorKind::Protocol,
                message: format!("Provider JSON 响应无效：{error}"),
                retryable: false,
            })
    }
}

fn pooled_agent(timeout: Duration) -> ureq::Agent {
    static POOL: OnceLock<Mutex<HashMap<u64, ureq::Agent>>> = OnceLock::new();
    let timeout_seconds = timeout.as_secs().max(1);
    let pool = POOL.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut agents) = pool.lock() {
        return agents
            .entry(timeout_seconds)
            .or_insert_with(|| {
                let config = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(timeout_seconds)))
                    .build();
                ureq::Agent::new_with_config(config)
            })
            .clone();
    }
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_seconds)))
        .build();
    ureq::Agent::new_with_config(config)
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

pub(crate) fn parse_chat_completion_stream<R, F>(
    reader: R,
    mut on_delta: F,
) -> Result<Value, String>
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
    }
}
