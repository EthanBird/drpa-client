use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::State;
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

const BUILTIN_DIFY_MANIFEST: &str =
    include_str!("../../../../plugins/builtin/dify-loves-hermes/plugin.yaml");
const BUILTIN_DIFY_CONFIG_SCHEMA: &str =
    include_str!("../../../../plugins/builtin/dify-loves-hermes/config.schema.json");
const BUILTIN_DIFY_SERVICE: &str =
    include_str!("../../../../plugins/builtin/dify-loves-hermes/service/dify_bridge.py");
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
    pub(crate) tools: Vec<PluginToolManifest>,
    #[serde(default)]
    pub(crate) default_config: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginServiceManifest {
    pub(crate) runtime: String,
    pub(crate) entry: String,
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
    config: Value,
    config_schema: Value,
    directory: String,
    last_error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginLogLine {
    timestamp: u64,
    stream: String,
    message: String,
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

struct RunningPlugin {
    child: Child,
    started_at: Instant,
    endpoint: String,
}

struct PluginManagerInner {
    processes: Mutex<HashMap<String, RunningPlugin>>,
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

impl Default for PluginManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(PluginManagerInner {
                processes: Mutex::new(HashMap::new()),
                logs: Mutex::new(HashMap::new()),
                last_errors: Mutex::new(HashMap::new()),
            }),
        }
    }
}

fn default_object_schema() -> Value {
    json!({"type":"object","properties":{},"additionalProperties":false})
}

const fn default_tool_timeout() -> u64 {
    30
}

