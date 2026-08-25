use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{
    AgentHostContext, AgentMessage, AgentStreamEvent, AgentToolEvent, AgentTurnRequest,
    AgentTurnResult, AgentUsage, persistent_tool_input,
};
use crate::agent_browser::AgentBrowserSession;
use crate::agent_loop_guard::ToolLoopGuard;
use crate::agent_runtime::AgentRunControl;

const JCODE_PROFILE: &str = "drpa-openai-compatible";
const MAX_EVENT_OUTPUT_BYTES: usize = 40_000;
const MAX_EVENT_TOOL_INPUT_BYTES: usize = 200_000;
const JCODE_LOOP_STOP_SIGNAL: &str = "__DRPA_JCODE_LOOP_STOP__";
static JCODE_SESSION_INDEX_LOCK: Mutex<()> = Mutex::new(());

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
    raw_message: String,
    message: String,
    session_id: String,
    usage: AgentUsage,
    tools: BTreeMap<String, AgentToolEvent>,
    tool_names: BTreeMap<String, String>,
    tool_inputs: BTreeMap<String, String>,
    tool_ordinals: BTreeMap<String, usize>,
    tool_started_at: BTreeMap<String, Instant>,
    tool_fingerprints: BTreeMap<String, (String, String)>,
    observed_tool_ids: BTreeSet<String>,
    current_tool_input_id: Option<String>,
    tool_calls: usize,
    stop_reason: Option<String>,
    tool_loop_guard: ToolLoopGuard,
    last_error: String,
}

