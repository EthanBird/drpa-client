use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{
    AgentMessage, AgentStreamEvent, AgentToolEvent, AgentTurnRequest, AgentTurnResult, AgentUsage,
    validate_project_id,
};

const JCODE_PROFILE: &str = "drpa-openai-compatible";
const MAX_EVENT_OUTPUT_BYTES: usize = 40_000;
static JCODE_RUN_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionIndex {
    #[serde(default)]
    sessions: BTreeMap<String, SessionState>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionState {
    jcode_session_id: String,
    conversation_digest: String,
}

#[derive(Debug, Default)]
struct StreamState {
    message: String,
    session_id: String,
    usage: AgentUsage,
    tools: BTreeMap<String, AgentToolEvent>,
    tool_names: BTreeMap<String, String>,
    last_error: String,
}

pub(crate) fn run_turn<F>(
    request: &AgentTurnRequest,
    workspace_root: &Path,
    resource_dir: Option<&Path>,
    mut emit: F,
) -> Result<AgentTurnResult, String>
where
    F: FnMut(AgentStreamEvent),
{
    let _run_guard = JCODE_RUN_LOCK
        .lock()
        .map_err(|_| "JCode 运行状态已损坏，请重启 DRPA".to_owned())?;
    let started = Instant::now();
    let executable = locate_jcode(workspace_root, resource_dir)?;
    let developer_root = workspace_root.join("agent").join("jcode");
    let jcode_home = developer_root.join("home");
    fs::create_dir_all(&jcode_home).map_err(|error| format!("创建 JCode 工作目录失败：{error}"))?;
    write_provider_config(&jcode_home, request)?;

    let working_dir = resolve_working_dir(workspace_root, &request.project_id)?;
    let index_path = developer_root.join("session-index.json");
    let mut index = read_session_index(&index_path)?;
    let previous_messages = request
        .messages
        .split_last()
        .map(|(_, previous)| previous)
        .unwrap_or(&[]);
    let previous_digest = conversation_digest(previous_messages);
    let resume_session = index
        .sessions
        .get(request.session_id.trim())
        .filter(|state| state.conversation_digest == previous_digest)
        .map(|state| state.jcode_session_id.clone());
    let prompt = if resume_session.is_some() {
        request
            .messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_default()
    } else {
        render_imported_conversation(&request.messages, request.context_window)
    };

    let mut command = Command::new(&executable);
    command
        .arg("--no-update")
        .arg("--quiet")
        .arg("-C")
        .arg(&working_dir)
        .arg("--provider-profile")
        .arg(JCODE_PROFILE)
        .arg("--model")
        .arg(request.model.trim())
        .arg("--tool-profile")
        .arg("full")
        .arg("--tools")
        .arg("all");
    if let Some(session_id) = resume_session.as_deref() {
        command.arg("--resume").arg(session_id);
    }
    command
        .arg("run")
        .arg("--ndjson")
        .arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("JCODE_HOME", &jcode_home)
        .env("DRPA_JCODE_API_KEY", request.api_key.trim())
        .env("JCODE_RUN_MCP", "1")
        .env("JCODE_NO_TELEMETRY", "1")
        .env("NO_COLOR", "1");
    hide_child_window(&mut command);

    let mut child = command.spawn().map_err(|error| {
        format!(
            "启动 JCode 开发者 Agent 失败（{}）：{error}",
            executable.display()
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "连接 JCode 事件流失败".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "连接 JCode 错误输出失败".to_owned())?;
    let stderr_reader = thread::spawn(move || {
        let mut text = String::new();
        let mut reader = BufReader::new(stderr);
        let _ = reader.read_to_string(&mut text);
        text
    });

    if request.stream {
        emit(AgentStreamEvent::RoundStarted { round: 1 });
    }
    let mut state = StreamState::default();
    let mut reader = BufReader::new(stdout);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        let read = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|error| format!("读取 JCode 事件流失败：{error}"))?;
        if read == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&bytes);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed)
            .map_err(|error| format!("JCode 返回了无效 NDJSON 事件：{error} · {trimmed}"))?;
        consume_event(&value, request.stream, &mut state, &mut emit)?;
    }

    let status = child
        .wait()
        .map_err(|error| format!("等待 JCode 退出失败：{error}"))?;
    let stderr = stderr_reader.join().unwrap_or_default().trim().to_owned();
    if !status.success() {
        let detail = if !state.last_error.is_empty() {
            state.last_error.clone()
        } else if !stderr.is_empty() {
            stderr
        } else {
            format!("exitCode={}", status.code().unwrap_or(-1))
        };
        return Err(format!("JCode 开发者 Agent 执行失败：{detail}"));
    }
    if !state.last_error.is_empty() && state.message.trim().is_empty() {
        return Err(format!("JCode 开发者 Agent 返回错误：{}", state.last_error));
    }
    if state.message.trim().is_empty() {
        return Err("JCode 开发者 Agent 返回了空消息".to_owned());
    }

    if !request.session_id.trim().is_empty() && !state.session_id.trim().is_empty() {
        let mut completed_messages = request.messages.clone();
        completed_messages.push(AgentMessage {
            role: "assistant".to_owned(),
            content: state.message.clone(),
        });
        index.sessions.insert(
            request.session_id.trim().to_owned(),
            SessionState {
                jcode_session_id: state.session_id.clone(),
                conversation_digest: conversation_digest(&completed_messages),
            },
        );
        write_session_index(&index_path, &index)?;
    }

    Ok(AgentTurnResult {
        message: state.message,
        tools: state.tools.into_values().collect(),
        usage: state.usage,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

fn consume_event<F>(
    value: &Value,
    stream: bool,
    state: &mut StreamState,
    emit: &mut F,
) -> Result<(), String>
where
    F: FnMut(AgentStreamEvent),
{
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "start" => {
            state.session_id = value
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
        }
        "session" => {
            state.session_id = value
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
        }
        "text_delta" => {
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            state.message.push_str(text);
            if stream && !text.is_empty() {
                emit(AgentStreamEvent::Delta {
                    content: text.to_owned(),
                });
            }
        }
        "text_replace" => {
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            state.message = text.to_owned();
            if stream {
                emit(AgentStreamEvent::ContentReplace {
                    content: text.to_owned(),
                });
            }
        }
        "tool_start" | "tool_exec" => {
            let id = event_id(value);
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            state.tool_names.insert(id.clone(), name.clone());
            if stream {
                emit(AgentStreamEvent::Tool {
                    tool: AgentToolEvent {
                        call_id: id,
                        name: format!("jcode:{name}"),
                        status: "running".to_owned(),
                        summary: "JCode 正在执行".to_owned(),
                        output: String::new(),
                    },
                });
            }
        }
        "tool_done" => {
            let id = event_id(value);
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| state.tool_names.get(&id).cloned())
                .unwrap_or_else(|| "tool".to_owned());
            let error = value.get("error").filter(|item| !item.is_null());
            let output = value.get("output").cloned().unwrap_or(Value::Null);
            let output = truncate_text(
                output
                    .as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| output.to_string()),
                MAX_EVENT_OUTPUT_BYTES,
            );
            let tool = AgentToolEvent {
                call_id: id.clone(),
                name: format!("jcode:{name}"),
                status: if error.is_some() {
                    "failed"
                } else {
                    "completed"
                }
                .to_owned(),
                summary: error
                    .map(|item| truncate_text(item.to_string(), 500))
                    .unwrap_or_else(|| format!("JCode 已执行 {name}")),
                output,
            };
            state.tools.insert(id, tool.clone());
            if stream {
                emit(AgentStreamEvent::Tool { tool });
            }
        }
        "tokens" => {
            state.usage.prompt_tokens = value.get("input").and_then(Value::as_u64).unwrap_or(0);
            state.usage.completion_tokens =
                value.get("output").and_then(Value::as_u64).unwrap_or(0);
        }
        "error" => {
            state.last_error = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("JCode 返回未知错误")
                .to_owned();
        }
        _ => {}
    }
    Ok(())
}

