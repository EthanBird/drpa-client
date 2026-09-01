use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use drpa_install::ComponentLeaseGuard;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const FIRST_AGENT_BROWSER_PORT: u16 = 20_000;
const LAST_AGENT_BROWSER_PORT: u16 = 54_999;

#[derive(Clone)]
pub(crate) struct AgentBrowserSession {
    pub(crate) port: u16,
    pub(crate) profile_root: PathBuf,
    pub(crate) artifact_root: PathBuf,
    bridge: Arc<Mutex<Option<BrowserHost>>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserSessionIndex {
    #[serde(default)]
    sessions: HashMap<String, u16>,
}

#[derive(Clone)]
pub(crate) struct AgentBrowserManager {
    workspace_root: PathBuf,
    sessions: Arc<Mutex<HashMap<String, u16>>>,
    bridges: Arc<Mutex<HashMap<String, Arc<Mutex<Option<BrowserHost>>>>>>,
}

impl AgentBrowserManager {
    pub(crate) fn with_workspace(workspace_root: &Path) -> Self {
        let index = read_index(workspace_root).unwrap_or_default();
        let mut seen = HashSet::new();
        let sessions = index
            .sessions
            .into_iter()
            .filter(|(session_id, port)| {
                valid_session_id(session_id)
                    && (*port >= FIRST_AGENT_BROWSER_PORT && *port <= LAST_AGENT_BROWSER_PORT)
                    && seen.insert(*port)
            })
            .collect();
        Self {
            workspace_root: workspace_root.to_path_buf(),
            sessions: Arc::new(Mutex::new(sessions)),
            bridges: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn session(&self, session_id: &str) -> Result<AgentBrowserSession, String> {
        if !valid_session_id(session_id) {
            return Err("Agent 浏览器会话 ID 无效".to_owned());
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "Agent 浏览器会话索引已损坏".to_owned())?;
        let port = if let Some(port) = sessions.get(session_id) {
            *port
        } else {
            let port = allocate_port(session_id, sessions.values().copied())?;
            sessions.insert(session_id.to_owned(), port);
            write_index(&self.workspace_root, &sessions)?;
            port
        };
        let profile_root = self
            .workspace_root
            .join("browser")
            .join("agent")
            .join(session_id);
        let artifact_root = self
            .workspace_root
            .join("agent")
            .join("sessions")
            .join(session_id)
            .join("browser");
        fs::create_dir_all(&profile_root)
            .and_then(|_| fs::create_dir_all(&artifact_root))
            .map_err(|error| format!("创建 Agent 浏览器会话目录失败：{error}"))?;
        let bridge = self
            .bridges
            .lock()
            .map_err(|_| "Agent 浏览器 Host 索引已损坏".to_owned())?
            .entry(session_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .clone();
        Ok(AgentBrowserSession {
            port,
            profile_root,
            artifact_root,
            bridge,
        })
    }

    pub(crate) fn release(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "Agent 浏览器会话索引已损坏".to_owned())?;
        if sessions.remove(session_id).is_some() {
            write_index(&self.workspace_root, &sessions)?;
        }
        drop(sessions);
        if let Some(bridge) = self
            .bridges
            .lock()
            .map_err(|_| "Agent 浏览器 Host 索引已损坏".to_owned())?
            .remove(session_id)
            && let Ok(mut host) = bridge.lock()
        {
            *host = None;
        }
        Ok(())
    }

    pub(crate) fn reset_hosts(&self) -> Result<(), String> {
        let mut bridges = self
            .bridges
            .lock()
            .map_err(|_| "Agent 浏览器 Host 索引已损坏".to_owned())?;
        for bridge in bridges.values() {
            if let Ok(mut host) = bridge.lock() {
                *host = None;
            }
        }
        bridges.clear();
        Ok(())
    }
}

impl AgentBrowserSession {
    #[cfg(test)]
    pub(crate) fn for_test(port: u16, profile_root: PathBuf, artifact_root: PathBuf) -> Self {
        Self {
            port,
            profile_root,
            artifact_root,
            bridge: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn call(
        &self,
        python: &Path,
        browser: Option<&Path>,
        component_leases: Vec<ComponentLeaseGuard>,
        name: &str,
        arguments: &Value,
        timeout: Duration,
        mut check_control: impl FnMut() -> Result<(), String>,
    ) -> Result<Value, String> {
        let mut slot = self
            .bridge
            .lock()
            .map_err(|_| "Agent 浏览器 Host 状态已损坏".to_owned())?;
        let requires_restart = slot
            .as_ref()
            .is_some_and(|host| !host.matches_runtime(python, browser));
        if requires_restart {
            *slot = None;
        }
        if slot.is_none() {
            *slot = Some(BrowserHost::spawn(python, browser, component_leases, self)?);
        }
        let result = slot.as_mut().expect("browser host initialized").call(
            name,
            arguments,
            timeout,
            &mut check_control,
        );
        match result {
            Ok(value) => Ok(value),
            Err((error, fatal)) => {
                if fatal {
                    *slot = None;
                }
                Err(error)
            }
        }
    }
}

struct BrowserHost {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    responses: Receiver<Result<Value, String>>,
    stderr_tail: Arc<Mutex<String>>,
    python: PathBuf,
    browser: Option<PathBuf>,
    next_id: u64,
    _component_leases: Vec<ComponentLeaseGuard>,
}

impl BrowserHost {
    fn spawn(
        python: &Path,
        browser: Option<&Path>,
        component_leases: Vec<ComponentLeaseGuard>,
        session: &AgentBrowserSession,
    ) -> Result<Self, String> {
        let mut command = Command::new(python);
        command
            .args(["-m", "drpa_runner.agent_mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .env("DRPA_BROWSER_PROFILE_ROOT", &session.profile_root)
            .env("DRPA_BROWSER_PORT", session.port.to_string())
            .env("DRPA_AGENT_ARTIFACT_ROOT", &session.artifact_root);
        if let Some(browser) = browser {
            command.env("DRPA_BROWSER_PATH", browser);
        }
        hide_browser_host_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("启动持久化 Browser Host 失败：{error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "无法连接 Browser Host 输入".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "无法连接 Browser Host 输出".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "无法连接 Browser Host 错误输出".to_owned())?;
        let (sender, responses) = mpsc::sync_channel(16);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let response = line
                    .map_err(|error| format!("读取 Browser Host 响应失败：{error}"))
                    .and_then(|line| {
                        serde_json::from_str(&line)
                            .map_err(|error| format!("Browser Host 返回无效 JSON：{error}"))
                    });
                if sender.send(response).is_err() {
                    break;
                }
            }
        });
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let stderr_output = Arc::clone(&stderr_tail);
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Ok(mut tail) = stderr_output.lock() {
                    tail.push_str(&line);
                    tail.push('\n');
                    if tail.len() > 16_384 {
                        let mut boundary = tail.len() - 16_384;
                        while boundary < tail.len() && !tail.is_char_boundary(boundary) {
                            boundary += 1;
                        }
                        tail.drain(..boundary);
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            responses,
            stderr_tail,
            python: python.to_path_buf(),
            browser: browser.map(Path::to_path_buf),
            next_id: 1,
            _component_leases: component_leases,
        })
    }

    fn matches_runtime(&self, python: &Path, browser: Option<&Path>) -> bool {
        self.python == python && self.browser.as_deref() == browser
    }

    fn call(
        &mut self,
        name: &str,
        arguments: &Value,
        timeout: Duration,
        check_control: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<Value, (String, bool)> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        });
        serde_json::to_writer(&mut self.stdin, &request)
            .map_err(|error| (format!("编码 Browser Host 请求失败：{error}"), true))?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|error| (format!("发送 Browser Host 请求失败：{error}"), true))?;
        let deadline = Instant::now() + timeout;
        loop {
            check_control().map_err(|error| (error, true))?;
            let now = Instant::now();
            if now >= deadline {
                return Err((format!("Browser Host 工具 {name} 执行超时"), true));
            }
            let wait = (deadline - now).min(Duration::from_millis(100));
            let response = match self.responses.recv_timeout(wait) {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => return Err((error, true)),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    let stderr = self
                        .stderr_tail
                        .lock()
                        .map(|tail| tail.trim().to_owned())
                        .unwrap_or_default();
                    let detail = if stderr.is_empty() {
                        "进程已退出".to_owned()
                    } else {
                        stderr
                    };
                    return Err((format!("Browser Host 连接中断：{detail}"), true));
                }
            };
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                return Err(("Browser Host 响应 ID 不匹配".to_owned(), true));
            }
            if let Some(error) = response.get("error") {
                return Err((format!("Browser Host 协议错误：{error}"), false));
            }
            let result = response
                .get("result")
                .ok_or_else(|| ("Browser Host 响应缺少 result".to_owned(), true))?;
            if result.get("isError").and_then(Value::as_bool) == Some(true) {
                let message = result
                    .get("content")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("Browser Host 工具执行失败");
                return Err((message.to_owned(), false));
            }
            if let Some(value) = result.get("structuredContent") {
                return Ok(value.clone());
            }
            let text = result
                .get("content")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("text"))
                .and_then(Value::as_str)
                .ok_or_else(|| ("Browser Host 响应没有结构化内容".to_owned(), false))?;
            return serde_json::from_str(text)
                .map_err(|error| (format!("Browser Host 文本结果不是 JSON：{error}"), false));
        }
    }
}