struct HostBridgeServer {
    endpoint: String,
    token: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HostBridgeServer {
    fn start(host: AgentHostContext) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("启动 Agent Host Bridge 失败：{error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("配置 Agent Host Bridge 失败：{error}"))?;
        let endpoint = listener
            .local_addr()
            .map_err(|error| format!("读取 Agent Host Bridge 地址失败：{error}"))?
            .to_string();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let expected_token = token.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("drpa-agent-host-bridge".to_owned())
            .spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_host_bridge_connection(stream, &expected_token, &host)
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| format!("启动 Agent Host Bridge 线程失败：{error}"))?;
        Ok(Self {
            endpoint,
            token,
            stop,
            worker: Some(worker),
        })
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for HostBridgeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(&self.endpoint);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn handle_host_bridge_connection(
    mut stream: TcpStream,
    expected_token: &str,
    host: &AgentHostContext,
) {
    let response = (|| -> Result<Value, String> {
        let mut source = String::new();
        BufReader::new(
            stream
                .try_clone()
                .map_err(|error| format!("读取 Bridge 请求失败：{error}"))?,
        )
        .read_line(&mut source)
        .map_err(|error| format!("读取 Bridge 请求失败：{error}"))?;
        let request: Value = serde_json::from_str(&source)
            .map_err(|error| format!("Bridge 请求 JSON 无效：{error}"))?;
        if request.get("token").and_then(Value::as_str) != Some(expected_token) {
            return Err("Bridge token 无效".to_owned());
        }
        let name = request
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "Bridge 请求缺少 name".to_owned())?;
        let arguments = request.get("arguments").unwrap_or(&Value::Null);
        host.execute(name, arguments)
    })();
    let payload = match response {
        Ok(result) => serde_json::json!({"ok": true, "result": result}),
        Err(error) => serde_json::json!({"ok": false, "error": error}),
    };
    if let Ok(mut bytes) = serde_json::to_vec(&payload) {
        bytes.push(b'\n');
        let _ = stream.write_all(&bytes);
        let _ = stream.flush();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_turn<F>(
    request: &AgentTurnRequest,
    workspace_root: &Path,
    resource_dir: Option<&Path>,
    python: &Path,
    browser: Option<&Path>,
    browser_session: Option<&AgentBrowserSession>,
    host: AgentHostContext,
    control: AgentRunControl,
    mut emit: F,
) -> Result<AgentTurnResult, String>
where
    F: FnMut(AgentStreamEvent),
{
    control.check()?;
    let started = Instant::now();
    let executable = locate_jcode(workspace_root, resource_dir)?;
    let developer_root = workspace_root.join("agent").join("jcode");
    let home_key = if request.session_id.trim().is_empty() {
        control.request_id()
    } else {
        request.session_id.trim()
    };
    let jcode_home = developer_root.join("homes").join(home_key);
    fs::create_dir_all(&jcode_home).map_err(|error| format!("创建 JCode 工作目录失败：{error}"))?;
    write_provider_config(&jcode_home, request)?;
    let host = host.with_agent_scope(home_key, python, control.clone());
    let host_bridge = HostBridgeServer::start(host)?;
    write_mcp_config(
        &jcode_home,
        python,
        browser,
        browser_session,
        host_bridge.endpoint(),
        host_bridge.token(),
    )?;

    let project_context = crate::agent_sessions::resolve_agent_project_context(
        workspace_root,
        &request.project_id,
        &request.session_id,
    )?;
    let working_dir = project_context
        .as_ref()
        .map(|project| project.root.clone())
        .unwrap_or_else(|| workspace_root.to_path_buf());
    let context_files = project_context
        .as_ref()
        .map(|project| project.context_files.as_slice())
        .unwrap_or_default();
    let index_path = developer_root.join("session-index.json");
    let previous_messages = request
        .messages
        .split_last()
        .map(|(_, previous)| previous)
        .unwrap_or(&[]);
    let previous_digest = conversation_digest(previous_messages);
    let resume_session = {
        let _guard = JCODE_SESSION_INDEX_LOCK
            .lock()
            .map_err(|_| "JCode 会话索引状态已损坏".to_owned())?;
        read_session_index(&index_path)?
            .sessions
            .get(request.session_id.trim())
            .filter(|state| state.conversation_digest == previous_digest)
            .map(|state| state.jcode_session_id.clone())
    };
    let mut agent_context = crate::agent_config::render_agent_context(
        workspace_root,
        Some(&working_dir),
        &request.selected_skill_ids,
    )?;
    agent_context.push_str(&crate::agent::render_session_project_context(
        project_context
            .as_ref()
            .map(|project| project.root.as_path()),
        context_files,
    ));
    let prompt = if resume_session.is_some() {
        format!(
            "{}\n\n{}",
            request
                .messages
                .last()
                .map(|message| message.content.clone())
                .unwrap_or_default(),
            agent_context
        )
    } else {
        format!(
            "{}\n\n{}",
            agent_context,
            render_imported_conversation(&request.messages, request.context_window)
        )
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
        .arg("all")
        .arg("--disabled-tools")
        .arg("browser");
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
        // DRPA owns the run lifecycle and budgets. JCode's default auto-poke
        // starts extra autonomous turns for unfinished todos and can replay the
        // same work outside DRPA's model/tool accounting.
        .env("JCODE_RUN_AUTO_POKE", "0")
        .env("JCODE_RUN_AUTO_POKE_MAX_TURNS", "1")
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

    emit(AgentStreamEvent::RoundStarted { round: 1 });
    let mut state = StreamState::default();
    let (stdout_tx, stdout_rx) = mpsc::channel::<Result<Option<Vec<u8>>, String>>();
    let stdout_reader = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => {
                    let _ = stdout_tx.send(Ok(None));
                    break;
                }
                Ok(_) => {
                    if stdout_tx.send(Ok(Some(bytes))).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = stdout_tx.send(Err(format!("读取 JCode 事件流失败：{error}")));
                    break;
                }
            }
        }
    });
    let stream_result = (|| -> Result<(), String> {
        loop {
            control.check()?;
            let bytes = match stdout_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(Some(bytes))) => bytes,
                Ok(Ok(None)) => break,
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if child
                        .try_wait()
                        .map_err(|error| format!("检查 JCode 进程状态失败：{error}"))?
                        .is_some()
                    {
                        break;
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let line = String::from_utf8_lossy(&bytes);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed)
                .map_err(|error| format!("JCode 返回了无效 NDJSON 事件：{error} · {trimmed}"))?;
            consume_event(
                &value,
                request.stream,
                request.max_tool_calls,
                &mut state,
                &mut emit,
            )?;
        }
        Ok(())
    })();

    let guarded_stop_reason = state.stop_reason.clone();
    if stream_result.is_err() {
        terminate_child_process_tree(&mut child);
        let _ = child.wait();
    }

    let _ = stdout_reader.join();
    if let Some(stop_reason) = guarded_stop_reason {
        let _ = stderr_reader.join();
        let notice = if stop_reason == "repeated-tool-call" {
            "检测到 JCode 连续执行相同工具且结果没有变化，已自动停止本次运行以避免继续浪费时间和 tokens。"
        } else {
            "JCode 已达到本次运行的工具调用上限，已自动停止后续工具。"
        };
        let message = if state.message.trim().is_empty() {
            notice.to_owned()
        } else {
            format!("{}\n\n{notice}", state.message.trim())
        };
        if !request.session_id.trim().is_empty() && !state.session_id.trim().is_empty() {
            let mut completed_messages = request.messages.clone();
            completed_messages.push(AgentMessage {
                role: "assistant".to_owned(),
                content: message.clone(),
            });
            persist_session_state(
                &index_path,
                request.session_id.trim(),
                &state.session_id,
                &conversation_digest(&completed_messages),
            )?;
        }
        return Ok(AgentTurnResult {
            message,
            tools: ordered_tools(state.tools),
            usage: state.usage,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            stop_reason,
            rounds: 1,
            tool_calls: state.tool_calls,
            retry_count: 0,
            context_checkpoint: request.context_checkpoint.clone(),
        });
    }
    if let Err(error) = stream_result {
        if !request.session_id.trim().is_empty() && !state.session_id.trim().is_empty() {
            let _ = persist_session_state(
                &index_path,
                request.session_id.trim(),
                &state.session_id,
                &previous_digest,
            );
        }
        return Err(error);
    }

    let status = child
        .wait()
        .map_err(|error| format!("等待 JCode 退出失败：{error}"))?;
    let stderr = stderr_reader.join().unwrap_or_default().trim().to_owned();
    if !status.success() {
        if !request.session_id.trim().is_empty() && !state.session_id.trim().is_empty() {
            let _ = persist_session_state(
                &index_path,
                request.session_id.trim(),
                &state.session_id,
                &previous_digest,
            );
        }
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
        if !request.session_id.trim().is_empty() && !state.session_id.trim().is_empty() {
            let _ = persist_session_state(
                &index_path,
                request.session_id.trim(),
                &state.session_id,
                &previous_digest,
            );
        }
        return Err(format!("JCode 开发者 Agent 返回错误：{}", state.last_error));
    }
    if state.message.trim().is_empty() {
        if !request.session_id.trim().is_empty() && !state.session_id.trim().is_empty() {
            let _ = persist_session_state(
                &index_path,
                request.session_id.trim(),
                &state.session_id,
                &previous_digest,
            );
        }
        return Err("JCode 开发者 Agent 返回了空消息".to_owned());
    }

    if !request.session_id.trim().is_empty() && !state.session_id.trim().is_empty() {
        let mut completed_messages = request.messages.clone();
        completed_messages.push(AgentMessage {
            role: "assistant".to_owned(),
            content: state.message.clone(),
        });
        persist_session_state(
            &index_path,
            request.session_id.trim(),
            &state.session_id,
            &conversation_digest(&completed_messages),
        )?;
    }

    Ok(AgentTurnResult {
        message: state.message,
        tools: ordered_tools(state.tools),
        usage: state.usage,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        stop_reason: "completed".to_owned(),
        rounds: 1,
        tool_calls: state.tool_calls,
        retry_count: 0,
        context_checkpoint: request.context_checkpoint.clone(),
    })
}