fn event_id(value: &Value) -> String {
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("jcode-tool-{}", uuid::Uuid::new_v4()))
}

fn resolve_working_dir(workspace_root: &Path, project_id: &str) -> Result<PathBuf, String> {
    if project_id.trim().is_empty() {
        return Ok(workspace_root.to_path_buf());
    }
    validate_project_id(project_id)?;
    let root = workspace_root.join("projects").join(project_id);
    if !root.is_dir() {
        return Err(format!("Agent 项目不存在：{project_id}"));
    }
    Ok(root)
}

fn locate_jcode(workspace_root: &Path, resource_dir: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("DRPA_JCODE_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "DRPA_JCODE_PATH 指向的文件不存在：{}",
            path.display()
        ));
    }

    let executable_name = if cfg!(windows) { "jcode.exe" } else { "jcode" };
    let mut candidates = Vec::new();
    if let Some(resource_dir) = resource_dir {
        candidates.push(resource_dir.join("jcode").join(executable_name));
    }
    if let Ok(current) = env::current_exe()
        && let Some(parent) = current.parent()
    {
        candidates.push(parent.join("jcode").join(executable_name));
    }
    candidates.push(
        workspace_root
            .join("runtime")
            .join("jcode")
            .join(executable_name),
    );
    #[cfg(debug_assertions)]
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/jcode")
            .join(executable_name),
    );
    if let Some(path) = find_on_path(executable_name) {
        candidates.push(path);
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "未找到内置 JCode。完整安装包应包含 jcode/jcode.exe；本地开发可设置 DRPA_JCODE_PATH。"
                .to_owned()
        })
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .map(|root| root.join(name))
        .find(|path| path.is_file())
}