impl Drop for BrowserHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
fn hide_browser_host_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_browser_host_window(_command: &mut Command) {}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 96
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn allocate_port(
    session_id: &str,
    allocated: impl IntoIterator<Item = u16>,
) -> Result<u16, String> {
    let allocated = allocated.into_iter().collect::<HashSet<_>>();
    let range = u32::from(LAST_AGENT_BROWSER_PORT - FIRST_AGENT_BROWSER_PORT) + 1;
    let mut hash = 2_166_136_261u32;
    for byte in session_id.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    for offset in 0..range {
        let port = FIRST_AGENT_BROWSER_PORT
            + u16::try_from((hash.wrapping_add(offset)) % range).unwrap_or_default();
        if !allocated.contains(&port) && !port_is_listening(port) {
            return Ok(port);
        }
    }
    Err("没有可用的 Agent Chrome 调试端口".to_owned())
}

fn port_is_listening(port: u16) -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&address, Duration::from_millis(8)).is_ok()
}

fn index_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("agent").join("browser-sessions.json")
}

fn read_index(workspace_root: &Path) -> Result<BrowserSessionIndex, String> {
    let path = index_path(workspace_root);
    if !path.is_file() {
        return Ok(BrowserSessionIndex::default());
    }
    let bytes = fs::read(&path).map_err(|error| format!("读取浏览器会话索引失败：{error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("浏览器会话索引已损坏：{error}"))
}

fn write_index(workspace_root: &Path, sessions: &HashMap<String, u16>) -> Result<(), String> {
    let path = index_path(workspace_root);
    let parent = path
        .parent()
        .ok_or_else(|| "浏览器会话索引路径无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建 Agent 数据目录失败：{error}"))?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&BrowserSessionIndex {
        sessions: sessions.clone(),
    })
    .map_err(|error| format!("编码浏览器会话索引失败：{error}"))?;
    fs::write(&temporary, bytes).map_err(|error| format!("写入浏览器会话索引失败：{error}"))?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("替换浏览器会话索引失败：{error}"))?;
    }
    fs::rename(&temporary, &path).map_err(|error| format!("提交浏览器会话索引失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_stable_isolated_browser_sessions() {
        let root =
            std::env::temp_dir().join(format!("drpa-browser-manager-{}", uuid::Uuid::new_v4()));
        let manager = AgentBrowserManager::with_workspace(&root);
        let first = manager.session("session-one").unwrap();
        let again = manager.session("session-one").unwrap();
        let second = manager.session("session-two").unwrap();
        assert_eq!(first.port, again.port);
        assert_eq!(first.profile_root, again.profile_root);
        assert_ne!(first.port, second.port);
        assert_ne!(first.profile_root, second.profile_root);

        let restored = AgentBrowserManager::with_workspace(&root)
            .session("session-one")
            .unwrap();
        assert_eq!(first.port, restored.port);
        manager.release("session-one").unwrap();
        let index = read_index(&root).unwrap();
        assert!(!index.sessions.contains_key("session-one"));
        let _ = fs::remove_dir_all(root);
    }
}