fn terminate_child_process_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_child_window(&mut command);
        let _ = command.status();
    }
    let _ = child.kill();
}

fn consume_event<F>(
    value: &Value,
    stream: bool,
    max_tool_calls: usize,
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
            state.raw_message.push_str(text);
            let visible = crate::provider::visible_assistant_content(&state.raw_message);
            let visible_delta = visible
                .strip_prefix(&state.message)
                .unwrap_or_default()
                .to_owned();
            state.message = visible;
            if stream && !visible_delta.is_empty() {
                emit(AgentStreamEvent::Delta {
                    content: visible_delta,
                });
            }
        }
        "text_replace" => {
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            state.raw_message = text.to_owned();
            state.message = crate::provider::visible_assistant_content(text);
            if stream {
                emit(AgentStreamEvent::ContentReplace {
                    content: state.message.clone(),
                });
            }
        }
        "tool_start" => {
            let id = event_id(value);
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let (newly_observed, ordinal) = observe_tool(state, &id);
            if newly_observed {
                if state.tool_calls > max_tool_calls {
                    return stop_jcode_tool(
                        state,
                        stream,
                        emit,
                        id,
                        name,
                        "tool-call-limit",
                        format!("JCode 工具调用已达到上限 {max_tool_calls}，已停止后续调用。"),
                    );
                }
            }
            state.tool_names.insert(id.clone(), name.clone());
            state.tool_inputs.insert(id.clone(), String::new());
            state.tool_started_at.insert(id.clone(), Instant::now());
            state.current_tool_input_id = Some(id.clone());
            emit(AgentStreamEvent::Tool {
                tool: AgentToolEvent {
                    call_id: id,
                    name: format!("jcode:{name}"),
                    status: "running".to_owned(),
                    summary: "JCode 正在执行".to_owned(),
                    output: String::new(),
                    ordinal,
                    round: 1,
                    input: String::new(),
                    duration_ms: None,
                },
            });
        }
        "tool_input" => {
            if let Some(id) = state.current_tool_input_id.as_ref()
                && let Some(input) = state.tool_inputs.get_mut(id)
            {
                append_bounded(
                    input,
                    value
                        .get("delta")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    MAX_EVENT_TOOL_INPUT_BYTES,
                );
            }
        }
        "tool_exec" => {
            let id = event_id(value);
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| state.tool_names.get(&id).cloned())
                .unwrap_or_else(|| "tool".to_owned());
            state.tool_names.insert(id.clone(), name.clone());
            state.current_tool_input_id = None;
            let (newly_observed, ordinal) = observe_tool(state, &id);
            if newly_observed && state.tool_calls > max_tool_calls {
                return stop_jcode_tool(
                    state,
                    stream,
                    emit,
                    id,
                    name,
                    "tool-call-limit",
                    format!("JCode 工具调用已达到上限 {max_tool_calls}，已停止后续调用。"),
                );
            }
            if !state.tool_fingerprints.contains_key(&id) {
                let canonical_arguments = canonical_tool_arguments(
                    state
                        .tool_inputs
                        .get(&id)
                        .map(String::as_str)
                        .unwrap_or_default(),
                );
                if state
                    .tool_loop_guard
                    .should_block(&name, &canonical_arguments)
                {
                    return stop_jcode_tool(
                        state,
                        stream,
                        emit,
                        id,
                        name.clone(),
                        "repeated-tool-call",
                        format!("JCode 工具连续返回相同结果，已停止重复调用：{name}"),
                    );
                } else {
                    state
                        .tool_fingerprints
                        .insert(id.clone(), (name.clone(), canonical_arguments));
                }
            }
            let input = display_jcode_tool_input(
                &name,
                state
                    .tool_inputs
                    .get(&id)
                    .map(String::as_str)
                    .unwrap_or_default(),
            );
            emit(AgentStreamEvent::Tool {
                tool: AgentToolEvent {
                    call_id: id,
                    name: format!("jcode:{name}"),
                    status: "running".to_owned(),
                    summary: "JCode 正在执行".to_owned(),
                    output: String::new(),
                    ordinal,
                    round: 1,
                    input,
                    duration_ms: None,
                },
            });
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
            let status = if error.is_some() {
                "failed"
            } else {
                "completed"
            };
            if let Some((tool_name, canonical_arguments)) = state.tool_fingerprints.remove(&id) {
                state
                    .tool_loop_guard
                    .record(&tool_name, &canonical_arguments, status, &output);
            }
            let input = display_jcode_tool_input(
                &name,
                state
                    .tool_inputs
                    .get(&id)
                    .map(String::as_str)
                    .unwrap_or_default(),
            );
            let tool = AgentToolEvent {
                call_id: id.clone(),
                name: format!("jcode:{name}"),
                status: status.to_owned(),
                summary: error
                    .map(|item| truncate_text(item.to_string(), 500))
                    .unwrap_or_else(|| format!("JCode 已执行 {name}")),
                output,
                ordinal: state.tool_ordinals.get(&id).copied().unwrap_or(1),
                round: 1,
                input,
                duration_ms: state.tool_started_at.remove(&id).map(|started| {
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
                }),
            };
            state.tool_inputs.remove(&id);
            state.tools.insert(id, tool.clone());
            emit(AgentStreamEvent::Tool { tool });
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