#[tauri::command]
pub(crate) fn list_plugins(
    paths: State<'_, AppPaths>,
    manager: State<'_, PluginManager>,
) -> Result<Vec<PluginSummary>, String> {
    list_plugins_inner(&paths.workspace_root, &manager)
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
    config: Value,
    autostart: bool,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let (root, manifest) = load_plugin(&paths.workspace_root, &plugin_id)?;
    let mut state = read_plugin_state(&root, &manifest)?;
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
    state.enabled = enabled;
    write_plugin_state(&root, &state)?;
    if !enabled {
        stop_plugin_inner(&plugin_id, &manager)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn stop_plugin(
    plugin_id: String,
    manager: State<'_, PluginManager>,
) -> Result<(), String> {
    stop_plugin_inner(&plugin_id, &manager)
}

#[tauri::command]
pub(crate) fn uninstall_plugin(
    plugin_id: String,
    paths: State<'_, AppPaths>,
    manager: State<'_, PluginManager>,
) -> Result<(), String> {
    validate_identifier(&plugin_id, "插件")?;
    stop_plugin_inner(&plugin_id, &manager)?;
    let root = plugins_root(&paths.workspace_root).join(&plugin_id);
    if !root.is_dir() {
        return Err(format!("插件不存在：{plugin_id}"));
    }
    fs::remove_dir_all(root).map_err(|error| format!("卸载插件失败：{error}"))?;
    if plugin_id == "dify-loves-hermes" {
        fs::write(
            plugins_root(&paths.workspace_root).join(".removed-dify-loves-hermes"),
            b"1\n",
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn get_plugin_logs(
    plugin_id: String,
    manager: State<'_, PluginManager>,
) -> Result<Vec<PluginLogLine>, String> {
    validate_identifier(&plugin_id, "插件")?;
    let logs = manager
        .inner
        .logs
        .lock()
        .map_err(|_| "插件日志状态已损坏".to_owned())?;
    let Some(lines) = logs.get(&plugin_id) else {
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
        let (root, manifest) = load_plugin(&workspace_root, &plugin_id)?;
        let state = read_plugin_state(&root, &manifest)?;
        let service = manifest
            .service
            .ok_or_else(|| "该插件没有 Provider 服务".to_owned())?;
        if service.test_endpoint.trim().is_empty() {
            return Err("该插件没有声明连接测试入口".to_owned());
        }
        let endpoint = render_endpoint(&service.test_endpoint, &state.config);
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(35)))
            .build();
        let http = ureq::Agent::new_with_config(config);
        let mut response = http
            .get(&endpoint)
            .call()
            .map_err(|error| format!("插件连接测试失败：{error}"))?;
        response
            .body_mut()
            .read_json::<Value>()
            .map_err(|error| format!("插件连接测试返回无效 JSON：{error}"))
    })
    .await
    .map_err(|error| format!("插件连接测试后台任务失败：{error}"))?
}

#[tauri::command]
pub(crate) fn list_plugin_projects(
    paths: State<'_, AppPaths>,
) -> Result<Vec<PluginProjectSummary>, String> {
    list_plugin_projects_inner(&paths.workspace_root)
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
            _ => return Err("插件项目类型只支持 tool 或 service".to_owned()),
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
    let mut files = Vec::new();
    collect_plugin_project_files(&root, &root, &mut files)?;
    files.sort();
    for relative in files {
        archive
            .start_file(relative.replace('\\', "/"), options)
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
    python: &Path,
    manager: &PluginManager,
) -> Result<(), String> {
    let result = start_plugin_process(workspace_root, plugin_id, python, manager);
    if let Err(error) = &result
        && let Ok(mut errors) = manager.inner.last_errors.lock()
    {
        errors.insert(plugin_id.to_owned(), error.clone());
    }
    result
}

fn start_plugin_process(
    workspace_root: &Path,
    plugin_id: &str,
    python: &Path,
    manager: &PluginManager,
) -> Result<(), String> {
    let (root, manifest) = load_plugin(workspace_root, plugin_id)?;
    let service = manifest
        .service
        .as_ref()
        .ok_or_else(|| "该插件没有后台服务".to_owned())?;
    let mut state = read_plugin_state(&root, &manifest)?;
    state.enabled = true;
    write_plugin_state(&root, &state)?;

    {
        let mut processes = manager
            .inner
            .processes
            .lock()
            .map_err(|_| "插件进程状态已损坏".to_owned())?;
        if let Some(running) = processes.get_mut(plugin_id) {
            if running
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_none()
            {
                return Ok(());
            }
            processes.remove(plugin_id);
        }
    }

    let entry = resolve_plugin_path(&root, &service.entry, true)?;
    let mut command = match service.runtime.as_str() {
        "bundled-python" => {
            if !python.is_file() {
                return Err("插件需要已初始化的封装 Python 运行环境".to_owned());
            }
            let mut command = Command::new(python);
            command.arg("-I").arg(&entry);
            command
        }
        "executable" => Command::new(&entry),
        runtime => return Err(format!("插件服务 runtime 无效：{runtime}")),
    };
    let config_path = root.join("state.json");
    let endpoint = render_endpoint(&service.endpoint, &state.config);
    command
        .args(&service.args)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("DRPA_PLUGIN_ID", plugin_id)
        .env("DRPA_PLUGIN_ROOT", &root)
        .env("DRPA_PLUGIN_CONFIG", &config_path)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动插件 {plugin_id} 失败：{error}"))?;
    let log_buffer = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(stdout, "stdout", Arc::clone(&log_buffer));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(stderr, "stderr", Arc::clone(&log_buffer));
    }
    manager
        .inner
        .logs
        .lock()
        .map_err(|_| "插件日志状态已损坏".to_owned())?
        .insert(plugin_id.to_owned(), log_buffer);
    manager
        .inner
        .last_errors
        .lock()
        .map_err(|_| "插件错误状态已损坏".to_owned())?
        .remove(plugin_id);
    manager
        .inner
        .processes
        .lock()
        .map_err(|_| "插件进程状态已损坏".to_owned())?
        .insert(
            plugin_id.to_owned(),
            RunningPlugin {
                child,
                started_at: Instant::now(),
                endpoint,
            },
        );
    if !service.healthcheck.trim().is_empty() {
        let healthcheck = render_endpoint(&service.healthcheck, &state.config);
        if let Err(error) = wait_for_plugin_health(plugin_id, &healthcheck, manager) {
            let _ = stop_plugin_inner(plugin_id, manager);
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) fn start_autostart_plugins(
    workspace_root: &Path,
    python: &Path,
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
        if state.enabled && state.autostart && manifest.service.is_some() {
            let _ = start_plugin_inner(workspace_root, &id, python, manager);
        }
    }
    Ok(())
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
        for tool in manifest.tools {
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
    let (root, manifest) = load_plugin(workspace_root, plugin_id)?;
    let state = read_plugin_state(&root, &manifest)?;
    if !state.enabled {
        return Err(format!("插件未启用：{plugin_id}"));
    }
    let tool = manifest
        .tools
        .iter()
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
            let callable = entry_spec.map_or("run", |(_, callable)| callable);
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
            )?
        }
        "executable" => {
            run_json_process(Command::new(&entry), &tool.args, &root, &request, timeout)?
        }
        runtime => return Err(format!("插件工具 runtime 无效：{runtime}")),
    };
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
        let (root, manifest) = load_plugin(workspace_root, &id)?;
        let state = read_plugin_state(&root, &manifest)?;
        let running = manager
            .inner
            .processes
            .lock()
            .map_err(|_| "插件进程状态已损坏".to_owned())?;
        let (status, running_endpoint) = running
            .get(&id)
            .map(|process| ("running".to_owned(), process.endpoint.clone()))
            .unwrap_or_else(|| {
                (
                    if state.enabled { "stopped" } else { "disabled" }.to_owned(),
                    String::new(),
                )
            });
        drop(running);
        let last_error = manager
            .inner
            .last_errors
            .lock()
            .map_err(|_| "插件错误状态已损坏".to_owned())?
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let schema_path = root.join("config.schema.json");
        let config_schema = if schema_path.is_file() {
            serde_json::from_str(
                &fs::read_to_string(schema_path).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("插件 {id} 的 config.schema.json 无效：{error}"))?
        } else {
            json!({"type":"object","properties":{}})
        };
        summaries.push(PluginSummary {
            id,
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            types: manifest.types,
            enabled: state.enabled,
            autostart: state.autostart,
            status: if !last_error.is_empty() && status != "running" {
                "error".to_owned()
            } else {
                status
            },
            endpoint: if running_endpoint.is_empty() {
                manifest
                    .service
                    .as_ref()
                    .map(|service| render_endpoint(&service.endpoint, &state.config))
                    .unwrap_or_default()
            } else {
                running_endpoint
            },
            tool_count: manifest.tools.len(),
            config: state.config,
            config_schema,
            directory: root.to_string_lossy().into_owned(),
            last_error,
        });
    }
    summaries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(summaries)
}

