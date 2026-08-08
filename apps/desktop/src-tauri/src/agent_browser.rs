use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const FIRST_AGENT_BROWSER_PORT: u16 = 20_000;
const LAST_AGENT_BROWSER_PORT: u16 = 54_999;

#[derive(Debug, Clone)]
pub(crate) struct AgentBrowserSession {
    pub(crate) port: u16,
    pub(crate) profile_root: PathBuf,
    pub(crate) artifact_root: PathBuf,
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
        Ok(AgentBrowserSession {
            port,
            profile_root,
            artifact_root,
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
        Ok(())
    }
}

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
