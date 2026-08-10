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
    let response_timeout = control.remaining()?;
    let profile = profile.clone();
    let payload = payload.clone();
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name(format!("drpa-provider-{}", control.request_id()))
        .spawn(move || {
            let delta_sender = sender.clone();
            let result = complete_blocking_with_response_timeout(
                &profile,
                &payload,
                stream,
                response_timeout,
                |content| {
                    let _ = delta_sender.send(WorkerEvent::Delta(content));
                },
            );
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
    complete_blocking_with_response_timeout(profile, payload, stream, profile.timeout, on_delta)
}

fn complete_blocking_with_response_timeout<F>(
    profile: &ProviderProfile,
    payload: &Value,
    stream: bool,
    response_timeout: Duration,
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
    let phase_timeout = profile
        .timeout
        .min(response_timeout)
        .max(Duration::from_millis(1));
    let response_timeout = response_timeout.max(Duration::from_millis(1));
    let http = pooled_agent(phase_timeout);
    let payload = prepare_provider_payload(profile, payload);
    let mut request = http
        .post(&endpoint)
        .config()
        .timeout_recv_response(Some(response_timeout))
        .timeout_recv_body(Some(response_timeout))
        .build()
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
    let mut response = request.send_json(&payload).map_err(|error| ProviderError {
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
        let response = response
            .body_mut()
            .read_json::<Value>()
            .map_err(|error| ProviderError {
                kind: ProviderErrorKind::Protocol,
                message: format!("Provider JSON 响应无效：{error}"),
                retryable: false,
            })?;
        Ok(normalize_chat_completion_response(response))
    }
}

fn pooled_agent(timeout: Duration) -> ureq::Agent {
    static POOL: OnceLock<Mutex<HashMap<u64, ureq::Agent>>> = OnceLock::new();
    let timeout_millis = u64::try_from(timeout.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let timeout = Duration::from_millis(timeout_millis);
    let pool = POOL.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut agents) = pool.lock() {
        return agents
            .entry(timeout_millis)
            .or_insert_with(|| {
                let config = ureq::Agent::config_builder()
                    .timeout_resolve(Some(timeout))
                    .timeout_connect(Some(timeout))
                    .timeout_send_request(Some(timeout))
                    .timeout_send_body(Some(timeout))
                    .build();
                ureq::Agent::new_with_config(config)
            })
            .clone();
    }
    let config = ureq::Agent::config_builder()
        .timeout_resolve(Some(timeout))
        .timeout_connect(Some(timeout))
        .timeout_send_request(Some(timeout))
        .timeout_send_body(Some(timeout))
        .build();
    ureq::Agent::new_with_config(config)
}

fn prepare_provider_payload(profile: &ProviderProfile, payload: &Value) -> Value {
    let mut prepared = payload.clone();
    if is_minimax_request(profile, payload)
        && let Some(object) = prepared.as_object_mut()
    {
        object
            .entry("reasoning_split".to_owned())
            .or_insert(Value::Bool(true));
    }
    prepared
}

fn is_minimax_request(profile: &ProviderProfile, payload: &Value) -> bool {
    profile.base_url.to_ascii_lowercase().contains("minimax")
        || payload
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|model| model.to_ascii_lowercase().contains("minimax"))
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
    raw_content: String,
    content: String,
    reasoning_content: String,
    reasoning_details: BTreeMap<usize, Value>,
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
        let response = normalize_chat_completion_response(response);
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
    if !accumulator.reasoning_details.is_empty() {
        assistant["reasoning_details"] = Value::Array(
            accumulator
                .reasoning_details
                .into_values()
                .collect::<Vec<_>>(),
        );
    } else if !accumulator.reasoning_content.is_empty() {
        assistant["reasoning_content"] = Value::String(accumulator.reasoning_content);
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
    let cumulative = chunk
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|model| model.to_ascii_lowercase().contains("minimax"))
        || delta.get("reasoning_details").is_some()
        || delta.get("reasoning_content").is_some();
    merge_reasoning_fields(delta, accumulator, cumulative);
    let content = message_content(delta.get("content"));
    if !content.is_empty() {
        merge_stream_fragment(&mut accumulator.raw_content, &content, cumulative);
        let split = split_thinking_content(&accumulator.raw_content);
        if !split.reasoning.is_empty() {
            merge_stream_fragment(&mut accumulator.reasoning_content, &split.reasoning, true);
        }
        if let Some(visible_delta) = split.visible.strip_prefix(&accumulator.content)
            && !visible_delta.is_empty()
        {
            accumulator.content.push_str(visible_delta);
            on_delta(visible_delta.to_owned());
        }
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
                merge_stream_fragment(&mut target.arguments, arguments, cumulative);
            }
        }
    }
    Ok(false)
}