fn refresh_processes(manager: &PluginManager) -> Result<(), String> {
    let mut exited = Vec::new();
    {
        let mut processes = manager
            .inner
            .processes
            .lock()
            .map_err(|_| "插件进程状态已损坏".to_owned())?;
        for (id, process) in processes.iter_mut() {
            if let Some(status) = process
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
            {
                exited.push((
                    id.clone(),
                    format!(
                        "插件进程已退出：{}，运行 {} ms",
                        status.code().unwrap_or(-1),
                        process.started_at.elapsed().as_millis()
                    ),
                ));
            }
        }
        for (id, _) in &exited {
            processes.remove(id);
        }
    }
    let mut errors = manager
        .inner
        .last_errors
        .lock()
        .map_err(|_| "插件错误状态已损坏".to_owned())?;
    for (id, error) in exited {
        errors.insert(id, error);
    }
    Ok(())
}

fn wait_for_plugin_health(
    plugin_id: &str,
    healthcheck: &str,
    manager: &PluginManager,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(600)))
        .build();
    let http = ureq::Agent::new_with_config(config);
    loop {
        {
            let mut processes = manager
                .inner
                .processes
                .lock()
                .map_err(|_| "插件进程状态已损坏".to_owned())?;
            let Some(process) = processes.get_mut(plugin_id) else {
                return Err(format!("插件 {plugin_id} 在健康检查前已停止"));
            };
            if let Some(status) = process
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
            {
                processes.remove(plugin_id);
                return Err(format!(
                    "插件 {plugin_id} 启动失败，进程退出码 {}",
                    status.code().unwrap_or(-1)
                ));
            }
        }
        if http.get(healthcheck).call().is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "插件 {plugin_id} 启动超时，健康检查未就绪：{healthcheck}"
            ));
        }
        thread::sleep(Duration::from_millis(120));
    }
}

