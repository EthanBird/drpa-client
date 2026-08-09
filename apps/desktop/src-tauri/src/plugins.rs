use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::State;
use url::Url;
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::AppPaths;

const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_PLUGIN_FILES: usize = 4096;
const MAX_PLUGIN_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PLUGIN_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_LOG_LINES: usize = 1000;
const MAX_LOG_LINE_BYTES: usize = 64 * 1024;
const MAX_DEBUG_REQUEST_BYTES: usize = 64 * 1024;
const MAX_DEBUG_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DEBUG_TIMEOUT_SECONDS: u64 = 30;

const BUILTIN_DIFY2API_MANIFEST: &str =
    include_str!("../../../../plugins/builtin/dify2api/plugin.yaml");
const BUILTIN_DIFY2API_CONFIG_SCHEMA: &str =
    include_str!("../../../../plugins/builtin/dify2api/config.schema.json");
const BUILTIN_DIFY2API_README: &str =
    include_str!("../../../../plugins/builtin/dify2api/README.md");
const BUILTIN_DIFY2API_NOTICES: &str =
    include_str!("../../../../plugins/builtin/dify2api/THIRD_PARTY_NOTICES.txt");
const BUILTIN_DIFY2API_PROVENANCE: &str =
    include_str!("../../../../plugins/builtin/dify2api/BUILD-PROVENANCE.md");
const BUILTIN_DIFY2API_MARKER: &str = "dify2api@1.0.0;bundle=2026-07-31.1";
#[cfg(target_os = "windows")]
const BUILTIN_DIFY2API_SERVICE: &[u8] =
    include_bytes!("../../../../plugins/builtin/dify2api/service/dify2api-server.exe");
#[cfg(target_os = "windows")]
const BUILTIN_DIFY2API_SERVICE_NAME: &str = "service/dify2api-server.exe";
#[cfg(target_os = "linux")]
const BUILTIN_DIFY2API_SERVICE: &[u8] =
    include_bytes!("../../../../plugins/builtin/dify2api/service/dify2api-server");
#[cfg(target_os = "linux")]
const BUILTIN_DIFY2API_SERVICE_NAME: &str = "service/dify2api-server";
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const BUILTIN_DIFY2API_SERVICE: &[u8] = &[];
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const BUILTIN_DIFY2API_SERVICE_NAME: &str = "";
static INITIALIZED_PLUGIN_ROOTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
#[cfg(target_os = "linux")]
static BUILTIN_DIFY2API_EXECUTION_ENTRY: OnceLock<Result<PathBuf, String>> = OnceLock::new();
const PLUGIN_PYTHON_RUNNER: &str = r#"
import importlib.util
import inspect
import json
import pathlib
import sys