fn write_provider_config(home: &Path, request: &AgentTurnRequest) -> Result<(), String> {
    let base_url = request.base_url.trim().trim_end_matches('/');
    if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err("OpenAI 兼容 URL 必须是有效的 http(s) 地址".to_owned());
    }
    let base_url = json_string(base_url)?;
    let model = json_string(request.model.trim())?;
    let (auth, key, required) = if request.api_key.trim().is_empty() {
        ("none", String::new(), "false")
    } else {
        (
            "bearer",
            "api_key_env = \"DRPA_JCODE_API_KEY\"\n".to_owned(),
            "true",
        )
    };
    let config = format!(
        "[provider]\n\
         default_provider = \"{JCODE_PROFILE}\"\n\
         default_model = {model}\n\n\
         [providers.{JCODE_PROFILE}]\n\
         type = \"openai-compatible\"\n\
         base_url = {base_url}\n\
         auth = \"{auth}\"\n\
         {key}\
         default_model = {model}\n\
         requires_api_key = {required}\n\n\
         [providers.{JCODE_PROFILE}.extra_body]\n\
         temperature = {}\n\
         max_tokens = {}\n\n\
         [[providers.{JCODE_PROFILE}.models]]\n\
         id = {model}\n\
         context_window = {}\n",
        request.temperature, request.max_output_tokens, request.context_window,
    );
    fs::write(home.join("config.toml"), config)
        .map_err(|error| format!("写入 JCode Provider 配置失败：{error}"))
}

fn json_string(value: &str) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

fn render_imported_conversation(messages: &[AgentMessage], context_window: u32) -> String {
    let budget = usize::try_from(context_window)
        .unwrap_or(usize::MAX)
        .saturating_mul(3)
        .clamp(8_000, 1_500_000);
    let mut selected = Vec::new();
    let mut used = 0usize;
    for message in messages.iter().rev() {
        let cost = message.content.len().saturating_add(40);
        if !selected.is_empty() && used.saturating_add(cost) > budget {
            break;
        }
        used = used.saturating_add(cost);
        selected.push(message);
    }
    selected.reverse();
    let mut prompt = String::from(
        "这是 DRPA 会话导入的上下文。你是 JCode 开发者 Agent，可直接使用完整工具集在当前工作目录完成最后一条用户请求。\n\n",
    );
    for message in selected {
        let label = if message.role == "assistant" {
            "ASSISTANT"
        } else {
            "USER"
        };
        prompt.push_str(&format!(
            "<DRPA_{label}>\n{}\n</DRPA_{label}>\n\n",
            message.content
        ));
    }
    prompt
}