fn merge_reasoning_fields(delta: &Value, accumulator: &mut StreamAccumulator, cumulative: bool) {
    for name in ["reasoning_content", "reasoning"] {
        if let Some(reasoning) = delta.get(name).and_then(Value::as_str) {
            merge_stream_fragment(&mut accumulator.reasoning_content, reasoning, cumulative);
        }
    }
    let Some(details) = delta.get("reasoning_details").and_then(Value::as_array) else {
        return;
    };
    for (position, detail) in details.iter().enumerate() {
        let index = detail
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(position);
        let target = accumulator
            .reasoning_details
            .entry(index)
            .or_insert_with(|| json!({}));
        if let (Some(target), Some(source)) = (target.as_object_mut(), detail.as_object()) {
            for (name, value) in source {
                if name == "text"
                    && let Some(fragment) = value.as_str()
                {
                    let current = target.get(name).and_then(Value::as_str).unwrap_or_default();
                    let mut merged = current.to_owned();
                    merge_stream_fragment(&mut merged, fragment, cumulative);
                    target.insert(name.clone(), Value::String(merged));
                } else {
                    target.insert(name.clone(), value.clone());
                }
            }
        }
    }
}

fn merge_stream_fragment(target: &mut String, fragment: &str, cumulative: bool) {
    if fragment.is_empty() {
        return;
    }
    if cumulative && fragment.starts_with(target.as_str()) {
        target.push_str(&fragment[target.len()..]);
    } else if cumulative && target.starts_with(fragment) {
        // A shorter cumulative snapshot carries no new information.
    } else {
        target.push_str(fragment);
    }
}

fn normalize_chat_completion_response(mut response: Value) -> Value {
    if let Some(choices) = response.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            if let Some(message) = choice.get_mut("message") {
                normalize_assistant_message(message);
            }
        }
    }
    response
}