entry_path = pathlib.Path(sys.argv[1]).resolve()
callable_name = sys.argv[2]
spec = importlib.util.spec_from_file_location("drpa_plugin_tool", entry_path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
function = getattr(module, callable_name)
request = json.load(sys.stdin)
arguments = request.get("arguments", {})
context = request.get("context", {})
parameters = inspect.signature(function).parameters
result = function(arguments, context) if len(parameters) >= 2 else function(arguments)
json.dump({"ok": True, "result": result}, sys.stdout, ensure_ascii=False)
"#;

const TOOL_PLUGIN_TEMPLATE: &str = r#"schema: 1
id: {id}
name: {name}
version: 0.1.0
description: DRPA 本地工具插件。
types: [tool-provider]
tools:
  - name: example
    description: 返回调用参数和插件上下文。
    runtime: bundled-python
    entry: tools/example.py:run
    timeout_seconds: 30
    parameters:
      type: object
      properties:
        text: { type: string }
      required: [text]
      additionalProperties: false
default_config: {}
"#;

const SERVICE_PLUGIN_TEMPLATE: &str = r#"schema: 1
id: {id}
name: {name}
version: 0.1.0
description: DRPA 本地 OpenAI 兼容服务插件。
types: [provider-adapter, service]
service:
  runtime: bundled-python
  entry: service/main.py
  transport: http
  endpoint: http://127.0.0.1:{port}/v1
  healthcheck: http://127.0.0.1:{port}/v1/health
tools: []
default_config:
  port: 34201
  model: local-plugin
"#;

const BUNDLE_PLUGIN_TEMPLATE: &str = r#"schema: 1
id: {id}
name: {name}
version: 0.1.0
description: DRPA 组合插件（服务、Provider、工具与调试入口）。
types: [provider-adapter, service, tool-provider, debugger]
config_schema: config.schema.json
services:
  - id: api
    title: 本地 API
    primary: true
    runtime: bundled-python
    entry: service/main.py
    transport: http
    endpoint: http://127.0.0.1:{config.port}/v1
    health:
      kind: http
      endpoint: http://127.0.0.1:{config.port}/v1/health
providers:
  - id: openai
    title: OpenAI 兼容 Provider
    protocol: openai
    service_id: api
    model_config_key: model
tool_providers:
  - id: python
    runtime: bundled-python
    entry: tools/example.py
    tools:
      - name: example
        description: 返回调用参数和插件上下文。
        callable: run
        parameters:
          type: object
          properties:
            text: { type: string }
          required: [text]
          additionalProperties: false
debugger:
  endpoints:
    - id: health
      title: 服务健康
      kind: health
      method: GET
      endpoint: http://127.0.0.1:{config.port}/v1/health
default_config:
  port: 34201
  model: local-plugin
"#;

const TOOL_PYTHON_TEMPLATE: &str = r#"def run(arguments, context):
    return {
        "text": arguments["text"],
        "plugin_root": context.get("pluginRoot"),
    }
"#;

const SERVICE_PYTHON_TEMPLATE: &str = r#"from __future__ import annotations

import json
import os
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

STATE_PATH = Path(os.environ["DRPA_PLUGIN_CONFIG"])


def config():
    return json.loads(STATE_PATH.read_text(encoding="utf-8")).get("config", {})


class Handler(BaseHTTPRequestHandler):
    def send_json(self, status, value):
        body = json.dumps(value, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        settings = config()
        if self.path.rstrip("/") in {"/health", "/v1/health"}:
            self.send_json(200, {"status": "ok"})
        elif self.path.rstrip("/") == "/v1/models":
            self.send_json(200, {"object": "list", "data": [{"id": settings.get("model", "local-plugin"), "object": "model"}]})
        else:
            self.send_json(404, {"error": {"message": "Not found"}})

    def do_POST(self):
        if self.path.rstrip("/") != "/v1/chat/completions":
            self.send_json(404, {"error": {"message": "Not found"}})
            return
        length = int(self.headers.get("Content-Length", "0"))
        payload = json.loads(self.rfile.read(length).decode("utf-8"))
        messages = payload.get("messages", [])
        content = str(messages[-1].get("content", "")) if messages else ""
        settings = config()
        self.send_json(200, {
            "id": f"chatcmpl-{uuid.uuid4().hex}",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": payload.get("model") or settings.get("model", "local-plugin"),
            "choices": [{"index": 0, "message": {"role": "assistant", "content": f"Plugin received: {content}"}, "finish_reason": "stop"}],
        })


if __name__ == "__main__":
    settings = config()
    ThreadingHTTPServer(("127.0.0.1", int(settings.get("port", 34201))), Handler).serve_forever()
"#;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginManifest {
    pub(crate) schema: u32,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) types: Vec<String>,
    #[serde(default)]
    pub(crate) service: Option<PluginServiceManifest>,
    #[serde(default)]
    pub(crate) services: Vec<PluginServiceManifest>,
    #[serde(default)]
    pub(crate) tools: Vec<PluginToolManifest>,
    #[serde(default)]
    pub(crate) tool_providers: Vec<PluginToolProviderManifest>,
    #[serde(default)]
    pub(crate) providers: Vec<PluginProviderManifest>,
    #[serde(default)]
    pub(crate) debugger: PluginDebuggerManifest,
    #[serde(default)]
    pub(crate) config_schema: Value,
    #[serde(default)]
    pub(crate) default_config: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginServiceManifest {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) primary: bool,
    pub(crate) runtime: String,
    #[serde(default)]
    pub(crate) entry: String,
    #[serde(default)]
    pub(crate) entry_by_platform: HashMap<String, String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default)]
    pub(crate) transport: String,
    #[serde(default)]
    pub(crate) endpoint: String,
    #[serde(default)]
    pub(crate) healthcheck: String,
    #[serde(default)]
    pub(crate) test_endpoint: String,
    #[serde(default)]
    pub(crate) health: Option<PluginHealthManifest>,
    #[serde(default)]
    pub(crate) logs: PluginServiceLogsManifest,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginHealthManifest {
    #[serde(default = "default_http_health_kind")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) endpoint: String,
    #[serde(default = "default_health_startup_timeout")]
    pub(crate) startup_timeout_seconds: u64,
    #[serde(default = "default_health_request_timeout")]
    pub(crate) request_timeout_millis: u64,
    #[serde(default = "default_health_interval")]
    pub(crate) interval_millis: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginServiceLogsManifest {
    #[serde(default = "default_true")]
    pub(crate) stdout: bool,
    #[serde(default = "default_true")]
    pub(crate) stderr: bool,
    #[serde(default = "default_log_format")]
    pub(crate) format: String,
}

impl Default for PluginServiceLogsManifest {
    fn default() -> Self {
        Self {
            stdout: true,
            stderr: true,
            format: default_log_format(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginToolManifest {
    pub(crate) name: String,
    pub(crate) description: String,
    #[serde(default = "default_object_schema")]
    pub(crate) parameters: Value,
    pub(crate) runtime: String,
    pub(crate) entry: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default = "default_tool_timeout")]
    pub(crate) timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginToolProviderManifest {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) title: String,
    pub(crate) runtime: String,
    pub(crate) entry: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default = "default_tool_timeout")]
    pub(crate) timeout_seconds: u64,
    #[serde(default)]
    pub(crate) tools: Vec<PluginProvidedToolManifest>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginProvidedToolManifest {
    pub(crate) name: String,
    pub(crate) description: String,
    #[serde(default = "default_object_schema")]
    pub(crate) parameters: Value,
    #[serde(default)]
    pub(crate) callable: String,
    #[serde(default)]
    pub(crate) timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginProviderManifest {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default = "default_provider_protocol")]
    pub(crate) protocol: String,
    pub(crate) service_id: String,
    #[serde(default)]
    pub(crate) endpoint: String,
    #[serde(default)]
    pub(crate) model_config_key: String,
    #[serde(default)]
    pub(crate) api_key_config_key: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub(crate) struct PluginDebuggerManifest {
    #[serde(default)]
    pub(crate) endpoints: Vec<PluginDebuggerEndpointManifest>,
    #[serde(default)]
    pub(crate) panels: Vec<PluginDebuggerPanelManifest>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub(crate) struct PluginDebuggerEndpointManifest {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default = "default_debugger_kind")]
    pub(crate) kind: String,
    #[serde(default = "default_http_method")]
    pub(crate) method: String,
    #[serde(alias = "url")]
    pub(crate) endpoint: String,
    #[serde(default)]
    pub(crate) bearer_config_key: String,
    #[serde(default)]
    pub(crate) request_defaults: Value,
    #[serde(default = "default_debugger_timeout")]
    pub(crate) timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub(crate) struct PluginDebuggerPanelManifest {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) endpoint: String,
    #[serde(default)]
    pub(crate) config: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PluginStateFile {
    enabled: bool,
    autostart: bool,
    config: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginSummary {
    id: String,
    name: String,
    version: String,
    description: String,
    types: Vec<String>,
    enabled: bool,
    autostart: bool,
    status: String,
    endpoint: String,
    tool_count: usize,
    service_count: usize,
    tool_provider_count: usize,
    services: Vec<PluginServiceSummary>,
    providers: Vec<PluginProviderSummary>,
    debugger: PluginDebuggerManifest,
    config: Value,
    configured_secrets: HashMap<String, bool>,
    config_schema: Value,
    directory: String,
    last_error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginServiceSummary {
    id: String,
    title: String,
    primary: bool,
    transport: String,
    status: String,
    endpoint: String,
    healthcheck: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginProviderSummary {
    id: String,
    title: String,
    protocol: String,
    service_id: String,
    endpoint: String,
    model_config_key: String,
    api_key_config_key: String,
}

pub(crate) struct PluginAgentProvider {
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) api_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginLogLine {
    timestamp: u64,
    stream: String,
    message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    service_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginDebuggerResponse {
    endpoint_id: String,
    status: u16,
    duration_ms: u64,
    content_type: String,
    body: Value,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginProjectSummary {
    id: String,
    name: String,
    version: String,
    description: String,
    types: Vec<String>,
    directory: String,
    valid: bool,
    validation_message: String,
}

pub(crate) struct PluginToolExecution {
    pub(crate) output: Value,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginToolDescriptor {
    name: String,
    title: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginToolWorkbenchResult {
    ok: bool,
    output: Value,
    duration_ms: u64,
}

struct RunningPlugin {
    manager_plugin_key: String,
    service_id: String,
    child: Child,
    started_at: Instant,
    endpoint: String,
}

#[derive(Debug, Clone)]
struct ResolvedPluginService {
    id: String,
    title: String,
    primary: bool,
    manifest: PluginServiceManifest,
}

#[derive(Debug, Clone)]
struct ResolvedPluginTool {
    provider_id: String,
    name: String,
    description: String,
    parameters: Value,
    runtime: String,
    entry: String,
    args: Vec<String>,
    callable: String,
    timeout_seconds: u64,
}

#[derive(Debug, Clone)]
struct ResolvedPluginHealth {
    kind: String,
    endpoint: String,
    startup_timeout: Duration,
    request_timeout: Duration,
    interval: Duration,
}

struct PluginManagerInner {
    processes: Mutex<HashMap<String, RunningPlugin>>,
    starting: Mutex<HashSet<String>>,
    logs: Mutex<HashMap<String, Arc<Mutex<VecDeque<PluginLogLine>>>>>,
    last_errors: Mutex<HashMap<String, String>>,
}

impl Drop for PluginManagerInner {
    fn drop(&mut self) {
        if let Ok(processes) = self.processes.get_mut() {
            for process in processes.values_mut() {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct PluginManager {
    inner: Arc<PluginManagerInner>,
}

struct PluginStartGuard<'a> {
    manager: &'a PluginManager,
    key: String,
}

impl Drop for PluginStartGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut starting) = self.manager.inner.starting.lock() {
            starting.remove(&self.key);
        }
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(PluginManagerInner {
                processes: Mutex::new(HashMap::new()),
                starting: Mutex::new(HashSet::new()),
                logs: Mutex::new(HashMap::new()),
                last_errors: Mutex::new(HashMap::new()),
            }),
        }
    }
}

fn default_object_schema() -> Value {
    json!({"type":"object","properties":{},"additionalProperties":false})
}

const fn default_true() -> bool {
    true
}

fn default_http_health_kind() -> String {
    "http".to_owned()
}

const fn default_health_startup_timeout() -> u64 {
    10
}

const fn default_health_request_timeout() -> u64 {
    600
}

const fn default_health_interval() -> u64 {
    120
}

fn default_log_format() -> String {
    "text".to_owned()
}

fn default_debugger_kind() -> String {
    "custom".to_owned()
}

fn default_http_method() -> String {
    "GET".to_owned()
}

fn default_provider_protocol() -> String {
    "openai".to_owned()
}

const fn default_debugger_timeout() -> u64 {
    15
}

const fn default_tool_timeout() -> u64 {
    30
}

#[tauri::command]
pub(crate) async fn list_plugins(
    paths: State<'_, AppPaths>,
    manager: State<'_, PluginManager>,
) -> Result<Vec<PluginSummary>, String> {
    let workspace_root = paths.workspace_root.clone();
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || list_plugins_inner(&workspace_root, &manager))
        .await
        .map_err(|error| format!("插件扫描后台任务失败：{error}"))?
}

#[tauri::command]
pub(crate) fn install_plugin(
    package_path: String,
    paths: State<'_, AppPaths>,
) -> Result<PluginSummary, String> {
    install_plugin_archive(&paths.workspace_root, Path::new(&package_path))?;
    let manager = PluginManager::default();
    let installed_id = read_archive_manifest(Path::new(&package_path))?.id;
    list_plugins_inner(&paths.workspace_root, &manager)?
        .into_iter()
        .find(|plugin| plugin.id == installed_id)
        .ok_or_else(|| "插件安装完成但无法重新读取".to_owned())
}

#[tauri::command]
pub(crate) fn save_plugin_config(
    plugin_id: String,
    mut config: Value,
    autostart: bool,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let (root, manifest) = load_plugin(&paths.workspace_root, &plugin_id)?;
    if !config.is_object() {
        return Err("插件配置根节点必须是 object".to_owned());
    }
    let schema = load_plugin_config_schema(&root, &manifest)?;
    let mut state = read_plugin_state(&root, &manifest)?;
    preserve_unchanged_plugin_secrets(&mut config, &state.config, &schema)?;
    if plugin_id == "dify2api" {
        secure_dify2api_config(&mut config)?;
    }
    validate_config_primitives(&config, &schema)?;
    state.config = config;
    state.autostart = autostart;
    write_plugin_state(&root, &state)
}

#[tauri::command]
pub(crate) fn set_plugin_enabled(
    plugin_id: String,
    enabled: bool,
    paths: State<'_, AppPaths>,
    manager: State<'_, PluginManager>,
) -> Result<(), String> {
    let (root, manifest) = load_plugin(&paths.workspace_root, &plugin_id)?;
    let mut state = read_plugin_state(&root, &manifest)?;
    if !enabled {
        stop_plugin_inner(&paths.workspace_root, &plugin_id, &manager)?;
    }
    state.enabled = enabled;
    write_plugin_state(&root, &state)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn stop_plugin(
    plugin_id: String,
    paths: State<'_, AppPaths>,
    manager: State<'_, PluginManager>,
) -> Result<(), String> {
    stop_plugin_inner(&paths.workspace_root, &plugin_id, &manager)
}

#[tauri::command]
pub(crate) fn uninstall_plugin(
    plugin_id: String,
    paths: State<'_, AppPaths>,
    manager: State<'_, PluginManager>,
) -> Result<(), String> {
    validate_identifier(&plugin_id, "插件")?;
    stop_plugin_inner(&paths.workspace_root, &plugin_id, &manager)?;
    let root = plugins_root(&paths.workspace_root).join(&plugin_id);
    if !root.is_dir() {
        return Err(format!("插件不存在：{plugin_id}"));
    }
    fs::remove_dir_all(root).map_err(|error| format!("卸载插件失败：{error}"))?;
    if plugin_id == "dify2api" {
        fs::write(
            plugins_root(&paths.workspace_root).join(".removed-dify2api"),
            b"1\n",
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn get_plugin_logs(
    plugin_id: String,
    paths: State<'_, AppPaths>,
    manager: State<'_, PluginManager>,
) -> Result<Vec<PluginLogLine>, String> {
    validate_identifier(&plugin_id, "插件")?;
    let key = plugin_manager_key(&paths.workspace_root, &plugin_id);
    let logs = manager
        .inner
        .logs
        .lock()
        .map_err(|_| "插件日志状态已损坏".to_owned())?;
    let Some(lines) = logs.get(&key) else {
        return Ok(Vec::new());
    };
    let result = lines
        .lock()
        .map_err(|_| "插件日志缓冲区已损坏".to_owned())?
        .iter()
        .cloned()
        .collect();
    Ok(result)
}

#[tauri::command]
pub(crate) async fn test_plugin_connection(
    plugin_id: String,
    paths: State<'_, AppPaths>,
) -> Result<Value, String> {
    let workspace_root = paths.workspace_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let (root, manifest) = load_plugin(&workspace_root, &plugin_id)?;
            let state = read_plugin_state(&root, &manifest)?;
            let services = resolved_plugin_services(&manifest)?;
            let service = services
                .iter()
                .filter(|service| !service.manifest.test_endpoint.trim().is_empty())
                .min_by_key(|service| !service.primary)
                .ok_or_else(|| "该插件没有声明连接测试入口".to_owned())?;
            let endpoint = render_manifest_template(
                &service.manifest.test_endpoint,
                &state.config,
                &root,
                &root.join("state.json"),
            )?;
            validate_loopback_http_endpoint(&endpoint)?;
            let config = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(35)))
                .proxy(None)
                .max_redirects(0)
                .build();
            let http = ureq::Agent::new_with_config(config);
            let mut response = http
                .get(&endpoint)
                .call()
                .map_err(|error| format!("插件连接测试失败：{error}"))?;
            let body = response
                .body_mut()
                .read_json::<Value>()
                .map_err(|error| format!("插件连接测试返回无效 JSON：{error}"))?;
            Ok(redact_plugin_value(
                body,
                &plugin_secret_values(&root, &manifest, &state.config),
            ))
        })();
        result.map_err(|error| redact_plugin_error(&workspace_root, &plugin_id, error))
    })
    .await
    .map_err(|error| format!("插件连接测试后台任务失败：{error}"))?
}

#[tauri::command]
pub(crate) async fn run_plugin_debugger(
    plugin_id: String,
    endpoint_id: String,
    request: Option<Value>,
    paths: State<'_, AppPaths>,
) -> Result<PluginDebuggerResponse, String> {
    let workspace_root = paths.workspace_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_plugin_debugger_inner(&workspace_root, &plugin_id, &endpoint_id, request)
            .map_err(|error| redact_plugin_error(&workspace_root, &plugin_id, error))
    })
    .await
    .map_err(|error| format!("插件调试后台任务失败：{error}"))?
}

#[tauri::command]
pub(crate) async fn list_plugin_projects(
    paths: State<'_, AppPaths>,
) -> Result<Vec<PluginProjectSummary>, String> {
    let workspace_root = paths.workspace_root.clone();
    tauri::async_runtime::spawn_blocking(move || list_plugin_projects_inner(&workspace_root))
        .await
        .map_err(|error| format!("插件项目扫描后台任务失败：{error}"))?
}

#[tauri::command]
pub(crate) fn create_plugin_project(
    plugin_id: String,
    name: String,
    project_type: String,
    paths: State<'_, AppPaths>,
) -> Result<PluginProjectSummary, String> {
    validate_identifier(&plugin_id, "插件")?;
    let display_name = name.trim();
    if display_name.is_empty() || display_name.len() > 80 {
        return Err("插件名称需要 1-80 个字符".to_owned());
    }
    let root = plugin_projects_root(&paths.workspace_root).join(&plugin_id);
    if root.exists() {
        return Err(format!("插件项目已存在：{plugin_id}"));
    }
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let result = (|| {
        let manifest = match project_type.as_str() {
            "tool" => {
                fs::create_dir_all(root.join("tools")).map_err(|error| error.to_string())?;
                fs::write(root.join("tools/example.py"), TOOL_PYTHON_TEMPLATE)
                    .map_err(|error| error.to_string())?;
                TOOL_PLUGIN_TEMPLATE
            }
            "service" => {
                fs::create_dir_all(root.join("service")).map_err(|error| error.to_string())?;
                fs::write(root.join("service/main.py"), SERVICE_PYTHON_TEMPLATE)
                    .map_err(|error| error.to_string())?;
                fs::write(
                    root.join("config.schema.json"),
                    serde_json::to_vec_pretty(&json!({
                        "type": "object",
                        "properties": {
                            "port": {"type": "integer", "title": "监听端口", "minimum": 1024, "maximum": 65535},
                            "model": {"type": "string", "title": "模型名称"}
                        },
                        "required": ["port", "model"]
                    }))
                    .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                SERVICE_PLUGIN_TEMPLATE
            }
            "bundle" => {
                fs::create_dir_all(root.join("service")).map_err(|error| error.to_string())?;
                fs::create_dir_all(root.join("tools")).map_err(|error| error.to_string())?;
                fs::write(root.join("service/main.py"), SERVICE_PYTHON_TEMPLATE)
                    .map_err(|error| error.to_string())?;
                fs::write(root.join("tools/example.py"), TOOL_PYTHON_TEMPLATE)
                    .map_err(|error| error.to_string())?;
                fs::write(
                    root.join("config.schema.json"),
                    serde_json::to_vec_pretty(&json!({
                        "type": "object",
                        "properties": {
                            "port": {"type": "integer", "title": "监听端口", "minimum": 1024, "maximum": 65535},
                            "model": {"type": "string", "title": "模型名称"}
                        },
                        "required": ["port", "model"],
                        "additionalProperties": false
                    }))
                    .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                BUNDLE_PLUGIN_TEMPLATE
            }
            _ => return Err("插件项目类型只支持 tool、service 或 bundle".to_owned()),
        };
        fs::write(
            root.join("plugin.yaml"),
            manifest
                .replace("{id}", &plugin_id)
                .replace("{name}", display_name),
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            root.join("README.md"),
            format!("# {display_name}\n\n使用 DRPA 插件开发工具验证、构建并安装。\n"),
        )
        .map_err(|error| error.to_string())?;
        validate_plugin_project_root(&root, &plugin_id)
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&root);
        return Err(error);
    }
    list_plugin_projects_inner(&paths.workspace_root)?
        .into_iter()
        .find(|project| project.id == plugin_id)
        .ok_or_else(|| "插件项目创建后读取失败".to_owned())
}

#[tauri::command]
pub(crate) fn validate_plugin_project(
    plugin_id: String,
    paths: State<'_, AppPaths>,
) -> Result<PluginProjectSummary, String> {
    validate_identifier(&plugin_id, "插件")?;
    let root = plugin_projects_root(&paths.workspace_root).join(&plugin_id);
    validate_plugin_project_root(&root, &plugin_id)?;
    list_plugin_projects_inner(&paths.workspace_root)?
        .into_iter()
        .find(|project| project.id == plugin_id)
        .ok_or_else(|| "插件项目不存在".to_owned())
}

#[tauri::command]
pub(crate) fn build_plugin_project(
    plugin_id: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    build_plugin_project_inner(&paths.workspace_root, &plugin_id)
}

fn build_plugin_project_inner(workspace_root: &Path, plugin_id: &str) -> Result<String, String> {
    validate_identifier(plugin_id, "插件")?;
    let root = plugin_projects_root(workspace_root).join(plugin_id);
    let manifest = validate_plugin_project_root(&root, plugin_id)?;
    let output_root = workspace_root.join("build").join("plugins");
    fs::create_dir_all(&output_root).map_err(|error| error.to_string())?;
    let output = output_root.join(format!("{}-{}.drpa-plugin", manifest.id, manifest.version));
    let file = File::create(&output).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let executable_paths = plugin_executable_paths(&manifest);
    let mut files = Vec::new();
    collect_plugin_project_files(&root, &root, &mut files)?;
    files.sort();
    for relative in files {
        let file_options = if executable_paths.contains(relative.replace('\\', "/").as_str()) {
            options.unix_permissions(0o755)
        } else {
            options.unix_permissions(0o644)
        };
        archive
            .start_file(relative.replace('\\', "/"), file_options)
            .map_err(|error| error.to_string())?;
        let mut source = File::open(root.join(&relative)).map_err(|error| error.to_string())?;
        std::io::copy(&mut source, &mut archive).map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    Ok(output.to_string_lossy().into_owned())
}

pub(crate) fn start_plugin_inner(
    workspace_root: &Path,
    plugin_id: &str,
    python: Option<&Path>,
    manager: &PluginManager,
) -> Result<(), String> {
    let result = start_plugin_process(workspace_root, plugin_id, python, manager);
    if let Err(error) = &result
        && let Ok(mut errors) = manager.inner.last_errors.lock()
    {
        errors.insert(plugin_manager_key(workspace_root, plugin_id), error.clone());
    }
    result
}

fn start_plugin_process(
    workspace_root: &Path,
    plugin_id: &str,
    python: Option<&Path>,
    manager: &PluginManager,
) -> Result<(), String> {
    let manager_plugin_key = plugin_manager_key(workspace_root, plugin_id);
    let _start_guard = {
        let mut starting = manager
            .inner
            .starting
            .lock()
            .map_err(|_| "插件启动锁已损坏".to_owned())?;
        if !starting.insert(manager_plugin_key.clone()) {
            return Err(format!("插件 {plugin_id} 正在启动"));
        }
        PluginStartGuard {
            manager,
            key: manager_plugin_key.clone(),
        }
    };
    let (root, manifest) = load_plugin(workspace_root, plugin_id)?;
    let services = resolved_plugin_services(&manifest)?;
    if services.is_empty() {
        return Err("该插件没有后台服务".to_owned());
    }
    let mut state = read_plugin_state(&root, &manifest)?;
    if plugin_id == "dify2api" {
        secure_dify2api_config(&mut state.config)?;
    }
    state.enabled = true;
    write_plugin_state(&root, &state)?;
    refresh_processes(manager)?;
    let log_buffer = {
        let mut logs = manager
            .inner
            .logs
            .lock()
            .map_err(|_| "插件日志状态已损坏".to_owned())?;
        Arc::clone(
            logs.entry(manager_plugin_key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(VecDeque::new()))),
        )
    };
    manager
        .inner
        .last_errors
        .lock()
        .map_err(|_| "插件错误状态已损坏".to_owned())?
        .remove(&manager_plugin_key);
    let redactions = Arc::new(plugin_secret_values(&root, &manifest, &state.config));

    let mut newly_started: Vec<String> = Vec::new();
    for service in &services {
        if service_is_running(workspace_root, plugin_id, &service.id, manager)? {
            continue;
        }
        if let Err(error) = start_plugin_service(
            workspace_root,
            plugin_id,
            &root,
            &state,
            service,
            python,
            Arc::clone(&log_buffer),
            Arc::clone(&redactions),
            manager,
        ) {
            let _ = stop_plugin_service_inner(workspace_root, plugin_id, &service.id, manager);
            for service_id in newly_started {
                let _ = stop_plugin_service_inner(workspace_root, plugin_id, &service_id, manager);
            }
            return Err(format!(
                "插件 {plugin_id} 的服务 {} 启动失败：{error}",
                service.id
            ));
        }
        newly_started.push(service.id.clone());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn start_plugin_service(
    workspace_root: &Path,
    plugin_id: &str,
    root: &Path,
    state: &PluginStateFile,
    service: &ResolvedPluginService,
    python: Option<&Path>,
    log_buffer: Arc<Mutex<VecDeque<PluginLogLine>>>,
    redactions: Arc<Vec<String>>,
    manager: &PluginManager,
) -> Result<(), String> {
    let entry_relative = resolve_service_entry(&service.manifest)?;
    let entry = resolve_plugin_path(root, entry_relative, true)?;
    let config_path = root.join("state.json");
    let mut command = match service.manifest.runtime.as_str() {
        "bundled-python" => {
            let Some(python) = python.filter(|path| path.is_file()) else {
                return Err("插件需要已初始化的封装 Python 运行环境".to_owned());
            };
            let mut command = Command::new(python);
            command.arg("-I").arg(&entry);
            command
        }
        "executable" => {
            let execution_entry = prepare_service_execution_entry(plugin_id, root, &entry)?;
            Command::new(execution_entry)
        }
        runtime => return Err(format!("插件服务 runtime 无效：{runtime}")),
    };
    let rendered_args = service
        .manifest
        .args
        .iter()
        .map(|argument| render_manifest_template(argument, &state.config, root, &config_path))
        .collect::<Result<Vec<_>, _>>()?;
    let endpoint = render_manifest_template(
        &service.manifest.endpoint,
        &state.config,
        root,
        &config_path,
    )?;
    let capture_stdout = service.manifest.logs.stdout;
    let capture_stderr = service.manifest.logs.stderr;
    command
        .args(&rendered_args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(if capture_stdout {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(if capture_stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .env("DRPA_PLUGIN_ID", plugin_id)
        .env("DRPA_PLUGIN_SERVICE_ID", &service.id)
        .env("DRPA_PLUGIN_ROOT", root)
        .env("DRPA_PLUGIN_CONFIG", &config_path)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动进程失败：{error}"))?;
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(
            stdout,
            "stdout",
            &service.id,
            &service.manifest.logs.format,
            Arc::clone(&log_buffer),
            Arc::clone(&redactions),
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(
            stderr,
            "stderr",
            &service.id,
            &service.manifest.logs.format,
            Arc::clone(&log_buffer),
            Arc::clone(&redactions),
        );
    }
    let manager_plugin_key = plugin_manager_key(workspace_root, plugin_id);
    let process_key = plugin_service_process_key(workspace_root, plugin_id, &service.id);
    manager
        .inner
        .processes
        .lock()
        .map_err(|_| "插件进程状态已损坏".to_owned())?
        .insert(
            process_key,
            RunningPlugin {
                manager_plugin_key,
                service_id: service.id.clone(),
                child,
                started_at: Instant::now(),
                endpoint,
            },
        );
    if let Some(health) = resolve_plugin_health(service, &state.config, root, &config_path)? {
        wait_for_plugin_health(workspace_root, plugin_id, &service.id, &health, manager)?;
    }
    Ok(())
}

pub(crate) fn start_autostart_plugins(
    workspace_root: &Path,
    python: Option<&Path>,
    manager: &PluginManager,
) -> Result<(), String> {
    ensure_plugins_root(workspace_root)?;
    for entry in fs::read_dir(plugins_root(workspace_root))
        .map_err(|error| error.to_string())?
        .flatten()
    {
        let id = entry.file_name().to_string_lossy().into_owned();
        let Ok((root, manifest)) = load_plugin(workspace_root, &id) else {
            continue;
        };
        let state = read_plugin_state(&root, &manifest)?;
        if state.enabled
            && state.autostart
            && resolved_plugin_services(&manifest).is_ok_and(|services| !services.is_empty())
        {
            let _ = start_plugin_inner(workspace_root, &id, python, manager);
        }
    }
    Ok(())
}

pub(crate) fn plugin_services_require_bundled_python(
    workspace_root: &Path,
    plugin_id: &str,
) -> Result<bool, String> {
    let (_, manifest) = load_plugin(workspace_root, plugin_id)?;
    Ok(resolved_plugin_services(&manifest)?
        .iter()
        .any(|service| service.manifest.runtime == "bundled-python"))
}

pub(crate) fn plugin_tool_definitions(workspace_root: &Path) -> Result<Vec<Value>, String> {
    ensure_plugins_root(workspace_root)?;
    let mut definitions = Vec::new();
    for entry in fs::read_dir(plugins_root(workspace_root))
        .map_err(|error| error.to_string())?
        .flatten()
    {
        let id = entry.file_name().to_string_lossy().into_owned();
        let Ok((root, manifest)) = load_plugin(workspace_root, &id) else {
            continue;
        };
        let state = read_plugin_state(&root, &manifest)?;
        if !state.enabled {
            continue;
        }
        for tool in resolved_plugin_tools(&manifest)? {
            definitions.push(json!({
                "type": "function",
                "function": {
                    "name": qualified_plugin_tool_name(&id, &tool.name),
                    "description": format!("插件 {}：{}", manifest.name, tool.description),
                    "parameters": tool.parameters,
                }
            }));
        }
    }
    Ok(definitions)
}

pub(crate) fn list_plugin_tools_for_workbench(
    workspace_root: &Path,
    plugin_id: &str,
) -> Result<Vec<PluginToolDescriptor>, String> {
    validate_identifier(plugin_id, "插件")?;
    let (_, manifest) = load_plugin(workspace_root, plugin_id)?;
    Ok(resolved_plugin_tools(&manifest)?
        .into_iter()
        .map(|tool| PluginToolDescriptor {
            title: tool.name.clone(),
            name: tool.name,
            description: tool.description,
            input_schema: tool.parameters,
        })
        .collect())
}

pub(crate) fn invoke_plugin_tool_for_workbench(
    workspace_root: &Path,
    python: &Path,
    plugin_id: &str,
    tool_name: &str,
    input: &Value,
) -> Result<PluginToolWorkbenchResult, String> {
    validate_identifier(plugin_id, "插件")?;
    validate_component_identifier(tool_name, "插件工具")?;
    if !input.is_object() {
        return Err("插件工具输入必须是 JSON object".to_owned());
    }
    let started = Instant::now();
    let result =
        execute_plugin_tool_inner(workspace_root, None, python, plugin_id, tool_name, input)?;
    Ok(PluginToolWorkbenchResult {
        ok: true,
        output: result.output,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

pub(crate) fn execute_plugin_tool(
    workspace_root: &Path,
    project_root: Option<&Path>,
    python: &Path,
    qualified_name: &str,
    arguments: &Value,
) -> Option<Result<PluginToolExecution, String>> {
    let remainder = qualified_name.strip_prefix("plugin_")?;
    let (plugin_id, tool_name) = remainder.split_once("__")?;
    Some(execute_plugin_tool_inner(
        workspace_root,
        project_root,
        python,
        plugin_id,
        tool_name,
        arguments,
    ))
}

fn execute_plugin_tool_inner(
    workspace_root: &Path,
    project_root: Option<&Path>,
    python: &Path,
    plugin_id: &str,
    tool_name: &str,
    arguments: &Value,
) -> Result<PluginToolExecution, String> {
    if serde_json::to_vec(arguments)
        .map_err(|error| format!("插件工具参数无效：{error}"))?
        .len()
        > MAX_CONFIG_BYTES
    {
        return Err("插件工具参数超过 256 KiB".to_owned());
    }
    let (root, manifest) = load_plugin(workspace_root, plugin_id)?;
    let state = read_plugin_state(&root, &manifest)?;
    let redactions = plugin_secret_values(&root, &manifest, &state.config);
    if !state.enabled {
        return Err(format!("插件未启用：{plugin_id}"));
    }
    let tool = resolved_plugin_tools(&manifest)?
        .into_iter()
        .find(|tool| tool.name == tool_name)
        .ok_or_else(|| {
            format!(
                "插件工具不存在：{qualified_name}",
                qualified_name = qualified_plugin_tool_name(plugin_id, tool_name)
            )
        })?;
    let entry_spec = tool.entry.split_once(':');
    let entry = resolve_plugin_path(
        &root,
        entry_spec.map_or(tool.entry.as_str(), |(path, _)| path),
        true,
    )?;
    let request = json!({
        "arguments": arguments,
        "provider": &tool.provider_id,
        "tool": &tool.name,
        "context": {
            "pluginRoot": root.to_string_lossy(),
            "workspaceRoot": workspace_root.to_string_lossy(),
            "projectRoot": project_root.map(|path| path.to_string_lossy().into_owned()),
            "config": state.config,
        }
    });
    let timeout = Duration::from_secs(tool.timeout_seconds.clamp(1, 300));
    let output = match tool.runtime.as_str() {
        "bundled-python" => {
            if !python.is_file() {
                return Err("插件工具需要已初始化的封装 Python 运行环境".to_owned());
            }
            let callable = if !tool.callable.trim().is_empty() {
                tool.callable.as_str()
            } else {
                entry_spec.map_or("run", |(_, callable)| callable)
            };
            run_json_process(
                Command::new(python),
                &[
                    "-I".to_owned(),
                    "-c".to_owned(),
                    PLUGIN_PYTHON_RUNNER.to_owned(),
                    entry.to_string_lossy().into_owned(),
                    callable.to_owned(),
                ],
                &root,
                &request,
                timeout,
            )
        }
        "executable" => {
            ensure_executable(&entry)?;
            run_json_process(Command::new(&entry), &tool.args, &root, &request, timeout)
        }
        runtime => return Err(format!("插件工具 runtime 无效：{runtime}")),
    }
    .map(|output| redact_plugin_value(output, &redactions))
    .map_err(|error| redact_plugin_text(error, &redactions))?;
    Ok(PluginToolExecution {
        output,
        summary: format!("插件 {plugin_id} 已执行工具 {tool_name}"),
    })
}

fn list_plugins_inner(
    workspace_root: &Path,
    manager: &PluginManager,
) -> Result<Vec<PluginSummary>, String> {
    ensure_plugins_root(workspace_root)?;
    refresh_processes(manager)?;
    let mut summaries = Vec::new();
    for entry in fs::read_dir(plugins_root(workspace_root))
        .map_err(|error| error.to_string())?
        .flatten()
    {
        if !entry.path().is_dir() || !entry.path().join("plugin.yaml").is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        summaries.push(
            summarize_plugin(workspace_root, &id, manager)
                .unwrap_or_else(|error| broken_plugin_summary(&id, &entry.path(), error)),
        );
    }
    summaries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(summaries)
}

fn summarize_plugin(
    workspace_root: &Path,
    id: &str,
    manager: &PluginManager,
) -> Result<PluginSummary, String> {
    let (root, manifest) = load_plugin(workspace_root, id)?;
    let state = read_plugin_state(&root, &manifest)?;
    let redactions = plugin_secret_values(&root, &manifest, &state.config);
    let services = resolved_plugin_services(&manifest)?;
    let tools = resolved_plugin_tools(&manifest)?;
    let manager_plugin_key = plugin_manager_key(workspace_root, id);
    let running = manager
        .inner
        .processes
        .lock()
        .map_err(|_| "插件进程状态已损坏".to_owned())?;
    let config_path = root.join("state.json");
    let mut service_summaries = Vec::with_capacity(services.len());
    let mut running_count = 0_usize;
    for service in &services {
        let process = running.get(&plugin_service_process_key(workspace_root, id, &service.id));
        if process.is_some() {
            running_count += 1;
        }
        let declared_endpoint = render_manifest_template(
            &service.manifest.endpoint,
            &state.config,
            &root,
            &config_path,
        )?;
        let healthcheck = resolve_plugin_health(service, &state.config, &root, &config_path)?
            .map(|health| health.endpoint)
            .unwrap_or_default();
        service_summaries.push(PluginServiceSummary {
            id: service.id.clone(),
            title: service.title.clone(),
            primary: service.primary,
            transport: service.manifest.transport.clone(),
            status: if process.is_some() {
                "running".to_owned()
            } else if state.enabled {
                "stopped".to_owned()
            } else {
                "disabled".to_owned()
            },
            endpoint: process
                .map(|running| running.endpoint.clone())
                .unwrap_or(declared_endpoint),
            healthcheck,
        });
    }
    let status = if !state.enabled {
        "disabled"
    } else if services.is_empty() || running_count == 0 {
        "stopped"
    } else if running_count == services.len() {
        "running"
    } else {
        "error"
    }
    .to_owned();
    for service in &mut service_summaries {
        service.endpoint = redact_plugin_text(service.endpoint.clone(), &redactions);
        service.healthcheck = redact_plugin_text(service.healthcheck.clone(), &redactions);
    }
    let endpoint = service_summaries
        .iter()
        .find(|service| service.primary)
        .or_else(|| service_summaries.first())
        .map(|service| service.endpoint.clone())
        .unwrap_or_default();
    drop(running);
    let mut last_error = manager
        .inner
        .last_errors
        .lock()
        .map_err(|_| "插件错误状态已损坏".to_owned())?
        .get(&manager_plugin_key)
        .cloned()
        .unwrap_or_default();
    if status == "error" && last_error.is_empty() {
        last_error = format!("插件仅有 {running_count}/{} 个服务仍在运行", services.len());
    }
    last_error = redact_plugin_text(last_error, &redactions);
    let config_schema = load_plugin_config_schema(&root, &manifest)
        .map_err(|error| format!("插件 {id} 的配置 schema 无效：{error}"))?;
    let mut provider_summaries =
        plugin_provider_summaries(&manifest, &services, &state.config, &root, &config_path)?;
    for provider in &mut provider_summaries {
        provider.endpoint = redact_plugin_text(provider.endpoint.clone(), &redactions);
    }
    let configured_secrets = configured_plugin_secrets(&config_schema, &state.config);
    let safe_config = redact_plugin_config(&config_schema, &state.config);
    Ok(PluginSummary {
        id: id.to_owned(),
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        types: manifest.types,
        enabled: state.enabled,
        autostart: state.autostart,
        status: if !last_error.is_empty() && status != "running" && status != "disabled" {
            "error".to_owned()
        } else {
            status
        },
        endpoint,
        tool_count: tools.len(),
        service_count: services.len(),
        tool_provider_count: manifest.tool_providers.len(),
        services: service_summaries,
        providers: provider_summaries,
        debugger: manifest.debugger,
        config: safe_config,
        configured_secrets,
        config_schema,
        directory: root.to_string_lossy().into_owned(),
        last_error,
    })
}

fn broken_plugin_summary(id: &str, root: &Path, error: String) -> PluginSummary {
    PluginSummary {
        id: id.to_owned(),
        name: id.to_owned(),
        version: "-".to_owned(),
        description: "插件清单或状态需要修复".to_owned(),
        types: Vec::new(),
        enabled: false,
        autostart: false,
        status: "error".to_owned(),
        endpoint: String::new(),
        tool_count: 0,
        service_count: 0,
        tool_provider_count: 0,
        services: Vec::new(),
        providers: Vec::new(),
        debugger: PluginDebuggerManifest::default(),
        config: json!({}),
        configured_secrets: HashMap::new(),
        config_schema: json!({"type":"object","properties":{}}),
        directory: root.to_string_lossy().into_owned(),
        last_error: error,
    }
}

fn refresh_processes(manager: &PluginManager) -> Result<(), String> {
    let mut exited = Vec::new();
    {
        let mut processes = manager
            .inner
            .processes
            .lock()
            .map_err(|_| "插件进程状态已损坏".to_owned())?;
        for (key, process) in processes.iter_mut() {
            if let Some(status) = process
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
            {
                exited.push((
                    key.clone(),
                    process.manager_plugin_key.clone(),
                    format!(
                        "服务 {} 已退出：{}，运行 {} ms",
                        process.service_id,
                        status.code().unwrap_or(-1),
                        process.started_at.elapsed().as_millis()
                    ),
                ));
            }
        }
        for (key, _, _) in &exited {
            processes.remove(key);
        }
    }
    let mut errors = manager
        .inner
        .last_errors
        .lock()
        .map_err(|_| "插件错误状态已损坏".to_owned())?;
    for (_, manager_plugin_key, error) in exited {
        errors
            .entry(manager_plugin_key)
            .and_modify(|current| {
                if !current.is_empty() {
                    current.push('；');
                }
                current.push_str(&error);
            })
            .or_insert(error);
    }
    Ok(())
}

fn wait_for_plugin_health(
    workspace_root: &Path,
    plugin_id: &str,
    service_id: &str,
    health: &ResolvedPluginHealth,
    manager: &PluginManager,
) -> Result<(), String> {
    let deadline = Instant::now() + health.startup_timeout;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(health.request_timeout))
        .proxy(None)
        .max_redirects(0)
        .build();
    let http = ureq::Agent::new_with_config(config);
    let process_key = plugin_service_process_key(workspace_root, plugin_id, service_id);
    loop {
        {
            let mut processes = manager
                .inner
                .processes
                .lock()
                .map_err(|_| "插件进程状态已损坏".to_owned())?;
            let Some(process) = processes.get_mut(&process_key) else {
                return Err(format!(
                    "插件 {plugin_id} 的服务 {service_id} 在健康检查前已停止"
                ));
            };
            if let Some(status) = process
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
            {
                processes.remove(&process_key);
                return Err(format!(
                    "插件 {plugin_id} 的服务 {service_id} 启动失败，进程退出码 {}",
                    status.code().unwrap_or(-1)
                ));
            }
        }
        if health.kind == "process" || http.get(&health.endpoint).call().is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "插件 {plugin_id} 的服务 {service_id} 启动超时，健康检查未就绪：{}",
                health.endpoint
            ));
        }
        thread::sleep(health.interval);
    }
}

fn stop_plugin_inner(
    workspace_root: &Path,
    plugin_id: &str,
    manager: &PluginManager,
) -> Result<(), String> {
    validate_identifier(plugin_id, "插件")?;
    let manager_plugin_key = plugin_manager_key(workspace_root, plugin_id);
    let _lifecycle_guard = {
        let mut starting = manager
            .inner
            .starting
            .lock()
            .map_err(|_| "插件生命周期锁已损坏".to_owned())?;
        if !starting.insert(manager_plugin_key.clone()) {
            return Err(format!("插件 {plugin_id} 正在执行启动或停止操作"));
        }
        PluginStartGuard {
            manager,
            key: manager_plugin_key.clone(),
        }
    };
    let mut processes = manager
        .inner
        .processes
        .lock()
        .map_err(|_| "插件进程状态已损坏".to_owned())?;
    let keys = processes
        .iter()
        .filter(|(_, process)| process.manager_plugin_key == manager_plugin_key)
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in keys {
        if let Some(mut process) = processes.remove(&key) {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }
    Ok(())
}

fn stop_plugin_service_inner(
    workspace_root: &Path,
    plugin_id: &str,
    service_id: &str,
    manager: &PluginManager,
) -> Result<(), String> {
    let key = plugin_service_process_key(workspace_root, plugin_id, service_id);
    let mut processes = manager
        .inner
        .processes
        .lock()
        .map_err(|_| "插件进程状态已损坏".to_owned())?;
    if let Some(mut process) = processes.remove(&key) {
        let _ = process.child.kill();
        let _ = process.child.wait();
    }
    Ok(())
}

fn service_is_running(
    workspace_root: &Path,
    plugin_id: &str,
    service_id: &str,
    manager: &PluginManager,
) -> Result<bool, String> {
    Ok(manager
        .inner
        .processes
        .lock()
        .map_err(|_| "插件进程状态已损坏".to_owned())?
        .contains_key(&plugin_service_process_key(
            workspace_root,
            plugin_id,
            service_id,
        )))
}

fn plugin_service_process_key(workspace_root: &Path, plugin_id: &str, service_id: &str) -> String {
    format!(
        "{}\u{1f}{service_id}",
        plugin_manager_key(workspace_root, plugin_id)
    )
}

fn plugin_manager_key(workspace_root: &Path, plugin_id: &str) -> String {
    let canonical =
        fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    let mut key = canonical.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.make_ascii_lowercase();
    }
    format!("{key}\u{1f}{plugin_id}")
}

fn resolved_plugin_services(
    manifest: &PluginManifest,
) -> Result<Vec<ResolvedPluginService>, String> {
    let mut services = Vec::new();
    if let Some(legacy) = &manifest.service {
        let id = if legacy.id.trim().is_empty() {
            "default".to_owned()
        } else {
            legacy.id.clone()
        };
        services.push(ResolvedPluginService {
            title: if legacy.title.trim().is_empty() {
                id.clone()
            } else {
                legacy.title.clone()
            },
            id,
            primary: legacy.primary,
            manifest: legacy.clone(),
        });
    }
    for service in &manifest.services {
        if service.id.trim().is_empty() {
            return Err("plugins.services[] 必须声明 id".to_owned());
        }
        services.push(ResolvedPluginService {
            id: service.id.clone(),
            title: if service.title.trim().is_empty() {
                service.id.clone()
            } else {
                service.title.clone()
            },
            primary: service.primary,
            manifest: service.clone(),
        });
    }
    let mut ids = HashSet::new();
    for service in &services {
        if !ids.insert(service.id.as_str()) {
            return Err(format!("插件服务 id 重复：{}", service.id));
        }
    }
    if services.iter().filter(|service| service.primary).count() > 1 {
        return Err("插件只能声明一个 primary service".to_owned());
    }
    if !services.is_empty() && !services.iter().any(|service| service.primary) {
        services[0].primary = true;
    }
    Ok(services)
}

fn resolved_plugin_tools(manifest: &PluginManifest) -> Result<Vec<ResolvedPluginTool>, String> {
    let mut tools = manifest
        .tools
        .iter()
        .map(|tool| ResolvedPluginTool {
            provider_id: "legacy".to_owned(),
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
            runtime: tool.runtime.clone(),
            entry: tool.entry.clone(),
            args: tool.args.clone(),
            callable: String::new(),
            timeout_seconds: tool.timeout_seconds,
        })
        .collect::<Vec<_>>();
    for provider in &manifest.tool_providers {
        for tool in &provider.tools {
            let callable = if !tool.callable.trim().is_empty() {
                tool.callable.clone()
            } else if provider.tools.len() == 1 {
                "run".to_owned()
            } else {
                tool.name.clone()
            };
            tools.push(ResolvedPluginTool {
                provider_id: provider.id.clone(),
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
                runtime: provider.runtime.clone(),
                entry: provider.entry.clone(),
                args: provider.args.clone(),
                callable,
                timeout_seconds: tool.timeout_seconds.unwrap_or(provider.timeout_seconds),
            });
        }
    }
    let mut names = HashSet::new();
    for tool in &tools {
        if !names.insert(tool.name.as_str()) {
            return Err(format!("插件工具名称重复：{}", tool.name));
        }
    }
    Ok(tools)
}

fn resolved_plugin_providers(
    manifest: &PluginManifest,
) -> Result<Vec<PluginProviderManifest>, String> {
    if !manifest.providers.is_empty() {
        return Ok(manifest.providers.clone());
    }
    if !manifest.types.iter().any(|kind| kind == "provider-adapter") {
        return Ok(Vec::new());
    }
    let Some(service) = resolved_plugin_services(manifest)?
        .into_iter()
        .find(|service| service.primary)
    else {
        return Ok(Vec::new());
    };
    Ok(vec![PluginProviderManifest {
        id: "default".to_owned(),
        title: manifest.name.clone(),
        protocol: "openai".to_owned(),
        service_id: service.id,
        endpoint: String::new(),
        model_config_key: "model".to_owned(),
        api_key_config_key: if manifest.default_config.get("proxy_api_key").is_some() {
            "proxy_api_key".to_owned()
        } else {
            String::new()
        },
    }])
}

fn plugin_provider_summaries(
    manifest: &PluginManifest,
    services: &[ResolvedPluginService],
    config: &Value,
    root: &Path,
    state_path: &Path,
) -> Result<Vec<PluginProviderSummary>, String> {
    resolved_plugin_providers(manifest)?
        .into_iter()
        .map(|provider| {
            let service = services
                .iter()
                .find(|service| service.id == provider.service_id)
                .ok_or_else(|| {
                    format!(
                        "Provider {} 引用了不存在的 service：{}",
                        provider.id, provider.service_id
                    )
                })?;
            let endpoint_template = if provider.endpoint.trim().is_empty() {
                &service.manifest.endpoint
            } else {
                &provider.endpoint
            };
            Ok(PluginProviderSummary {
                id: provider.id,
                title: provider.title,
                protocol: provider.protocol,
                service_id: provider.service_id,
                endpoint: render_manifest_template(endpoint_template, config, root, state_path)?,
                model_config_key: provider.model_config_key,
                api_key_config_key: provider.api_key_config_key,
            })
        })
        .collect()
}

pub(crate) fn resolve_agent_provider(
    workspace_root: &Path,
    plugin_id: &str,
    provider_id: &str,
) -> Result<PluginAgentProvider, String> {
    validate_identifier(plugin_id, "插件")?;
    validate_component_identifier(provider_id, "Provider")?;
    let (root, manifest) = load_plugin(workspace_root, plugin_id)?;
    let state = read_plugin_state(&root, &manifest)?;
    if !state.enabled {
        return Err(format!("插件未启用：{plugin_id}"));
    }
    let services = resolved_plugin_services(&manifest)?;
    let provider = resolved_plugin_providers(&manifest)?
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| format!("插件未声明 Provider：{provider_id}"))?;
    let protocol = provider.protocol.trim().to_ascii_lowercase();
    if !matches!(
        protocol.as_str(),
        "openai" | "openai-compatible" | "openai_compatible"
    ) {
        return Err(format!(
            "AI Agent 暂不支持插件 Provider 协议：{}",
            provider.protocol
        ));
    }
    let service = services
        .iter()
        .find(|service| service.id == provider.service_id)
        .ok_or_else(|| {
            format!(
                "Provider {} 引用了不存在的 service：{}",
                provider.id, provider.service_id
            )
        })?;
    let endpoint_template = if provider.endpoint.trim().is_empty() {
        &service.manifest.endpoint
    } else {
        &provider.endpoint
    };
    let state_path = root.join("state.json");
    let base_url = render_manifest_template(endpoint_template, &state.config, &root, &state_path)?;
    if base_url.trim().is_empty() {
        return Err(format!("Provider {} 没有可用 Endpoint", provider.id));
    }
    if plugin_secret_values(&root, &manifest, &state.config)
        .iter()
        .any(|secret| base_url.contains(secret))
    {
        return Err(format!(
            "Provider {} 的 Endpoint 不能包含密钥，请使用 api_key_config_key",
            provider.id
        ));
    }
    let model = state
        .config
        .get(&provider.model_config_key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&manifest.name)
        .to_owned();
    let api_key = if provider.api_key_config_key.trim().is_empty() {
        String::new()
    } else {
        state
            .config
            .get(&provider.api_key_config_key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                format!(
                    "Provider {} 的凭据尚未配置：{}",
                    provider.id, provider.api_key_config_key
                )
            })?
            .to_owned()
    };
    Ok(PluginAgentProvider {
        base_url,
        model,
        api_key,
    })
}

fn resolve_service_entry(service: &PluginServiceManifest) -> Result<&str, String> {
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        std::env::consts::OS
    };
    service
        .entry_by_platform
        .get(platform)
        .map(String::as_str)
        .or_else(|| (!service.entry.trim().is_empty()).then_some(service.entry.as_str()))
        .ok_or_else(|| format!("插件服务没有适用于 {platform} 的 entry"))
}

fn resolve_plugin_health(
    service: &ResolvedPluginService,
    config: &Value,
    root: &Path,
    state_path: &Path,
) -> Result<Option<ResolvedPluginHealth>, String> {
    let declared = if let Some(health) = &service.manifest.health {
        Some(health.clone())
    } else if !service.manifest.healthcheck.trim().is_empty() {
        Some(PluginHealthManifest {
            kind: "http".to_owned(),
            endpoint: service.manifest.healthcheck.clone(),
            startup_timeout_seconds: default_health_startup_timeout(),
            request_timeout_millis: default_health_request_timeout(),
            interval_millis: default_health_interval(),
        })
    } else {
        None
    };
    let Some(health) = declared else {
        return Ok(None);
    };
    let kind = health.kind.trim().to_ascii_lowercase();
    let endpoint = render_manifest_template(&health.endpoint, config, root, state_path)?;
    Ok(Some(ResolvedPluginHealth {
        kind,
        endpoint,
        startup_timeout: Duration::from_secs(health.startup_timeout_seconds.clamp(1, 120)),
        request_timeout: Duration::from_millis(health.request_timeout_millis.clamp(100, 10_000)),
        interval: Duration::from_millis(health.interval_millis.clamp(25, 5_000)),
    }))
}

fn render_manifest_template(
    template: &str,
    config: &Value,
    plugin_root: &Path,
    state_path: &Path,
) -> Result<String, String> {
    let mut rendered = template
        .replace("{state}", &state_path.to_string_lossy())
        .replace("{pluginRoot}", &plugin_root.to_string_lossy());
    if let Some(values) = config.as_object() {
        for (key, value) in values {
            let Some(scalar) = config_scalar(value) else {
                continue;
            };
            rendered = rendered
                .replace(&format!("{{config.{key}}}"), &scalar)
                .replace(&format!("{{{key}}}"), &scalar);
        }
    }
    if rendered.contains("{port}") {
        rendered = rendered.replace("{port}", "34121");
    }
    if let Some(start) = rendered.find("{config.") {
        let unresolved = rendered[start..]
            .split_once('}')
            .map_or(&rendered[start..], |(token, _)| token);
        return Err(format!(
            "插件模板引用了不存在或非标量的配置：{unresolved}}}"
        ));
    }
    Ok(rendered)
}

fn config_scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null => Some(String::new()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn render_json_templates(
    value: &Value,
    config: &Value,
    plugin_root: &Path,
    state_path: &Path,
) -> Result<Value, String> {
    match value {
        Value::String(value) => Ok(Value::String(render_manifest_template(
            value,
            config,
            plugin_root,
            state_path,
        )?)),
        Value::Array(values) => values
            .iter()
            .map(|value| render_json_templates(value, config, plugin_root, state_path))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                Ok((
                    key.clone(),
                    render_json_templates(value, config, plugin_root, state_path)?,
                ))
            })
            .collect::<Result<serde_json::Map<_, _>, String>>()
            .map(Value::Object),
        _ => Ok(value.clone()),
    }
}

fn load_plugin_config_schema(root: &Path, manifest: &PluginManifest) -> Result<Value, String> {
    let schema = match &manifest.config_schema {
        Value::Null => {
            let legacy = root.join("config.schema.json");
            if !legacy.is_file() {
                return Ok(json!({"type":"object","properties":{}}));
            }
            read_plugin_json_file(&legacy)?
        }
        Value::String(relative) => {
            validate_relative_plugin_path(relative, "插件 config_schema")?;
            read_plugin_json_file(&resolve_plugin_path(root, relative, true)?)?
        }
        Value::Object(descriptor)
            if descriptor.len() == 1
                && descriptor.get("path").and_then(Value::as_str).is_some() =>
        {
            let relative = descriptor
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            validate_relative_plugin_path(relative, "插件 config_schema.path")?;
            read_plugin_json_file(&resolve_plugin_path(root, relative, true)?)?
        }
        Value::Object(_) => manifest.config_schema.clone(),
        _ => return Err("config_schema 必须是 JSON Schema 对象或相对文件路径".to_owned()),
    };
    if !schema.is_object() {
        return Err("config schema 根节点必须是对象".to_owned());
    }
    Ok(schema)
}

fn read_plugin_json_file(path: &Path) -> Result<Value, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_CONFIG_BYTES as u64 {
        return Err("config schema 文件过大".to_owned());
    }
    serde_json::from_str(&fs::read_to_string(path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("{} 不是有效 JSON：{error}", path.display()))
}

fn validate_config_primitives(config: &Value, schema: &Value) -> Result<(), String> {
    let values = config
        .as_object()
        .ok_or_else(|| "插件配置根节点必须是 object".to_owned())?;
    let schema_object = schema
        .as_object()
        .ok_or_else(|| "config schema 根节点必须是 object".to_owned())?;
    if let Some(required) = schema_object.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !values.contains_key(key) {
                return Err(format!("插件配置缺少必填字段：{key}"));
            }
        }
    }
    let properties = schema_object.get("properties").and_then(Value::as_object);
    if schema_object
        .get("additionalProperties")
        .and_then(Value::as_bool)
        == Some(false)
    {
        for key in values.keys() {
            if !properties.is_some_and(|properties| properties.contains_key(key)) {
                return Err(format!("插件配置包含未声明字段：{key}"));
            }
        }
    }
    let Some(properties) = properties else {
        return Ok(());
    };
    for (key, value) in values {
        let Some(descriptor) = properties.get(key).and_then(Value::as_object) else {
            continue;
        };
        if let Some(expected) = descriptor.get("type").and_then(Value::as_str) {
            let matches = match expected {
                "string" => value.is_string(),
                "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
                "number" => value.is_number(),
                "boolean" => value.is_boolean(),
                "object" => value.is_object(),
                "array" => value.is_array(),
                "null" => value.is_null(),
                _ => true,
            };
            if !matches {
                return Err(format!("插件配置字段 {key} 的类型必须是 {expected}"));
            }
        }
        if let Some(allowed) = descriptor.get("enum").and_then(Value::as_array)
            && !allowed.contains(value)
        {
            return Err(format!("插件配置字段 {key} 不在允许值范围内"));
        }
        if let Some(number) = value.as_f64() {
            if descriptor
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|minimum| number < minimum)
            {
                return Err(format!("插件配置字段 {key} 小于 minimum"));
            }
            if descriptor
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|maximum| number > maximum)
            {
                return Err(format!("插件配置字段 {key} 大于 maximum"));
            }
        }
        if let Some(text) = value.as_str() {
            let length = text.chars().count() as u64;
            if descriptor
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|minimum| length < minimum)
            {
                return Err(format!("插件配置字段 {key} 短于 minLength"));
            }
            if descriptor
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|maximum| length > maximum)
            {
                return Err(format!("插件配置字段 {key} 长于 maxLength"));
            }
        }
    }
    Ok(())
}