fn conversation_digest(messages: &[AgentMessage]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for message in messages {
        for byte in message
            .role
            .bytes()
            .chain([0])
            .chain(message.content.bytes())
            .chain([0xff])
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

fn read_session_index(path: &Path) -> Result<SessionIndex, String> {
    if !path.is_file() {
        return Ok(SessionIndex::default());
    }
    let source =
        fs::read_to_string(path).map_err(|error| format!("读取 JCode 会话索引失败：{error}"))?;
    serde_json::from_str(&source).map_err(|error| format!("JCode 会话索引已损坏：{error}"))
}

fn write_session_index(path: &Path, index: &SessionIndex) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 JCode 会话目录失败：{error}"))?;
    }
    let content = serde_json::to_vec_pretty(index).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| format!("保存 JCode 会话索引失败：{error}"))
}

fn truncate_text(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value.push_str("\n… output truncated by DRPA");
    value
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

    fn request(api_key: &str) -> AgentTurnRequest {
        AgentTurnRequest {
            request_id: "req-jcode-test".to_owned(),
            session_id: "session-one".to_owned(),
            base_url: "http://127.0.0.1:8000/v1".to_owned(),
            model: "coder-model".to_owned(),
            mode: "developer".to_owned(),
            database_dialect: "sqlite".to_owned(),
            api_key: api_key.to_owned(),
            provider_ref: None,
            project_id: String::new(),
            stream: true,
            context_window: 128_000,
            max_output_tokens: 4_096,
            max_rounds: 64,
            temperature: 0.2,
            python_timeout_seconds: 300,
            selected_skill_ids: Vec::new(),
            tool_policy: crate::agent::AgentToolPolicy::default(),
            messages: vec![AgentMessage {
                role: "user".to_owned(),
                content: "inspect the project".to_owned(),
            }],
        }
    }

    #[test]
    fn provider_config_keeps_secret_out_of_file_and_exposes_full_model_limits() {
        let root = env::temp_dir().join(format!("drpa-jcode-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        write_provider_config(&root, &request("secret-token")).unwrap();
        let config = fs::read_to_string(root.join("config.toml")).unwrap();

        assert!(config.contains("api_key_env = \"DRPA_JCODE_API_KEY\""));
        assert!(!config.contains("secret-token"));
        assert!(config.contains("context_window = 128000"));
        assert!(config.contains("max_tokens = 4096"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn provider_config_supports_keyless_local_endpoint() {
        let root = env::temp_dir().join(format!("drpa-jcode-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        write_provider_config(&root, &request("")).unwrap();
        let config = fs::read_to_string(root.join("config.toml")).unwrap();

        assert!(config.contains("auth = \"none\""));
        assert!(config.contains("requires_api_key = false"));
        assert!(!config.contains("api_key_env"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ndjson_events_are_mapped_to_drpa_streams() {
        let mut state = StreamState::default();
        let mut emitted = Vec::new();
        consume_event(
            &serde_json::json!({"type":"start","session_id":"fox"}),
            true,
            &mut state,
            &mut |event| emitted.push(event),
        )
        .unwrap();
        consume_event(
            &serde_json::json!({"type":"text_delta","text":"done"}),
            true,
            &mut state,
            &mut |event| emitted.push(event),
        )
        .unwrap();
        consume_event(
            &serde_json::json!({"type":"tool_done","id":"1","name":"bash","output":"ok","error":null}),
            true,
            &mut state,
            &mut |event| emitted.push(event),
        )
        .unwrap();
        consume_event(
            &serde_json::json!({"type":"tokens","input":12,"output":4}),
            true,
            &mut state,
            &mut |event| emitted.push(event),
        )
        .unwrap();

        assert_eq!(state.session_id, "fox");
        assert_eq!(state.message, "done");
        assert_eq!(state.usage.prompt_tokens, 12);
        assert_eq!(state.tools["1"].status, "completed");
        assert_eq!(emitted.len(), 2);
    }

    #[test]
    fn conversation_digest_changes_when_latest_message_is_edited() {
        let original = vec![AgentMessage {
            role: "user".to_owned(),
            content: "first".to_owned(),
        }];
        let edited = vec![AgentMessage {
            role: "user".to_owned(),
            content: "edited".to_owned(),
        }];
        assert_ne!(conversation_digest(&original), conversation_digest(&edited));
        assert_eq!(
            conversation_digest(&original),
            conversation_digest(&original)
        );
    }
}