#[allow(clippy::too_many_arguments)]
fn stop_jcode_tool<F>(
    state: &mut StreamState,
    _stream: bool,
    emit: &mut F,
    id: String,
    name: String,
    reason: &str,
    summary: String,
) -> Result<(), String>
where
    F: FnMut(AgentStreamEvent),
{
    state.stop_reason = Some(reason.to_owned());
    let ordinal = if let Some(ordinal) = state.tool_ordinals.get(&id).copied() {
        ordinal
    } else {
        let ordinal = state.tool_ordinals.len().saturating_add(1);
        state.tool_ordinals.insert(id.clone(), ordinal);
        ordinal
    };
    let input = display_jcode_tool_input(
        &name,
        state
            .tool_inputs
            .get(&id)
            .map(String::as_str)
            .unwrap_or_default(),
    );
    let duration_ms = state
        .tool_started_at
        .remove(&id)
        .map(|started| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
    let tool = AgentToolEvent {
        call_id: id.clone(),
        name: format!("jcode:{name}"),
        status: "failed".to_owned(),
        summary: summary.clone(),
        output: serde_json::json!({"ok": false, "error": summary}).to_string(),
        ordinal,
        round: 1,
        input,
        duration_ms,
    };
    state.tools.insert(id, tool.clone());
    emit(AgentStreamEvent::Tool { tool });
    Err(JCODE_LOOP_STOP_SIGNAL.to_owned())
}

fn observe_tool(state: &mut StreamState, id: &str) -> (bool, usize) {
    let ordinal = state.tool_ordinals.get(id).copied().unwrap_or_else(|| {
        let ordinal = state.tool_ordinals.len().saturating_add(1);
        state.tool_ordinals.insert(id.to_owned(), ordinal);
        ordinal
    });
    let newly_observed = state.observed_tool_ids.insert(id.to_owned());
    if newly_observed {
        state.tool_calls = state.tool_calls.saturating_add(1);
    }
    (newly_observed, ordinal)
}

fn display_jcode_tool_input(name: &str, input: &str) -> String {
    let value = serde_json::from_str(input).unwrap_or_else(|_| Value::String(input.to_owned()));
    persistent_tool_input(&format!("jcode:{name}"), &value)
}

fn ordered_tools(tools: BTreeMap<String, AgentToolEvent>) -> Vec<AgentToolEvent> {
    let mut tools = tools.into_values().collect::<Vec<_>>();
    tools.sort_by_key(|tool| tool.ordinal);
    tools
}

fn event_id(value: &Value) -> String {
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("jcode-tool-{}", uuid::Uuid::new_v4()))
}