fn is_secret_config_field(key: &str, descriptor: Option<&Value>, value: Option<&Value>) -> bool {
    let declared_secret = descriptor
        .and_then(|descriptor| descriptor.get("secret"))
        .and_then(Value::as_bool)
        == Some(true)
        || descriptor
            .and_then(|descriptor| descriptor.get("format"))
            .and_then(Value::as_str)
            == Some("password");
    let normalized = key.to_ascii_lowercase();
    declared_secret
        || value.is_some_and(Value::is_string)
            && ["key", "token", "secret", "password"]
                .iter()
                .any(|fragment| normalized.contains(fragment))
}

fn configured_plugin_secrets(schema: &Value, config: &Value) -> HashMap<String, bool> {
    let descriptors = schema.get("properties").and_then(Value::as_object);
    let values = config.as_object();
    let mut keys = HashSet::new();
    if let Some(descriptors) = descriptors {
        keys.extend(descriptors.keys().cloned());
    }
    if let Some(values) = values {
        keys.extend(values.keys().cloned());
    }
    keys.into_iter()
        .filter_map(|key| {
            let descriptor = descriptors.and_then(|descriptors| descriptors.get(&key));
            let value = values.and_then(|values| values.get(&key));
            is_secret_config_field(&key, descriptor, value).then(|| {
                let configured = value.is_some_and(|value| match value {
                    Value::String(value) => !value.trim().is_empty(),
                    Value::Null => false,
                    _ => true,
                });
                (key, configured)
            })
        })
        .collect()
}