fn stop_plugin_inner(plugin_id: &str, manager: &PluginManager) -> Result<(), String> {
    validate_identifier(plugin_id, "插件")?;
    let mut processes = manager
        .inner
        .processes
        .lock()
        .map_err(|_| "插件进程状态已损坏".to_owned())?;
    if let Some(mut process) = processes.remove(plugin_id) {
        let _ = process.child.kill();
        let _ = process.child.wait();
    }
    Ok(())
}

fn ensure_plugins_root(workspace_root: &Path) -> Result<(), String> {
    let root = plugins_root(workspace_root);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let builtin = root.join("dify-loves-hermes");
    if !root.join(".removed-dify-loves-hermes").is_file() && !builtin.join("plugin.yaml").is_file()
    {
        fs::create_dir_all(builtin.join("service")).map_err(|error| error.to_string())?;
        fs::write(builtin.join("plugin.yaml"), BUILTIN_DIFY_MANIFEST)
            .map_err(|error| error.to_string())?;
        fs::write(
            builtin.join("config.schema.json"),
            BUILTIN_DIFY_CONFIG_SCHEMA,
        )
        .map_err(|error| error.to_string())?;
        fs::write(builtin.join("service/dify_bridge.py"), BUILTIN_DIFY_SERVICE)
            .map_err(|error| error.to_string())?;
        fs::write(builtin.join(".builtin"), b"dify-loves-hermes\n")
            .map_err(|error| error.to_string())?;
    } else if builtin.join(".builtin").is_file() {
        write_if_different(&builtin.join("plugin.yaml"), BUILTIN_DIFY_MANIFEST)?;
        write_if_different(
            &builtin.join("config.schema.json"),
            BUILTIN_DIFY_CONFIG_SCHEMA,
        )?;
        write_if_different(
            &builtin.join("service/dify_bridge.py"),
            BUILTIN_DIFY_SERVICE,
        )?;
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
        load_manifest_from_root(&stage, Some(&manifest.id))?;
        fs::rename(&stage, &target).map_err(|error| error.to_string())?;
        if manifest.id == "dify-loves-hermes" {
            let _ =
                fs::remove_file(plugins_root(workspace_root).join(".removed-dify-loves-hermes"));
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
    if let Some(service) = &manifest.service {
        if !matches!(service.runtime.as_str(), "bundled-python" | "executable") {
            return Err("插件 service.runtime 只支持 bundled-python 或 executable".to_owned());
        }
        if !matches!(service.transport.as_str(), "http" | "") {
            return Err("当前插件服务 transport 只支持 http".to_owned());
        }
        validate_relative_plugin_path(&service.entry, "插件 service.entry")?;
        if !service.healthcheck.is_empty()
            && !(service.healthcheck.starts_with("http://")
                || service.healthcheck.starts_with("https://"))
        {
            return Err("插件 service.healthcheck 必须是 http(s) 地址".to_owned());
        }
        if !service.test_endpoint.is_empty()
            && !(service.test_endpoint.starts_with("http://")
                || service.test_endpoint.starts_with("https://"))
        {
            return Err("插件 service.test_endpoint 必须是 http(s) 地址".to_owned());
        }
    }
    let mut tool_names = HashSet::new();
    for tool in &manifest.tools {
        validate_tool_name(&tool.name)?;
        if !tool_names.insert(tool.name.as_str()) {
            return Err(format!("插件工具名称重复：{}", tool.name));
        }
        if qualified_plugin_tool_name(&manifest.id, &tool.name).len() > 64 {
            return Err(format!(
                "插件工具 {} 的完整注册名称超过 64 个字符",
                tool.name
            ));
        }
        if !matches!(tool.runtime.as_str(), "bundled-python" | "executable") {
            return Err(format!("插件工具 {} 的 runtime 无效", tool.name));
        }
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
    Ok(manifest)
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
    let target = root.join("state.json");
    if target.exists() {
        fs::remove_file(&target).map_err(|error| error.to_string())?;
    }
    fs::rename(temporary, target).map_err(|error| error.to_string())
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

fn render_endpoint(template: &str, config: &Value) -> String {
    let port = config.get("port").and_then(Value::as_u64).unwrap_or(34121);
    template.replace("{port}", &port.to_string())
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
    target: Arc<Mutex<VecDeque<PluginLogLine>>>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            let Ok(read) = reader.read_until(b'\n', &mut bytes) else {
                break;
            };
            if read == 0 {
                break;
            }
            let message = String::from_utf8_lossy(&bytes)
                .trim_end_matches(['\r', '\n'])
                .to_owned();
            if let Ok(mut lines) = target.lock() {
                lines.push_back(PluginLogLine {
                    timestamp: unix_millis(),
                    stream: stream.to_owned(),
                    message,
                });
                while lines.len() > MAX_LOG_LINES {
                    lines.pop_front();
                }
            }
        }
    });
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
    if let Some(service) = &manifest.service {
        resolve_plugin_path(root, &service.entry, true)?;
    }
    for tool in &manifest.tools {
        let entry = tool
            .entry
            .split_once(':')
            .map_or(tool.entry.as_str(), |(path, _)| path);
        resolve_plugin_path(root, entry, true)?;
    }
    let schema = root.join("config.schema.json");
    if schema.is_file() {
        serde_json::from_str::<Value>(
            &fs::read_to_string(schema).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("config.schema.json 无效：{error}"))?;
    }
    Ok(manifest)
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
        if kind.is_symlink() || name == "__pycache__" || name == "state.json" || name == ".builtin"
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
    fn seeds_and_lists_the_builtin_dify_bridge() {
        let workspace = std::env::temp_dir().join(format!("drpa-plugins-{}", Uuid::new_v4()));
        let manager = PluginManager::default();
        let plugins = list_plugins_inner(&workspace, &manager).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "dify-loves-hermes");
        assert_eq!(plugins[0].status, "disabled");
        assert!(plugins[0].types.contains(&"provider-adapter".to_owned()));
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn plugin_manifest_rejects_parent_paths_and_invalid_tools() {
        let workspace = std::env::temp_dir().join(format!("drpa-plugins-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("dify-loves-hermes");
        assert!(resolve_plugin_path(&root, "../outside.py", false).is_err());
        let invalid = "schema: 1\nid: bad\nname: Bad\nversion: 1\ndescription: Bad\ntools:\n  - name: Bad Tool\n    description: bad\n    runtime: executable\n    entry: bad.exe\n";
        assert!(parse_plugin_manifest(invalid, None).is_err());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn starts_builtin_service_and_waits_for_its_healthcheck() {
        let Some(python) = find_test_python() else {
            return;
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let workspace = std::env::temp_dir().join(format!("drpa-plugin-start-{}", Uuid::new_v4()));
        ensure_plugins_root(&workspace).unwrap();
        let root = plugins_root(&workspace).join("dify-loves-hermes");
        write_plugin_state(
            &root,
            &PluginStateFile {
                enabled: true,
                autostart: false,
                config: json!({"port": port, "model": "dify-health-test"}),
            },
        )
        .unwrap();
        let manager = PluginManager::default();

        start_plugin_inner(&workspace, "dify-loves-hermes", &python, &manager).unwrap();
        let plugins = list_plugins_inner(&workspace, &manager).unwrap();
        assert_eq!(plugins[0].status, "running");
        assert_eq!(plugins[0].endpoint, format!("http://127.0.0.1:{port}/v1"));

        stop_plugin_inner("dify-loves-hermes", &manager).unwrap();
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

    fn find_test_python() -> Option<PathBuf> {
        let local = if cfg!(windows) {
            PathBuf::from(".venv/Scripts/python.exe")
        } else {
            PathBuf::from(".venv/bin/python")
        };
        if local.is_file() {
            return fs::canonicalize(local).ok();
        }
        let candidates: &[&str] = if cfg!(windows) {
            &["python.exe", "python"]
        } else {
            &["python3", "python"]
        };
        candidates.iter().find_map(|candidate| {
            let output = Command::new(candidate)
                .args(["-c", "import sys; print(sys.executable)"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            path.is_file().then_some(path)
        })
    }
}