fn append_bounded(target: &mut String, fragment: &str, max_bytes: usize) {
    if target.len() >= max_bytes || fragment.is_empty() {
        return;
    }
    let mut remaining = max_bytes.saturating_sub(target.len()).min(fragment.len());
    while !fragment.is_char_boundary(remaining) {
        remaining -= 1;
    }
    target.push_str(&fragment[..remaining]);
}

fn canonical_tool_arguments(input: &str) -> String {
    serde_json::from_str::<Value>(input)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| input.trim().to_owned())
}

fn locate_jcode(workspace_root: &Path, resource_dir: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("DRPA_JCODE_PATH") {
        let path = PathBuf::from(path);
        if is_executable_file(&path) {
            return Ok(path);
        }
        return Err(format!(
            "DRPA_JCODE_PATH 指向的文件不存在或不可执行：{}",
            path.display()
        ));
    }

    let executable_name = jcode_executable_name();
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
        .find(|path| is_executable_file(path))
        .ok_or_else(|| {
            format!(
                "未找到内置 JCode。完整安装包应包含 jcode/{executable_name}；本地开发可设置 DRPA_JCODE_PATH。"
            )
        })
}

fn jcode_executable_name() -> &'static str {
    if cfg!(windows) { "jcode.exe" } else { "jcode" }
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .map(|root| root.join(name))
        .find(|path| is_executable_file(path))
}