fn redact_plugin_config(schema: &Value, config: &Value) -> Value {
    let descriptors = schema.get("properties").and_then(Value::as_object);
    let Some(values) = config.as_object() else {
        return json!({});
    };
    Value::Object(
        values
            .iter()
            .filter(|(key, value)| {
                !is_secret_config_field(
                    key,
                    descriptors.and_then(|descriptors| descriptors.get(*key)),
                    Some(value),
                )
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn preserve_unchanged_plugin_secrets(
    incoming: &mut Value,
    current: &Value,
    schema: &Value,
) -> Result<(), String> {
    let incoming = incoming
        .as_object_mut()
        .ok_or_else(|| "插件配置根节点必须是 object".to_owned())?;
    let Some(current) = current.as_object() else {
        return Ok(());
    };
    let descriptors = schema.get("properties").and_then(Value::as_object);
    for (key, current_value) in current {
        let descriptor = descriptors.and_then(|descriptors| descriptors.get(key));
        if !is_secret_config_field(key, descriptor, Some(current_value)) {
            continue;
        }
        let unchanged = incoming
            .get(key)
            .is_none_or(|value| value.as_str().is_some_and(|value| value.is_empty()));
        if unchanged {
            incoming.insert(key.clone(), current_value.clone());
        }
    }
    Ok(())
}

fn plugin_secret_values(root: &Path, manifest: &PluginManifest, config: &Value) -> Vec<String> {
    let schema = load_plugin_config_schema(root, manifest).ok();
    let descriptors = schema
        .as_ref()
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object);
    config
        .as_object()
        .into_iter()
        .flat_map(|values| values.iter())
        .filter(|(key, value)| {
            let descriptor = descriptors.and_then(|descriptors| descriptors.get(*key));
            is_secret_config_field(key, descriptor, Some(value))
        })
        .filter_map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn secure_dify2api_config(config: &mut Value) -> Result<bool, String> {
    let values = config
        .as_object_mut()
        .ok_or_else(|| "dify2api 配置根节点必须是 object".to_owned())?;
    let mut changed = false;
    let proxy_missing = values
        .get("proxy_api_key")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty());
    if proxy_missing {
        values.insert(
            "proxy_api_key".to_owned(),
            Value::String(format!(
                "{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            )),
        );
        changed = true;
    } else if values
        .get("proxy_api_key")
        .and_then(Value::as_str)
        .is_some_and(|value| value.len() < 24)
    {
        return Err("dify2api proxy_api_key 至少需要 24 个字符".to_owned());
    }
    let port = values
        .get("listen_addr")
        .and_then(Value::as_str)
        .and_then(|value| {
            value
                .rsplit_once(':')
                .map_or(Some(value), |(_, port)| Some(port))
        })
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(34123);
    let loopback = format!("127.0.0.1:{port}");
    if values.get("listen_addr").and_then(Value::as_str) != Some(loopback.as_str()) {
        values.insert("listen_addr".to_owned(), Value::String(loopback));
        changed = true;
    }
    Ok(changed)
}

fn run_plugin_debugger_inner(
    workspace_root: &Path,
    plugin_id: &str,
    endpoint_id: &str,
    request: Option<Value>,
) -> Result<PluginDebuggerResponse, String> {
    validate_identifier(plugin_id, "插件")?;
    validate_component_identifier(endpoint_id, "调试 endpoint")?;
    let (root, manifest) = load_plugin(workspace_root, plugin_id)?;
    let state = read_plugin_state(&root, &manifest)?;
    let redactions = plugin_secret_values(&root, &manifest, &state.config);
    if !state.enabled {
        return Err(format!("插件未启用：{plugin_id}"));
    }
    let endpoint = manifest
        .debugger
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == endpoint_id)
        .ok_or_else(|| format!("插件未声明调试 endpoint：{endpoint_id}"))?;
    let state_path = root.join("state.json");
    let rendered_endpoint =
        render_manifest_template(&endpoint.endpoint, &state.config, &root, &state_path)?;
    validate_loopback_http_endpoint(&rendered_endpoint)?;
    let method = endpoint.method.trim().to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return Err(format!("调试 endpoint {} 的 method 不受支持", endpoint.id));
    }
    let rendered_defaults = render_json_templates(
        &endpoint.request_defaults,
        &state.config,
        &root,
        &state_path,
    )?;
    let payload = merge_debugger_request(&rendered_defaults, request);
    let payload_bytes =
        serde_json::to_vec(&payload).map_err(|error| format!("调试请求无效：{error}"))?;
    if payload_bytes.len() > MAX_DEBUG_REQUEST_BYTES {
        return Err(format!(
            "插件调试请求超过 {} KiB",
            MAX_DEBUG_REQUEST_BYTES / 1024
        ));
    }
    let bearer = if endpoint.bearer_config_key.trim().is_empty() {
        None
    } else {
        state
            .config
            .get(&endpoint.bearer_config_key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
    };
    let timeout = endpoint.timeout_seconds.clamp(1, MAX_DEBUG_TIMEOUT_SECONDS);
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .proxy(None)
        .http_status_as_error(false)
        .max_redirects(0)
        .build();
    let http = ureq::Agent::new_with_config(config);
    let started = Instant::now();
    let mut response = match method.as_str() {
        "POST" | "PUT" | "PATCH" => {
            let mut builder = match method.as_str() {
                "POST" => http.post(&rendered_endpoint),
                "PUT" => http.put(&rendered_endpoint),
                _ => http.patch(&rendered_endpoint),
            }
            .header("Accept", "application/json")
            .header("User-Agent", "DRPA-Plugin-Debugger/1");
            if let Some(secret) = &bearer {
                builder = builder.header("Authorization", &format!("Bearer {secret}"));
            }
            builder
                .send_json(&payload)
                .map_err(|error| format!("插件调试请求失败：{error}"))?
        }
        "DELETE" => {
            let mut builder = http
                .delete(&rendered_endpoint)
                .header("Accept", "application/json")
                .header("User-Agent", "DRPA-Plugin-Debugger/1");
            if let Some(secret) = &bearer {
                builder = builder.header("Authorization", &format!("Bearer {secret}"));
            }
            builder
                .call()
                .map_err(|error| format!("插件调试请求失败：{error}"))?
        }
        _ => {
            let mut builder = http
                .get(&rendered_endpoint)
                .header("Accept", "application/json")
                .header("User-Agent", "DRPA-Plugin-Debugger/1");
            if let Some(secret) = &bearer {
                builder = builder.header("Authorization", &format!("Bearer {secret}"));
            }
            builder
                .call()
                .map_err(|error| format!("插件调试请求失败：{error}"))?
        }
    };
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let declared_truncated = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_DEBUG_RESPONSE_BYTES);
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take((MAX_DEBUG_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取插件调试响应失败：{error}"))?;
    let truncated = declared_truncated || bytes.len() > MAX_DEBUG_RESPONSE_BYTES;
    bytes.truncate(MAX_DEBUG_RESPONSE_BYTES);
    let text = redact_plugin_text(String::from_utf8_lossy(&bytes).into_owned(), &redactions);
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Ok(PluginDebuggerResponse {
        endpoint_id: endpoint.id.clone(),
        status,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        content_type,
        body,
        truncated,
    })
}

fn redact_plugin_text(mut text: String, secrets: &[String]) -> String {
    for secret in secrets {
        text = text.replace(secret, "[REDACTED]");
    }
    text
}

fn redact_plugin_value(value: Value, secrets: &[String]) -> Value {
    match value {
        Value::String(value) => Value::String(redact_plugin_text(value, secrets)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| redact_plugin_value(value, secrets))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, redact_plugin_value(value, secrets)))
                .collect(),
        ),
        value => value,
    }
}

pub(crate) fn redact_plugin_error(workspace_root: &Path, plugin_id: &str, error: String) -> String {
    if let Ok((root, manifest)) = load_plugin(workspace_root, plugin_id)
        && let Ok(state) = read_plugin_state(&root, &manifest)
    {
        return redact_plugin_text(
            error,
            &plugin_secret_values(&root, &manifest, &state.config),
        );
    }
    error
}

fn merge_debugger_request(defaults: &Value, request: Option<Value>) -> Value {
    let Some(request) = request else {
        return defaults.clone();
    };
    match (defaults, request) {
        (Value::Object(defaults), Value::Object(request)) => {
            let mut merged = defaults.clone();
            merged.extend(request);
            Value::Object(merged)
        }
        (_, request) => request,
    }
}

fn validate_loopback_http_endpoint(value: &str) -> Result<(), String> {
    let parsed = Url::parse(value).map_err(|error| format!("调试 endpoint URL 无效：{error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("调试 endpoint 只允许 http/https".to_owned());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("调试 endpoint 不允许 URL 用户凭据".to_owned());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "调试 endpoint 缺少 host".to_owned())?
        .trim_matches(['[', ']']);
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !loopback {
        return Err("调试 endpoint 只允许 loopback 地址".to_owned());
    }
    Ok(())
}

fn ensure_plugins_root(workspace_root: &Path) -> Result<(), String> {
    let cache = INITIALIZED_PLUGIN_ROOTS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut initialized = cache
        .lock()
        .map_err(|_| "插件根目录初始化缓存已损坏".to_owned())?;
    if initialized.contains(workspace_root) && plugins_root(workspace_root).is_dir() {
        return Ok(());
    }
    ensure_plugins_root_uncached(workspace_root)?;
    initialized.insert(workspace_root.to_path_buf());
    Ok(())
}

fn ensure_plugins_root_uncached(workspace_root: &Path) -> Result<(), String> {
    let root = plugins_root(workspace_root);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let service_name = BUILTIN_DIFY2API_SERVICE_NAME;
    // Unsupported desktop targets use an empty compile-time sentinel so their
    // legacy built-in service is retired instead of installing a foreign binary.
    #[allow(clippy::const_is_empty)]
    if service_name.is_empty() {
        return retire_legacy_builtin_dify(&root);
    }
    let builtin = root.join("dify2api");
    let removed = root.join(".removed-dify2api");
    if !removed.is_file() && !builtin.join("plugin.yaml").is_file() {
        fs::create_dir_all(builtin.join("service")).map_err(|error| error.to_string())?;
        fs::write(builtin.join(".builtin"), b"dify2api@1.0.0\n")
            .map_err(|error| error.to_string())?;
    }
    if builtin.join(".builtin").is_file() {
        let marker = fs::read_to_string(builtin.join(".builtin")).unwrap_or_default();
        let service_path = builtin.join(service_name);
        let bundle_is_current = marker.trim() == BUILTIN_DIFY2API_MARKER
            && builtin.join("plugin.yaml").is_file()
            && builtin.join("config.schema.json").is_file()
            && builtin.join("README.md").is_file()
            && builtin.join("THIRD_PARTY_NOTICES.txt").is_file()
            && builtin.join("BUILD-PROVENANCE.md").is_file()
            && fs::metadata(&service_path).is_ok_and(|metadata| {
                metadata.is_file() && metadata.len() == BUILTIN_DIFY2API_SERVICE.len() as u64
            });
        if bundle_is_current {
            return Ok(());
        }
        write_if_different(&builtin.join("plugin.yaml"), BUILTIN_DIFY2API_MANIFEST)?;
        write_if_different(
            &builtin.join("config.schema.json"),
            BUILTIN_DIFY2API_CONFIG_SCHEMA,
        )?;
        write_if_different(&builtin.join("README.md"), BUILTIN_DIFY2API_README)?;
        write_if_different(
            &builtin.join("THIRD_PARTY_NOTICES.txt"),
            BUILTIN_DIFY2API_NOTICES,
        )?;
        write_if_different(
            &builtin.join("BUILD-PROVENANCE.md"),
            BUILTIN_DIFY2API_PROVENANCE,
        )?;
        write_bytes_if_different(&service_path, BUILTIN_DIFY2API_SERVICE)?;
        if marker.trim() != BUILTIN_DIFY2API_MARKER {
            fs::write(
                builtin.join(".builtin"),
                format!("{BUILTIN_DIFY2API_MARKER}\n"),
            )
            .map_err(|error| error.to_string())?;
        }
        ensure_executable(&service_path)?;
        migrate_builtin_dify_state(&root, &builtin)?;
        ensure_builtin_dify2api_state(&builtin)?;
    } else {
        retire_legacy_builtin_dify(&root)?;
    }
    Ok(())
}

fn migrate_builtin_dify_state(plugins_root: &Path, target: &Path) -> Result<(), String> {
    let legacy = plugins_root.join("dify-loves-hermes");
    if !legacy.join(".builtin").is_file() {
        return Ok(());
    }
    let target_manifest = load_manifest_from_root(target, Some("dify2api"))?;
    let mut target_state = read_plugin_state(target, &target_manifest)?;
    if let Ok(legacy_manifest) = load_manifest_from_root(&legacy, Some("dify-loves-hermes"))
        && let Ok(legacy_state) = read_plugin_state(&legacy, &legacy_manifest)
    {
        target_state.enabled = legacy_state.enabled;
        target_state.autostart = legacy_state.autostart;
        let target_config = target_state
            .config
            .as_object_mut()
            .ok_or_else(|| "dify2api 默认配置必须是 object".to_owned())?;
        let legacy_config = legacy_state.config.as_object();
        for (old_key, new_key) in [
            ("base_url", "dify_base_url"),
            ("api_key", "dify_api_key"),
            ("model", "model_name"),
            ("tool_bridge", "enable_tool_emu"),
        ] {
            if let Some(value) = legacy_config.and_then(|config| config.get(old_key)) {
                target_config.insert(new_key.to_owned(), value.clone());
            }
        }
        if let Some(port) = legacy_config
            .and_then(|config| config.get("port"))
            .and_then(Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port > 0)
        {
            target_config.insert(
                "listen_addr".to_owned(),
                Value::String(format!("127.0.0.1:{port}")),
            );
        }
    }
    secure_dify2api_config(&mut target_state.config)?;
    write_plugin_state(target, &target_state)?;
    fs::remove_dir_all(&legacy).map_err(|error| format!("退役旧内置 Dify 插件失败：{error}"))
}

fn ensure_builtin_dify2api_state(root: &Path) -> Result<(), String> {
    let manifest = load_manifest_from_root(root, Some("dify2api"))?;
    let state_path = root.join("state.json");
    let mut state = read_plugin_state(root, &manifest)?;
    if !state_path.is_file() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("为 dify2api 分配本地端口失败：{error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("读取 dify2api 本地端口失败：{error}"))?
            .port();
        drop(listener);
        state.config["listen_addr"] = json!(format!("127.0.0.1:{port}"));
    }
    let changed = secure_dify2api_config(&mut state.config)?;
    if changed || !state_path.is_file() {
        write_plugin_state(root, &state)?;
    }
    Ok(())
}

fn retire_legacy_builtin_dify(plugins_root: &Path) -> Result<(), String> {
    let legacy = plugins_root.join("dify-loves-hermes");
    if legacy.join(".builtin").is_file() {
        fs::remove_dir_all(legacy).map_err(|error| format!("退役旧内置 Dify 插件失败：{error}"))?;
    }
    Ok(())
}

fn install_plugin_archive(workspace_root: &Path, package_path: &Path) -> Result<(), String> {
    ensure_plugins_root(workspace_root)?;
    if !package_path.is_file() {
        return Err("插件包不存在".to_owned());
    }
    if fs::metadata(package_path)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_PLUGIN_ARCHIVE_BYTES
    {
        return Err("插件包超过 256 MiB".to_owned());
    }
    let manifest = read_archive_manifest(package_path)?;
    let target = plugins_root(workspace_root).join(&manifest.id);
    if target.exists() {
        return Err(format!("插件已存在：{}", manifest.id));
    }
    let stage = plugins_root(workspace_root).join(format!(".install-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&stage).map_err(|error| error.to_string())?;
    let result = (|| {
        let file = File::open(package_path).map_err(|error| error.to_string())?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|error| format!("插件包不是有效 ZIP：{error}"))?;
        if archive.len() > MAX_PLUGIN_FILES {
            return Err(format!("插件文件超过 {MAX_PLUGIN_FILES} 个"));
        }
        let mut unpacked_bytes = 0_u64;
        for index in 0..archive.len() {
            let source = archive.by_index(index).map_err(|error| error.to_string())?;
            unpacked_bytes = unpacked_bytes
                .checked_add(source.size())
                .ok_or_else(|| "插件解压大小溢出".to_owned())?;
            if unpacked_bytes > MAX_PLUGIN_UNPACKED_BYTES {
                return Err("插件解压后超过 512 MiB".to_owned());
            }
        }
        for index in 0..archive.len() {
            let mut source = archive.by_index(index).map_err(|error| error.to_string())?;
            let enclosed = source
                .enclosed_name()
                .ok_or_else(|| "插件包包含不安全路径".to_owned())?;
            let output = stage.join(enclosed);
            if source.is_dir() {
                fs::create_dir_all(&output).map_err(|error| error.to_string())?;
                continue;
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut target = File::create(&output).map_err(|error| error.to_string())?;
            std::io::copy(&mut source, &mut target).map_err(|error| error.to_string())?;
        }
        let installed_manifest = load_manifest_from_root(&stage, Some(&manifest.id))?;
        validate_plugin_root_files(&stage, &installed_manifest)?;
        ensure_manifest_executables(&stage, &installed_manifest)?;
        fs::rename(&stage, &target).map_err(|error| error.to_string())?;
        if manifest.id == "dify2api" {
            let _ = fs::remove_file(plugins_root(workspace_root).join(".removed-dify2api"));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn read_archive_manifest(package_path: &Path) -> Result<PluginManifest, String> {
    let file = File::open(package_path).map_err(|error| error.to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("插件包不是有效 ZIP：{error}"))?;
    let mut source = archive
        .by_name("plugin.yaml")
        .map_err(|_| "插件包根目录缺少 plugin.yaml".to_owned())?;
    if source.size() > MAX_MANIFEST_BYTES as u64 {
        return Err("plugin.yaml 过大".to_owned());
    }
    let mut text = String::new();
    source
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    parse_plugin_manifest(&text, None)
}

fn load_plugin(
    workspace_root: &Path,
    plugin_id: &str,
) -> Result<(PathBuf, PluginManifest), String> {
    ensure_plugins_root(workspace_root)?;
    validate_identifier(plugin_id, "插件")?;
    let root = plugins_root(workspace_root).join(plugin_id);
    let manifest = load_manifest_from_root(&root, Some(plugin_id))?;
    Ok((root, manifest))
}

fn load_manifest_from_root(
    root: &Path,
    expected_id: Option<&str>,
) -> Result<PluginManifest, String> {
    let path = root.join("plugin.yaml");
    let metadata =
        fs::metadata(&path).map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    if metadata.len() > MAX_MANIFEST_BYTES as u64 {
        return Err("plugin.yaml 过大".to_owned());
    }
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse_plugin_manifest(&source, expected_id)
}

fn parse_plugin_manifest(
    source: &str,
    expected_id: Option<&str>,
) -> Result<PluginManifest, String> {
    let manifest: PluginManifest =
        serde_yaml::from_str(source).map_err(|error| format!("plugin.yaml 无效：{error}"))?;
    if manifest.schema != 1 {
        return Err("plugin.yaml schema 必须为 1".to_owned());
    }
    validate_identifier(&manifest.id, "插件")?;
    if let Some(expected) = expected_id
        && manifest.id != expected
    {
        return Err(format!("plugin.yaml 的 id 必须与目录名一致：{expected}"));
    }
    if manifest.name.trim().is_empty()
        || manifest.version.trim().is_empty()
        || manifest.description.trim().is_empty()
    {
        return Err("plugin.yaml 需要 name、version 和 description".to_owned());
    }
    for service in resolved_plugin_services(&manifest)? {
        validate_component_identifier(&service.id, "插件服务")?;
        validate_plugin_service_manifest(&service.manifest)?;
    }
    let mut tool_provider_ids = HashSet::new();
    for provider in &manifest.tool_providers {
        validate_component_identifier(&provider.id, "工具 provider")?;
        if !tool_provider_ids.insert(provider.id.as_str()) {
            return Err(format!("工具 provider id 重复：{}", provider.id));
        }
        validate_plugin_runtime(&provider.runtime, "工具 provider runtime")?;
        validate_relative_plugin_path(
            provider
                .entry
                .split_once(':')
                .map_or(provider.entry.as_str(), |(path, _)| path),
            "工具 provider entry",
        )?;
        if provider.timeout_seconds == 0 || provider.timeout_seconds > 300 {
            return Err(format!("工具 provider {} 的超时设置无效", provider.id));
        }
        if provider.tools.is_empty() {
            return Err(format!("工具 provider {} 没有声明 tools", provider.id));
        }
        for tool in &provider.tools {
            if !tool.callable.is_empty() {
                validate_python_callable(&tool.callable)?;
            }
        }
    }
    for tool in resolved_plugin_tools(&manifest)? {
        validate_tool_name(&tool.name)?;
        if qualified_plugin_tool_name(&manifest.id, &tool.name).len() > 64 {
            return Err(format!(
                "插件工具 {} 的完整注册名称超过 64 个字符",
                tool.name
            ));
        }
        validate_plugin_runtime(&tool.runtime, &format!("插件工具 {} runtime", tool.name))?;
        validate_relative_plugin_path(
            tool.entry
                .split_once(':')
                .map_or(tool.entry.as_str(), |(path, _)| path),
            "插件工具 entry",
        )?;
        if !tool.parameters.is_object() || tool.timeout_seconds == 0 || tool.timeout_seconds > 300 {
            return Err(format!("插件工具 {} 的参数或超时设置无效", tool.name));
        }
    }
    let service_ids = resolved_plugin_services(&manifest)?
        .into_iter()
        .map(|service| service.id)
        .collect::<HashSet<_>>();
    let mut provider_ids = HashSet::new();
    for provider in &manifest.providers {
        validate_component_identifier(&provider.id, "Provider")?;
        if !provider_ids.insert(provider.id.as_str()) {
            return Err(format!("Provider id 重复：{}", provider.id));
        }
        if provider.title.trim().is_empty()
            || provider.title.len() > 100
            || provider.protocol.trim().is_empty()
        {
            return Err(format!(
                "Provider {} 的 title 或 protocol 无效",
                provider.id
            ));
        }
        if !service_ids.contains(&provider.service_id) {
            return Err(format!(
                "Provider {} 引用了不存在的 service：{}",
                provider.id, provider.service_id
            ));
        }
        if !provider.endpoint.is_empty() && !is_http_template(&provider.endpoint) {
            return Err(format!(
                "Provider {} 的 endpoint 必须是 http(s) 模板",
                provider.id
            ));
        }
        for (label, key) in [
            ("model_config_key", provider.model_config_key.as_str()),
            ("api_key_config_key", provider.api_key_config_key.as_str()),
        ] {
            if key.len() > 128
                || key
                    .chars()
                    .any(|value| value.is_control() || matches!(value, '{' | '}'))
            {
                return Err(format!("Provider {} 的 {label} 无效", provider.id));
            }
        }
    }
    validate_plugin_debugger_manifest(&manifest.debugger)?;
    match &manifest.config_schema {
        Value::Null | Value::Object(_) => {}
        Value::String(relative) => {
            validate_relative_plugin_path(relative, "插件 config_schema")?;
        }
        _ => return Err("config_schema 必须是 JSON Schema 对象或相对文件路径".to_owned()),
    }
    Ok(manifest)
}

fn validate_plugin_service_manifest(service: &PluginServiceManifest) -> Result<(), String> {
    validate_plugin_runtime(&service.runtime, "插件 service.runtime")?;
    if !matches!(
        service.transport.trim().to_ascii_lowercase().as_str(),
        "http" | "process" | ""
    ) {
        return Err("插件 service.transport 当前只支持 http 或 process".to_owned());
    }
    if service.entry.trim().is_empty() && service.entry_by_platform.is_empty() {
        return Err("插件 service 需要 entry 或 entry_by_platform".to_owned());
    }
    if !service.entry.trim().is_empty() {
        validate_relative_plugin_path(&service.entry, "插件 service.entry")?;
    }
    for (platform, entry) in &service.entry_by_platform {
        if !matches!(platform.as_str(), "windows" | "linux" | "macos") {
            return Err(format!(
                "插件 service.entry_by_platform 不支持平台：{platform}"
            ));
        }
        validate_relative_plugin_path(entry, "插件 service.entry_by_platform")?;
    }
    resolve_service_entry(service)?;
    if !service.endpoint.is_empty() && !is_http_template(&service.endpoint) {
        return Err("插件 service.endpoint 必须是 http(s) 地址模板".to_owned());
    }
    if !service.healthcheck.is_empty() && !is_http_template(&service.healthcheck) {
        return Err("插件 service.healthcheck 必须是 http(s) 地址模板".to_owned());
    }
    if !service.test_endpoint.is_empty() && !is_http_template(&service.test_endpoint) {
        return Err("插件 service.test_endpoint 必须是 http(s) 地址模板".to_owned());
    }
    if let Some(health) = &service.health {
        if !matches!(health.kind.trim(), "http" | "process") {
            return Err("插件 service.health.kind 只支持 http 或 process".to_owned());
        }
        if health.kind == "http" && !is_http_template(&health.endpoint) {
            return Err("插件 service.health.endpoint 必须是 http(s) 地址模板".to_owned());
        }
        if health.startup_timeout_seconds == 0
            || health.startup_timeout_seconds > 120
            || health.request_timeout_millis < 100
            || health.request_timeout_millis > 10_000
            || health.interval_millis < 25
            || health.interval_millis > 5_000
        {
            return Err("插件 service.health 的超时或轮询设置无效".to_owned());
        }
    }
    if !matches!(
        service.logs.format.as_str(),
        "text" | "json" | "drpa-event-line"
    ) {
        return Err("插件 service.logs.format 只支持 text、json 或 drpa-event-line".to_owned());
    }
    Ok(())
}

fn validate_plugin_runtime(runtime: &str, label: &str) -> Result<(), String> {
    if matches!(runtime, "bundled-python" | "executable") {
        Ok(())
    } else {
        Err(format!("{label} 只支持 bundled-python 或 executable"))
    }
}

fn validate_plugin_debugger_manifest(debugger: &PluginDebuggerManifest) -> Result<(), String> {
    let mut endpoint_ids = HashSet::new();
    for endpoint in &debugger.endpoints {
        validate_component_identifier(&endpoint.id, "调试 endpoint")?;
        if !endpoint_ids.insert(endpoint.id.as_str()) {
            return Err(format!("调试 endpoint id 重复：{}", endpoint.id));
        }
        if endpoint.title.trim().is_empty() || endpoint.title.len() > 100 {
            return Err(format!("调试 endpoint {} 的 title 无效", endpoint.id));
        }
        if !matches!(
            endpoint.method.trim().to_ascii_uppercase().as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
        ) {
            return Err(format!("调试 endpoint {} 的 method 无效", endpoint.id));
        }
        if !is_http_template(&endpoint.endpoint) {
            return Err(format!(
                "调试 endpoint {} 必须是 http(s) 地址模板",
                endpoint.id
            ));
        }
        if endpoint.timeout_seconds == 0 || endpoint.timeout_seconds > MAX_DEBUG_TIMEOUT_SECONDS {
            return Err(format!(
                "调试 endpoint {} 的 timeout_seconds 无效",
                endpoint.id
            ));
        }
        if !endpoint.bearer_config_key.is_empty()
            && (endpoint.bearer_config_key.len() > 128
                || endpoint
                    .bearer_config_key
                    .chars()
                    .any(|value| value.is_control() || matches!(value, '{' | '}')))
        {
            return Err(format!(
                "调试 endpoint {} 的 bearer_config_key 无效",
                endpoint.id
            ));
        }
        if serde_json::to_vec(&endpoint.request_defaults)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_DEBUG_REQUEST_BYTES
        {
            return Err(format!(
                "调试 endpoint {} 的 request_defaults 过大",
                endpoint.id
            ));
        }
    }
    let mut panel_ids = HashSet::new();
    for panel in &debugger.panels {
        validate_component_identifier(&panel.id, "调试 panel")?;
        if !panel_ids.insert(panel.id.as_str()) {
            return Err(format!("调试 panel id 重复：{}", panel.id));
        }
        if panel.title.trim().is_empty() || panel.title.len() > 100 || panel.kind.trim().is_empty()
        {
            return Err(format!("调试 panel {} 的 title 或 kind 无效", panel.id));
        }
        if !panel.endpoint.is_empty() && !endpoint_ids.contains(panel.endpoint.as_str()) {
            return Err(format!(
                "调试 panel {} 引用了不存在的 endpoint：{}",
                panel.id, panel.endpoint
            ));
        }
    }
    Ok(())
}

fn is_http_template(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn validate_relative_plugin_path(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || Path::new(value).is_absolute() {
        return Err(format!("{label} 无效"));
    }
    if Path::new(value)
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("{label} 包含不安全片段"));
    }
    Ok(())
}

fn read_plugin_state(root: &Path, manifest: &PluginManifest) -> Result<PluginStateFile, String> {
    let path = root.join("state.json");
    let backup = root.join(".state.backup");
    if !path.is_file() && backup.is_file() {
        fs::rename(&backup, &path).map_err(|error| format!("恢复插件配置备份失败：{error}"))?;
    }
    if !path.is_file() {
        return Ok(PluginStateFile {
            enabled: false,
            autostart: false,
            config: manifest.default_config.clone(),
        });
    }
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_CONFIG_BYTES as u64 {
        return Err("插件配置过大".to_owned());
    }
    set_private_file_permissions(&path)?;
    serde_json::from_str(&fs::read_to_string(path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("插件 state.json 无效：{error}"))
}

fn write_plugin_state(root: &Path, state: &PluginStateFile) -> Result<(), String> {
    let content = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    if content.len() > MAX_CONFIG_BYTES {
        return Err("插件配置过大".to_owned());
    }
    let temporary = root.join(format!(".state-{}.tmp", Uuid::new_v4().simple()));
    fs::write(&temporary, content).map_err(|error| error.to_string())?;
    set_private_file_permissions(&temporary)?;
    let target = root.join("state.json");
    let backup = root.join(".state.backup");
    if target.exists() {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| error.to_string())?;
        }
        fs::rename(&target, &backup).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&temporary, &target) {
        if backup.is_file() {
            let _ = fs::rename(&backup, &target);
        }
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    if backup.is_file() {
        fs::remove_file(backup).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn write_if_different(path: &Path, content: &str) -> Result<(), String> {
    if fs::read(path).ok().as_deref() == Some(content.as_bytes()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, content.as_bytes()).map_err(|error| error.to_string())
}

fn write_bytes_if_different(path: &Path, content: &[u8]) -> Result<(), String> {
    if fs::read(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, content).map_err(|error| error.to_string())
}

fn prepare_service_execution_entry(
    plugin_id: &str,
    root: &Path,
    entry: &Path,
) -> Result<PathBuf, String> {
    ensure_executable(entry)?;
    #[cfg(target_os = "linux")]
    if plugin_id == "dify2api"
        && root.join(".builtin").is_file()
        && entry == root.join(BUILTIN_DIFY2API_SERVICE_NAME)
    {
        return staged_builtin_dify2api_execution_entry();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (plugin_id, root);
    Ok(entry.to_path_buf())
}

#[cfg(target_os = "linux")]
fn staged_builtin_dify2api_execution_entry() -> Result<PathBuf, String> {
    BUILTIN_DIFY2API_EXECUTION_ENTRY
        .get_or_init(|| {
            use std::os::unix::fs::PermissionsExt;

            let runtime_root = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|path| path.is_dir())
                .unwrap_or_else(std::env::temp_dir)
                .join(format!("drpa-next-{}", std::process::id()))
                .join("plugin-runtime");
            fs::create_dir_all(&runtime_root).map_err(|error| {
                format!(
                    "创建 UOS 插件执行缓存 {} 失败：{error}",
                    runtime_root.display()
                )
            })?;
            fs::set_permissions(&runtime_root, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("设置 UOS 插件执行缓存权限失败：{error}"))?;
            let entry = runtime_root.join("dify2api-server");
            write_bytes_if_different(&entry, BUILTIN_DIFY2API_SERVICE)?;
            fs::set_permissions(&entry, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("设置 Dify2API sidecar 执行权限失败：{error}"))?;
            Ok(entry)
        })
        .clone()
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    let mode = permissions.mode();
    if mode & 0o111 == 0 {
        permissions.set_mode(mode | 0o755);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_executable(path: &Path) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("插件 executable 不存在：{}", path.display()))
    }
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    let account = match (
        std::env::var("USERDOMAIN").ok(),
        std::env::var("USERNAME").ok(),
    ) {
        (Some(domain), Some(user)) if !domain.trim().is_empty() && !user.trim().is_empty() => {
            format!("{domain}\\{user}")
        }
        (_, Some(user)) if !user.trim().is_empty() => user,
        _ => return Err("无法确定当前 Windows 账号，不能安全保存插件密钥".to_owned()),
    };
    let mut command = Command::new("icacls.exe");
    command
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{account}:(F)"))
        .arg("/Q");
    hide_child_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法设置插件密钥文件 ACL：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "无法限制插件密钥文件 ACL：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(all(not(unix), not(windows)))]
fn set_private_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn resolve_plugin_path(root: &Path, relative: &str, must_exist: bool) -> Result<PathBuf, String> {
    if relative.trim().is_empty() || Path::new(relative).is_absolute() {
        return Err("插件文件路径无效".to_owned());
    }
    let mut checked = PathBuf::new();
    for component in Path::new(relative).components() {
        match component {
            std::path::Component::Normal(value) => checked.push(value),
            _ => return Err("插件文件路径包含不安全片段".to_owned()),
        }
    }
    let target = root.join(checked);
    if must_exist && !target.exists() {
        return Err(format!("插件文件不存在：{relative}"));
    }
    Ok(target)
}

fn spawn_log_reader<R: Read + Send + 'static>(
    reader: R,
    stream: &'static str,
    service_id: &str,
    format: &str,
    target: Arc<Mutex<VecDeque<PluginLogLine>>>,
    redactions: Arc<Vec<String>>,
) {
    let service_id = service_id.to_owned();
    let format = format.to_owned();
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            let Ok((read, truncated)) = read_bounded_log_line(&mut reader, &mut bytes) else {
                break;
            };
            if read == 0 {
                break;
            }
            let mut raw_message = String::from_utf8_lossy(&bytes)
                .trim_end_matches(['\r', '\n'])
                .to_owned();
            if truncated {
                raw_message.push_str(" …[truncated]");
            }
            for secret in redactions.iter() {
                raw_message = raw_message.replace(secret, "[REDACTED]");
            }
            let event_payload = match format.as_str() {
                "drpa-event-line" => raw_message
                    .strip_prefix("DRPA_PLUGIN_EVENT ")
                    .and_then(|payload| serde_json::from_str::<Value>(payload).ok()),
                "json" => serde_json::from_str::<Value>(&raw_message).ok(),
                _ => None,
            };
            let message = event_payload
                .as_ref()
                .and_then(|value| {
                    value
                        .get("message")
                        .or_else(|| value.get("title"))
                        .or_else(|| value.get("detail"))
                })
                .and_then(Value::as_str)
                .unwrap_or(&raw_message)
                .to_owned();
            let event = event_payload;
            if let Ok(mut lines) = target.lock() {
                lines.push_back(PluginLogLine {
                    timestamp: unix_millis(),
                    stream: stream.to_owned(),
                    message,
                    service_id: service_id.clone(),
                    event,
                });
                while lines.len() > MAX_LOG_LINES {
                    lines.pop_front();
                }
            }
        }
    });
}

fn read_bounded_log_line<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
) -> io::Result<(usize, bool)> {
    let mut total = 0_usize;
    let mut truncated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok((total, truncated));
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        let remaining = MAX_LOG_LINE_BYTES.saturating_sub(output.len());
        let copied = consumed.min(remaining);
        output.extend_from_slice(&available[..copied]);
        if copied < consumed {
            truncated = true;
        }
        let ended = available[..consumed].last() == Some(&b'\n');
        reader.consume(consumed);
        total = total.saturating_add(consumed);
        if ended {
            return Ok((total, truncated));
        }
    }
}

fn run_json_process(
    mut command: Command,
    args: &[String],
    current_dir: &Path,
    request: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    let temporary = std::env::temp_dir().join(format!("drpa-plugin-tool-{}", Uuid::new_v4()));
    fs::create_dir_all(&temporary).map_err(|error| error.to_string())?;
    let stdout_path = temporary.join("stdout.json");
    let stderr_path = temporary.join("stderr.log");
    let stdout = File::create(&stdout_path).map_err(|error| error.to_string())?;
    let stderr = File::create(&stderr_path).map_err(|error| error.to_string())?;
    command
        .args(args)
        .current_dir(current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动插件工具失败：{error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(request.to_string().as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(&temporary);
            return Err(format!("插件工具执行超过 {} 秒", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(30));
    };
    let stdout = read_limited(&stdout_path);
    let stderr = read_limited(&stderr_path);
    let _ = fs::remove_dir_all(&temporary);
    if !status.success() {
        return Err(format!(
            "插件工具退出码 {}：{}",
            status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    serde_json::from_str(stdout.trim())
        .map_err(|error| format!("插件工具输出不是有效 JSON：{error}；输出：{stdout}"))
}

fn read_limited(path: &Path) -> String {
    let mut content = Vec::new();
    if let Ok(file) = File::open(path) {
        let _ = file
            .take(MAX_TOOL_OUTPUT_BYTES as u64)
            .read_to_end(&mut content);
    }
    String::from_utf8_lossy(&content).into_owned()
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 48
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(format!("{label} ID 只允许 1-48 位小写字母、数字和中划线"));
    }
    Ok(())
}

fn validate_component_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 48
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
        || matches!(value.as_bytes().first(), Some(b'-' | b'_'))
        || matches!(value.as_bytes().last(), Some(b'-' | b'_'))
    {
        return Err(format!(
            "{label} ID 只允许 1-48 位小写字母、数字、中划线和下划线"
        ));
    }
    Ok(())
}

fn validate_python_callable(value: &str) -> Result<(), String> {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return Err("工具 callable 不能为空".to_owned());
    };
    if !(first.is_ascii_alphabetic() || first == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!("工具 callable 不是有效 Python 标识符：{value}"));
    }
    Ok(())
}

fn validate_tool_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 48
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || value.starts_with('_')
        || value.ends_with('_')
    {
        return Err("插件工具名称只允许 1-48 位小写字母、数字和下划线".to_owned());
    }
    Ok(())
}

fn qualified_plugin_tool_name(plugin: &str, tool: &str) -> String {
    format!("plugin_{plugin}__{tool}")
}

fn list_plugin_projects_inner(workspace_root: &Path) -> Result<Vec<PluginProjectSummary>, String> {
    let root = plugin_projects_root(workspace_root);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let mut projects = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|error| error.to_string())?
        .flatten()
    {
        if !entry.path().is_dir() || !entry.path().join("plugin.yaml").is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let result = validate_plugin_project_root(&entry.path(), &id);
        match result {
            Ok(manifest) => projects.push(PluginProjectSummary {
                id,
                name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                types: manifest.types,
                directory: entry.path().to_string_lossy().into_owned(),
                valid: true,
                validation_message: "插件清单与入口文件有效".to_owned(),
            }),
            Err(error) => projects.push(PluginProjectSummary {
                id: id.clone(),
                name: id,
                version: "-".to_owned(),
                description: "插件项目需要修复".to_owned(),
                types: Vec::new(),
                directory: entry.path().to_string_lossy().into_owned(),
                valid: false,
                validation_message: error,
            }),
        }
    }
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(projects)
}

fn validate_plugin_project_root(root: &Path, expected_id: &str) -> Result<PluginManifest, String> {
    if !root.is_dir() {
        return Err(format!("插件项目不存在：{expected_id}"));
    }
    let manifest = load_manifest_from_root(root, Some(expected_id))?;
    validate_plugin_root_files(root, &manifest)?;
    Ok(manifest)
}

fn validate_plugin_root_files(root: &Path, manifest: &PluginManifest) -> Result<(), String> {
    for service in manifest.service.iter().chain(manifest.services.iter()) {
        if !service.entry.trim().is_empty() {
            resolve_plugin_path(root, &service.entry, true)?;
        }
        for entry in service.entry_by_platform.values() {
            resolve_plugin_path(root, entry, true)?;
        }
    }
    for tool in resolved_plugin_tools(manifest)? {
        let entry = tool
            .entry
            .split_once(':')
            .map_or(tool.entry.as_str(), |(path, _)| path);
        resolve_plugin_path(root, entry, true)?;
    }
    load_plugin_config_schema(root, manifest)?;
    Ok(())
}

fn plugin_executable_paths(manifest: &PluginManifest) -> HashSet<String> {
    let mut paths = HashSet::new();
    for service in manifest
        .service
        .iter()
        .chain(manifest.services.iter())
        .filter(|service| service.runtime == "executable")
    {
        if !service.entry.trim().is_empty() {
            paths.insert(service.entry.replace('\\', "/"));
        }
        paths.extend(
            service
                .entry_by_platform
                .values()
                .map(|entry| entry.replace('\\', "/")),
        );
    }
    for tool in resolved_plugin_tools(manifest).unwrap_or_default() {
        if tool.runtime == "executable" {
            paths.insert(
                tool.entry
                    .split_once(':')
                    .map_or(tool.entry.as_str(), |(path, _)| path)
                    .replace('\\', "/"),
            );
        }
    }
    paths
}

fn ensure_manifest_executables(root: &Path, manifest: &PluginManifest) -> Result<(), String> {
    for relative in plugin_executable_paths(manifest) {
        ensure_executable(&resolve_plugin_path(root, &relative, true)?)?;
    }
    Ok(())
}

fn collect_plugin_project_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_symlink()
            || name == "__pycache__"
            || name == "state.json"
            || name == ".state.backup"
            || name == ".builtin"
        {
            continue;
        }
        if kind.is_dir() {
            collect_plugin_project_files(root, &entry.path(), output)?;
        } else if kind.is_file() && !name.ends_with(".pyc") {
            output.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            if output.len() > MAX_PLUGIN_FILES {
                return Err(format!("插件项目文件超过 {MAX_PLUGIN_FILES} 个"));
            }
        }
    }
    Ok(())
}

fn plugins_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("plugins")
}

fn plugin_projects_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("plugin-projects")
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
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
    fn seeds_and_lists_the_builtin_dify2api_gateway() {
        let workspace = std::env::temp_dir().join(format!("drpa-plugins-{}", Uuid::new_v4()));
        let manager = PluginManager::default();
        let plugins = list_plugins_inner(&workspace, &manager).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "dify2api");
        assert_eq!(plugins[0].status, "disabled");
        assert!(plugins[0].types.contains(&"provider-adapter".to_owned()));
        assert_eq!(plugins[0].service_count, 1);
        assert_eq!(plugins[0].providers.len(), 1);
        assert!(plugins[0].config.get("proxy_api_key").is_none());
        assert_eq!(
            plugins[0].configured_secrets.get("proxy_api_key"),
            Some(&true)
        );
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn masks_and_preserves_configured_plugin_secrets() {
        let schema = json!({
            "type": "object",
            "properties": {
                "endpoint": {"type": "string"},
                "api_key": {"type": "string", "secret": true}
            }
        });
        let current = json!({"endpoint":"http://127.0.0.1:3000","api_key":"secret-value"});
        let mut incoming = json!({"endpoint":"http://127.0.0.1:4000"});
        preserve_unchanged_plugin_secrets(&mut incoming, &current, &schema).unwrap();
        assert_eq!(incoming["api_key"], "secret-value");
        let redacted = redact_plugin_config(&schema, &incoming);
        assert!(redacted.get("api_key").is_none());
        assert_eq!(
            configured_plugin_secrets(&schema, &incoming).get("api_key"),
            Some(&true)
        );
        let redacted_output = redact_plugin_value(
            json!({"nested":{"echo":"secret-value"},"items":["prefix secret-value suffix"]}),
            &["secret-value".to_owned()],
        );
        assert_eq!(redacted_output["nested"]["echo"], "[REDACTED]");
        assert_eq!(redacted_output["items"][0], "prefix [REDACTED] suffix");
        assert_eq!(
            redact_plugin_text(
                "tool failed with secret-value".to_owned(),
                &["secret-value".to_owned()]
            ),
            "tool failed with [REDACTED]"
        );
    }

    #[test]
    fn plugin_summary_scrubs_secrets_from_public_endpoints() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-plugin-redaction-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("secret-endpoint");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("plugin.yaml"),
            r#"schema: 1
id: secret-endpoint
name: Secret Endpoint
version: 1.0.0
description: redaction test
types: [provider-adapter, service]
config_schema:
  type: object
  properties:
    api_key: { type: string, secret: true }
services:
  - id: api
    title: API
    primary: true
    runtime: executable
    entry: service.exe
    endpoint: "http://127.0.0.1:39001/v1?key={config.api_key}"
    healthcheck: "http://127.0.0.1:39001/health?key={config.api_key}"
providers:
  - id: openai
    title: OpenAI
    protocol: openai
    service_id: api
    endpoint: "http://127.0.0.1:39001/v1?key={config.api_key}"
default_config:
  api_key: tiny-secret
"#,
        )
        .unwrap();
        let plugins = list_plugins_inner(&workspace, &PluginManager::default()).unwrap();
        let summary = plugins
            .iter()
            .find(|plugin| plugin.id == "secret-endpoint")
            .unwrap();
        assert!(!summary.endpoint.contains("tiny-secret"));
        assert!(!summary.services[0].healthcheck.contains("tiny-secret"));
        assert!(!summary.providers[0].endpoint.contains("tiny-secret"));
        assert!(summary.endpoint.contains("[REDACTED]"));
        let manifest = load_manifest_from_root(&root, Some("secret-endpoint")).unwrap();
        write_plugin_state(
            &root,
            &PluginStateFile {
                enabled: true,
                autostart: false,
                config: manifest.default_config.clone(),
            },
        )
        .unwrap();
        assert!(resolve_agent_provider(&workspace, "secret-endpoint", "openai").is_err());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn lists_plugin_tools_for_the_generic_workbench() {
        let workspace = std::env::temp_dir().join(format!("drpa-plugin-tools-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("example-tool");
        fs::create_dir_all(root.join("tools")).unwrap();
        fs::write(
            root.join("plugin.yaml"),
            TOOL_PLUGIN_TEMPLATE
                .replace("{id}", "example-tool")
                .replace("{name}", "Example Tool"),
        )
        .unwrap();
        fs::write(root.join("tools/example.py"), TOOL_PYTHON_TEMPLATE).unwrap();
        let tools = list_plugin_tools_for_workbench(&workspace, "example-tool").unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "example");
        assert!(tools[0].input_schema.get("properties").is_some());
        assert!(
            invoke_plugin_tool_for_workbench(
                &workspace,
                Path::new("missing-python"),
                "example-tool",
                "example",
                &json!("not-an-object"),
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn plugin_manifest_rejects_parent_paths_and_invalid_tools() {
        let workspace = std::env::temp_dir().join(format!("drpa-plugins-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("dify2api");
        assert!(resolve_plugin_path(&root, "../outside.py", false).is_err());
        let invalid = "schema: 1\nid: bad\nname: Bad\nversion: 1\ndescription: Bad\ntools:\n  - name: Bad Tool\n    description: bad\n    runtime: executable\n    entry: bad.exe\n";
        assert!(parse_plugin_manifest(invalid, None).is_err());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn starts_builtin_service_and_waits_for_its_healthcheck() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let workspace = std::env::temp_dir().join(format!("drpa-plugin-start-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("dify2api");
        let manifest = load_manifest_from_root(&root, Some("dify2api")).unwrap();
        let mut config = manifest.default_config.clone();
        config["listen_addr"] = json!(format!("127.0.0.1:{port}"));
        config["dify_api_key"] = json!("test-dify-api-key");
        secure_dify2api_config(&mut config).unwrap();
        write_plugin_state(
            &root,
            &PluginStateFile {
                enabled: true,
                autostart: false,
                config,
            },
        )
        .unwrap();
        let manager = PluginManager::default();

        assert!(!plugin_services_require_bundled_python(&workspace, "dify2api").unwrap());

        if let Err(error) = start_plugin_inner(&workspace, "dify2api", None, &manager) {
            let logs = manager
                .inner
                .logs
                .lock()
                .unwrap()
                .get(&plugin_manager_key(&workspace, "dify2api"))
                .map(|lines| lines.lock().unwrap().clone())
                .unwrap_or_default();
            panic!("{error}; logs={logs:?}");
        }
        let plugins = list_plugins_inner(&workspace, &manager).unwrap();
        assert_eq!(plugins[0].status, "running");
        assert_eq!(plugins[0].endpoint, format!("http://127.0.0.1:{port}/v1"));
        let debug = run_plugin_debugger_inner(&workspace, "dify2api", "health", None).unwrap();
        assert_eq!(debug.status, 200);

        stop_plugin_inner(&workspace, "dify2api", &manager).unwrap();

        let mut state = read_plugin_state(&root, &manifest).unwrap();
        state.autostart = true;
        write_plugin_state(&root, &state).unwrap();
        start_autostart_plugins(&workspace, None, &manager).unwrap();
        assert!(service_is_running(&workspace, "dify2api", "gateway", &manager).unwrap());
        stop_plugin_inner(&workspace, "dify2api", &manager).unwrap();
        let _ = fs::remove_dir_all(workspace);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stages_builtin_dify2api_outside_the_workspace_for_uos_noexec_mounts() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = std::env::temp_dir().join(format!("drpa-plugin-stage-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("dify2api");
        let source = root.join(BUILTIN_DIFY2API_SERVICE_NAME);
        let staged = prepare_service_execution_entry("dify2api", &root, &source).unwrap();
        assert_ne!(staged, source);
        assert_eq!(fs::read(&staged).unwrap(), BUILTIN_DIFY2API_SERVICE);
        assert_ne!(
            fs::metadata(&staged).unwrap().permissions().mode() & 0o111,
            0
        );
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn validates_and_collects_generated_plugin_projects() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-plugin-project-{}", Uuid::new_v4()));
        let root = plugin_projects_root(&workspace).join("example-service");
        fs::create_dir_all(root.join("service")).unwrap();
        fs::write(
            root.join("plugin.yaml"),
            SERVICE_PLUGIN_TEMPLATE
                .replace("{id}", "example-service")
                .replace("{name}", "Example Service"),
        )
        .unwrap();
        fs::write(root.join("service/main.py"), SERVICE_PYTHON_TEMPLATE).unwrap();
        fs::write(root.join("state.json"), "{}").unwrap();
        fs::create_dir_all(root.join("service/__pycache__")).unwrap();
        fs::write(root.join("service/__pycache__/main.pyc"), b"ignored").unwrap();

        let manifest = validate_plugin_project_root(&root, "example-service").unwrap();
        assert!(manifest.types.contains(&"provider-adapter".to_owned()));
        let mut files = Vec::new();
        collect_plugin_project_files(&root, &root, &mut files).unwrap();
        files.sort();
        assert_eq!(files, vec!["plugin.yaml", "service/main.py"]);
        let package = build_plugin_project_inner(&workspace, "example-service").unwrap();
        let mut archive = zip::ZipArchive::new(File::open(package).unwrap()).unwrap();
        assert!(archive.by_name("plugin.yaml").is_ok());
        assert!(archive.by_name("service/main.py").is_ok());
        assert!(archive.by_name("state.json").is_err());

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn parses_plural_capabilities_and_renders_scalar_templates() {
        let manifest = parse_plugin_manifest(BUILTIN_DIFY2API_MANIFEST, Some("dify2api")).unwrap();
        let services = resolved_plugin_services(&manifest).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].id, "gateway");
        assert!(services[0].primary);
        let endpoint_ids = manifest
            .debugger
            .endpoints
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            endpoint_ids,
            HashSet::from(["health", "upstream", "models", "chat", "tool-call"])
        );
        assert_eq!(manifest.providers[0].service_id, "gateway");
        let rendered = render_manifest_template(
            "{pluginRoot}|{state}|{config.listen_addr}",
            &json!({"listen_addr":"127.0.0.1:39999"}),
            Path::new("plugin"),
            Path::new("plugin/state.json"),
        )
        .unwrap();
        assert!(rendered.ends_with("127.0.0.1:39999"));
    }

    #[test]
    fn debugger_rejects_non_loopback_and_bounds_log_lines() {
        assert!(validate_loopback_http_endpoint("http://127.0.0.1:34123/healthz").is_ok());
        assert!(validate_loopback_http_endpoint("http://[::1]:34123/healthz").is_ok());
        assert!(validate_loopback_http_endpoint("https://example.com/debug").is_err());
        let source = vec![b'x'; MAX_LOG_LINE_BYTES + 1024]
            .into_iter()
            .chain(*b"\n")
            .collect::<Vec<_>>();
        let mut reader = BufReader::new(source.as_slice());
        let mut line = Vec::new();
        let (_, truncated) = read_bounded_log_line(&mut reader, &mut line).unwrap();
        assert!(truncated);
        assert_eq!(line.len(), MAX_LOG_LINE_BYTES);
        let serialized = serde_json::to_value(PluginLogLine {
            timestamp: 1,
            stream: "stdout".to_owned(),
            message: "event".to_owned(),
            service_id: "gateway".to_owned(),
            event: Some(json!({
                "kind": "request.started",
                "req_id": "req-1",
                "title": "请求开始"
            })),
        })
        .unwrap();
        assert_eq!(
            serialized.pointer("/event/req_id").and_then(Value::as_str),
            Some("req-1")
        );
    }

    #[test]
    fn resolves_provider_credentials_without_exposing_them_in_the_summary() {
        let workspace =
            std::env::temp_dir().join(format!("drpa-plugin-provider-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("dify2api");
        let manifest = load_manifest_from_root(&root, Some("dify2api")).unwrap();
        let mut state = read_plugin_state(&root, &manifest).unwrap();
        state.enabled = true;
        state.config["dify_api_key"] = json!("upstream-secret");
        write_plugin_state(&root, &state).unwrap();
        let provider = resolve_agent_provider(&workspace, "dify2api", "openai").unwrap();
        assert!(provider.base_url.ends_with("/v1"));
        assert_eq!(provider.model, "dify-agent");
        assert!(
            provider.api_key.len() >= 32,
            "the generated local proxy credential should be resolved inside Rust"
        );
        let plugins = list_plugins_inner(&workspace, &PluginManager::default()).unwrap();
        assert!(plugins[0].config.get("proxy_api_key").is_none());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn manager_keys_are_isolated_by_workspace() {
        let first = std::env::temp_dir().join(format!("drpa-plugin-scope-a-{}", Uuid::new_v4()));
        let second = std::env::temp_dir().join(format!("drpa-plugin-scope-b-{}", Uuid::new_v4()));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        assert_ne!(
            plugin_manager_key(&first, "same-plugin"),
            plugin_manager_key(&second, "same-plugin")
        );
        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }
}