fn normalize_assistant_message(message: &mut Value) {
    let Some(content) = message.get("content").and_then(Value::as_str) else {
        return;
    };
    let split = split_thinking_content(content);
    if split.visible == content {
        return;
    }
    message["content"] = Value::String(split.visible);
    if !split.reasoning.is_empty()
        && message.get("reasoning_details").is_none()
        && message.get("reasoning_content").is_none()
        && message.get("reasoning").is_none()
    {
        message["reasoning_content"] = Value::String(split.reasoning);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ThinkingContentSplit {
    visible: String,
    reasoning: String,
}

pub(crate) fn visible_assistant_content(content: &str) -> String {
    split_thinking_content(content).visible
}

fn split_thinking_content(content: &str) -> ThinkingContentSplit {
    const OPEN_TAGS: [&str; 2] = ["<think>", "<mm:think>"];
    const CLOSE_TAGS: [&str; 2] = ["</think>", "</mm:think>"];

    let lower = content.to_ascii_lowercase();
    let mut split = ThinkingContentSplit::default();
    let mut cursor = 0usize;
    while let Some((open_at, open_len)) = find_next_tag(&lower, cursor, &OPEN_TAGS) {
        append_visible_without_closing_tags(
            &content[cursor..open_at],
            &mut split.visible,
            &CLOSE_TAGS,
        );
        let reasoning_start = open_at + open_len;
        if let Some((close_at, close_len)) = find_next_tag(&lower, reasoning_start, &CLOSE_TAGS) {
            split
                .reasoning
                .push_str(&content[reasoning_start..close_at]);
            cursor = close_at + close_len;
        } else {
            split.reasoning.push_str(&content[reasoning_start..]);
            return split;
        }
    }
    let tail = &content[cursor..];
    let visible_end =
        trailing_tag_prefix_start(tail, &OPEN_TAGS, &CLOSE_TAGS).unwrap_or(tail.len());
    append_visible_without_closing_tags(&tail[..visible_end], &mut split.visible, &CLOSE_TAGS);
    split
}

fn find_next_tag(lower: &str, start: usize, tags: &[&str]) -> Option<(usize, usize)> {
    tags.iter()
        .filter_map(|tag| lower[start..].find(tag).map(|at| (start + at, tag.len())))
        .min_by_key(|(at, _)| *at)
}

fn trailing_tag_prefix_start(tail: &str, open_tags: &[&str], close_tags: &[&str]) -> Option<usize> {
    let lower = tail.to_ascii_lowercase();
    lower
        .char_indices()
        .map(|(start, _)| start)
        .rev()
        .find(|start| {
            let candidate = &lower[*start..];
            candidate.starts_with('<')
                && open_tags
                    .iter()
                    .chain(close_tags.iter())
                    .any(|tag| tag.starts_with(candidate) && candidate.len() < tag.len())
        })
}

fn append_visible_without_closing_tags(source: &str, target: &mut String, close_tags: &[&str]) {
    let lower = source.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some((tag_at, tag_len)) = find_next_tag(&lower, cursor, close_tags) {
        target.push_str(&source[cursor..tag_at]);
        cursor = tag_at + tag_len;
    }
    target.push_str(&source[cursor..]);
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

    #[test]
    fn provider_network_timeout_is_phase_scoped_instead_of_global() {
        let agent = pooled_agent(Duration::from_secs(120));
        let timeouts = agent.config().timeouts();
        assert_eq!(timeouts.global, None);
        assert_eq!(timeouts.recv_body, None);
        assert_eq!(timeouts.connect, Some(Duration::from_secs(120)));
        assert_eq!(timeouts.recv_response, None);
    }

    #[test]
    fn active_stream_can_outlive_the_initial_response_timeout() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            let header_end = loop {
                let read = socket.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                if let Some(at) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break at + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
            if headers.contains("expect: 100-continue") {
                socket.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
                socket.flush().unwrap();
            }
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or_default();
            while request.len().saturating_sub(header_end) < content_length {
                let read = socket.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"working\"}}]}\n\n",
                )
                .unwrap();
            socket.flush().unwrap();
            std::thread::sleep(Duration::from_millis(1_200));
            socket
                .write_all(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\" done\"}}]}\n\ndata: [DONE]\n\n",
                )
                .unwrap();
            socket.flush().unwrap();
        });
        let profile = ProviderProfile {
            base_url: format!("http://{address}/v1"),
            api_key: String::new(),
            timeout: Duration::from_secs(1),
            user_agent: "test".to_owned(),
            headers: BTreeMap::new(),
        };
        let mut deltas = Vec::new();
        let response = complete_cancellable(
            &profile,
            &json!({"model":"test","messages":[],"stream":true}),
            true,
            &AgentRunControl::for_tests(),
            |delta| deltas.push(delta),
        )
        .unwrap();
        server.join().unwrap();

        assert_eq!(deltas, vec!["working", " done"]);
        assert_eq!(
            response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str),
            Some("working done")
        );
    }

    #[test]
    fn minimax_requests_enable_reasoning_split_without_overriding_user_choice() {
        let profile = ProviderProfile::openai("https://api.minimax.io/v1", "token").unwrap();
        let prepared =
            prepare_provider_payload(&profile, &json!({"model": "minimax-m3", "messages": []}));
        assert_eq!(prepared.get("reasoning_split"), Some(&Value::Bool(true)));

        let prepared = prepare_provider_payload(
            &profile,
            &json!({"model": "minimax-m3", "reasoning_split": false}),
        );
        assert_eq!(prepared.get("reasoning_split"), Some(&Value::Bool(false)));
    }

    #[test]
    fn strips_complete_and_fragmented_thinking_tags_from_visible_content() {
        assert_eq!(
            split_thinking_content("<think>private plan</think>\nFinal answer"),
            ThinkingContentSplit {
                visible: "\nFinal answer".to_owned(),
                reasoning: "private plan".to_owned(),
            }
        );
        assert_eq!(visible_assistant_content("prefix <thi"), "prefix ");
        assert_eq!(
            visible_assistant_content("<mm:think>internal</mm:think>result"),
            "result"
        );
    }

    #[test]
    fn minimax_sse_routes_reasoning_away_from_markdown_and_deduplicates_snapshots() {
        let source = concat!(
            "data: {\"model\":\"MiniMax-M3\",\"choices\":[{\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"index\":0,\"text\":\"inspect\"}]}}]}\n\n",
            "data: {\"model\":\"MiniMax-M3\",\"choices\":[{\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"index\":0,\"text\":\"inspect files\"}],\"content\":\"Final \"}}]}\n\n",
            "data: {\"model\":\"MiniMax-M3\",\"choices\":[{\"delta\":{\"content\":\"Final answer\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let mut deltas = Vec::new();
        let response =
            parse_chat_completion_stream(std::io::Cursor::new(source.as_bytes()), |delta| {
                deltas.push(delta)
            })
            .unwrap();

        assert_eq!(deltas, vec!["Final ", "answer"]);
        assert_eq!(
            response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str),
            Some("Final answer")
        );
        assert_eq!(
            response
                .pointer("/choices/0/message/reasoning_details/0/text")
                .and_then(Value::as_str),
            Some("inspect files")
        );
    }

    #[test]
    fn tagged_reasoning_never_enters_stream_deltas() {
        let source = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"nk>secret\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"</think>Visible\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let mut deltas = Vec::new();
        let response =
            parse_chat_completion_stream(std::io::Cursor::new(source.as_bytes()), |delta| {
                deltas.push(delta)
            })
            .unwrap();
        assert_eq!(deltas, vec!["Visible"]);
        assert_eq!(
            response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str),
            Some("Visible")
        );
        assert_eq!(
            response
                .pointer("/choices/0/message/reasoning_content")
                .and_then(Value::as_str),
            Some("secret")
        );
    }

    #[test]
    fn non_streaming_responses_keep_reasoning_for_history_but_not_visible_content() {
        let response = normalize_chat_completion_response(json!({
            "choices": [{"message": {
                "role": "assistant",
                "content": "<think>inspect project</think>Completed"
            }}]
        }));
        assert_eq!(
            response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str),
            Some("Completed")
        );
        assert_eq!(
            response
                .pointer("/choices/0/message/reasoning_content")
                .and_then(Value::as_str),
            Some("inspect project")
        );
    }
}