fn write_provider_config(home: &Path, request: &AgentTurnRequest) -> Result<(), String> {
    let base_url = request.base_url.trim().trim_end_matches('/');
    if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err("OpenAI 兼容 URL 必须是有效的 http(s) 地址".to_owned());
    }
    let base_url = json_string(base_url)?;
    let model = json_string(request.model.trim())?;
    let reasoning_split = if request
        .model
        .trim()
        .to_ascii_lowercase()
        .contains("minimax")
        || request
            .base_url
            .trim()
            .to_ascii_lowercase()
            .contains("minimax")
    {
        "reasoning_split = true\n"
    } else {
        ""
    };
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
         {reasoning_split}\
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

fn write_mcp_config(
    home: &Path,
    python: &Path,
    browser: Option<&Path>,
    browser_session: Option<&AgentBrowserSession>,
    bridge_endpoint: &str,
    bridge_token: &str,
) -> Result<(), String> {
    if python.as_os_str().is_empty() {
        return Err("JCode 的 DRPA 工具桥需要内置 Python 运行时".to_owned());
    }
    let browser_session = browser_session.ok_or_else(|| "JCode 浏览器会话尚未初始化".to_owned())?;
    let mut environment = serde_json::Map::from_iter([
        (
            "PYTHONIOENCODING".to_owned(),
            Value::String("utf-8".to_owned()),
        ),
        ("PYTHONUTF8".to_owned(), Value::String("1".to_owned())),
        (
            "DRPA_BROWSER_PROFILE_ROOT".to_owned(),
            Value::String(browser_session.profile_root.display().to_string()),
        ),
        (
            "DRPA_BROWSER_PORT".to_owned(),
            Value::String(browser_session.port.to_string()),
        ),
        (
            "DRPA_AGENT_ARTIFACT_ROOT".to_owned(),
            Value::String(browser_session.artifact_root.display().to_string()),
        ),
        (
            "DRPA_AGENT_BRIDGE_ENDPOINT".to_owned(),
            Value::String(bridge_endpoint.to_owned()),
        ),
        (
            "DRPA_AGENT_BRIDGE_TOKEN".to_owned(),
            Value::String(bridge_token.to_owned()),
        ),
    ]);
    if let Some(browser) = browser {
        environment.insert(
            "DRPA_BROWSER_PATH".to_owned(),
            Value::String(browser.display().to_string()),
        );
    }
    #[cfg(debug_assertions)]
    {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../runtime/python/src");
        if source.is_dir() {
            environment.insert(
                "PYTHONPATH".to_owned(),
                Value::String(source.display().to_string()),
            );
        }
    }
    let config = serde_json::json!({
        "mcpServers": {
            "drpa": {
                "command": python.display().to_string(),
                "args": ["-m", "drpa_runner.agent_mcp"],
                "env": environment,
                "shared": true
            }
        }
    });
    let bytes = serde_json::to_vec_pretty(&config)
        .map_err(|error| format!("生成 JCode MCP 配置失败：{error}"))?;
    fs::write(home.join("mcp.json"), bytes)
        .map_err(|error| format!("写入 JCode MCP 配置失败：{error}"))
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
        "这是 DRPA 会话导入的上下文。你是 JCode 开发者 Agent，可直接使用完整工具集在当前工作目录完成最后一条用户请求。浏览器操作使用 mcp__drpa__browser_*，不要调用 JCode 自带的 Firefox browser setup；运行 RPAZ 包使用 mcp__drpa__rpaz_run_package，并用 mcp__drpa__run_get_detail 查看实时事件和 debug 日志。凭据保险箱已由用户验证解锁时，可使用 mcp__drpa__vault_list_credentials、mcp__drpa__vault_get_credential 和 mcp__drpa__vault_upsert_credential。\n\n",
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

fn persist_session_state(
    index_path: &Path,
    drpa_session_id: &str,
    jcode_session_id: &str,
    conversation_digest: &str,
) -> Result<(), String> {
    let _guard = JCODE_SESSION_INDEX_LOCK
        .lock()
        .map_err(|_| "JCode 会话索引状态已损坏".to_owned())?;
    let mut index = read_session_index(index_path)?;
    index.sessions.insert(
        drpa_session_id.to_owned(),
        SessionState {
            jcode_session_id: jcode_session_id.to_owned(),
            conversation_digest: conversation_digest.to_owned(),
        },
    );
    write_session_index(index_path, &index)
}

pub(crate) fn delete_session_data(workspace_root: &Path, session_id: &str) -> Result<(), String> {
    let developer_root = workspace_root.join("agent").join("jcode");
    let home = developer_root.join("homes").join(session_id);
    if home.is_dir() {
        fs::remove_dir_all(&home).map_err(|error| format!("清理 JCode 会话目录失败：{error}"))?;
    }
    let index_path = developer_root.join("session-index.json");
    let _guard = JCODE_SESSION_INDEX_LOCK
        .lock()
        .map_err(|_| "JCode 会话索引状态已损坏".to_owned())?;
    let mut index = read_session_index(&index_path)?;
    if index.sessions.remove(session_id).is_some() {
        write_session_index(&index_path, &index)?;
    }
    Ok(())
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

    fn consume_test_event(
        state: &mut StreamState,
        max_tool_calls: usize,
        value: Value,
    ) -> Result<(), String> {
        consume_event(&value, false, max_tool_calls, state, &mut |_| {})
    }

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
            max_tool_calls: 128,
            max_wall_time_seconds: 900,
            selected_skill_ids: Vec::new(),
            tool_policy: crate::agent::AgentToolPolicy::default(),
            context_checkpoint: None,
            messages: vec![AgentMessage {
                role: "user".to_owned(),
                content: "inspect the project".to_owned(),
            }],
        }
    }

    #[test]
    fn bundled_jcode_name_matches_the_target_platform() {
        let expected = if cfg!(windows) { "jcode.exe" } else { "jcode" };
        assert_eq!(jcode_executable_name(), expected);
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
    fn provider_config_separates_minimax_reasoning_from_answer_text() {
        let root = env::temp_dir().join(format!("drpa-jcode-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut minimax = request("token");
        minimax.model = "minimax-m3".to_owned();
        write_provider_config(&root, &minimax).unwrap();
        let config = fs::read_to_string(root.join("config.toml")).unwrap();

        assert!(config.contains("reasoning_split = true"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_config_uses_bundled_python_chrome_and_host_bridge() {
        let root = env::temp_dir().join(format!("drpa-jcode-mcp-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        write_mcp_config(
            &root,
            Path::new("C:/DRPA/runtime/python.exe"),
            Some(Path::new("C:/DRPA/runtime/chrome.exe")),
            Some(&AgentBrowserSession {
                port: 43_124,
                profile_root: PathBuf::from("C:/DRPA/browser/session"),
                artifact_root: PathBuf::from("C:/DRPA/agent/session/browser"),
            }),
            "127.0.0.1:43123",
            "test-token",
        )
        .unwrap();
        let config: Value =
            serde_json::from_slice(&fs::read(root.join("mcp.json")).unwrap()).unwrap();

        assert_eq!(
            config
                .pointer("/mcpServers/drpa/command")
                .and_then(Value::as_str),
            Some("C:/DRPA/runtime/python.exe")
        );
        assert_eq!(
            config
                .pointer("/mcpServers/drpa/env/DRPA_BROWSER_PATH")
                .and_then(Value::as_str),
            Some("C:/DRPA/runtime/chrome.exe")
        );
        assert_eq!(
            config
                .pointer("/mcpServers/drpa/env/DRPA_BROWSER_PORT")
                .and_then(Value::as_str),
            Some("43124")
        );
        assert_eq!(
            config
                .pointer("/mcpServers/drpa/env/DRPA_AGENT_BRIDGE_ENDPOINT")
                .and_then(Value::as_str),
            Some("127.0.0.1:43123")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ndjson_events_are_mapped_to_drpa_streams() {
        let mut state = StreamState::default();
        let mut emitted = Vec::new();
        consume_event(
            &serde_json::json!({"type":"start","session_id":"fox"}),
            true,
            128,
            &mut state,
            &mut |event| emitted.push(event),
        )
        .unwrap();
        consume_event(
            &serde_json::json!({"type":"text_delta","text":"done"}),
            true,
            128,
            &mut state,
            &mut |event| emitted.push(event),
        )
        .unwrap();
        consume_event(
            &serde_json::json!({"type":"tool_done","id":"1","name":"bash","output":"ok","error":null}),
            true,
            128,
            &mut state,
            &mut |event| emitted.push(event),
        )
        .unwrap();
        consume_event(
            &serde_json::json!({"type":"tokens","input":12,"output":4}),
            true,
            128,
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
    fn ndjson_stops_a_repeated_tool_after_unchanged_results() {
        let mut state = StreamState::default();
        for index in 0..3 {
            let id = format!("call-{index}");
            consume_test_event(
                &mut state,
                128,
                serde_json::json!({"type":"tool_start","id":id,"name":"read"}),
            )
            .unwrap();
            consume_test_event(
                &mut state,
                128,
                serde_json::json!({"type":"tool_input","delta":"{\"path\":\"a.txt\"}"}),
            )
            .unwrap();
            consume_test_event(
                &mut state,
                128,
                serde_json::json!({"type":"tool_exec","id":id,"name":"read"}),
            )
            .unwrap();
            consume_test_event(
                &mut state,
                128,
                serde_json::json!({"type":"tool_done","id":id,"name":"read","output":"same","error":null}),
            )
            .unwrap();
        }

        consume_test_event(
            &mut state,
            128,
            serde_json::json!({"type":"tool_start","id":"call-blocked","name":"read"}),
        )
        .unwrap();
        consume_test_event(
            &mut state,
            128,
            serde_json::json!({"type":"tool_input","delta":"{\"path\":\"a.txt\"}"}),
        )
        .unwrap();
        let error = consume_test_event(
            &mut state,
            128,
            serde_json::json!({"type":"tool_exec","id":"call-blocked","name":"read"}),
        )
        .unwrap_err();

        assert_eq!(error, JCODE_LOOP_STOP_SIGNAL);
        assert_eq!(state.stop_reason.as_deref(), Some("repeated-tool-call"));
        assert_eq!(state.tool_calls, 4);
        assert_eq!(state.tools["call-blocked"].status, "failed");
    }

    #[test]
    fn ndjson_enforces_drpa_tool_call_budget() {
        let mut state = StreamState::default();
        for id in ["first", "second"] {
            let started = consume_test_event(
                &mut state,
                1,
                serde_json::json!({"type":"tool_start","id":id,"name":"bash"}),
            );
            if id == "second" {
                assert_eq!(started.unwrap_err(), JCODE_LOOP_STOP_SIGNAL);
                break;
            }
            started.unwrap();
            let result = consume_test_event(
                &mut state,
                1,
                serde_json::json!({"type":"tool_exec","id":id,"name":"bash"}),
            );
            result.unwrap();
        }

        assert_eq!(state.stop_reason.as_deref(), Some("tool-call-limit"));
        assert_eq!(state.tool_calls, 2);
    }

    #[test]
    fn ndjson_thinking_tags_are_kept_out_of_visible_text_events() {
        let mut state = StreamState::default();
        let mut emitted = Vec::new();
        for text in ["<thi", "nk>private plan", "</think>Final answer"] {
            consume_event(
                &serde_json::json!({"type":"text_delta","text":text}),
                true,
                128,
                &mut state,
                &mut |event| emitted.push(event),
            )
            .unwrap();
        }

        assert_eq!(state.message, "Final answer");
        assert_eq!(emitted.len(), 1);
        assert!(matches!(
            &emitted[0],
            AgentStreamEvent::Delta { content } if content == "Final answer"
        ));
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
