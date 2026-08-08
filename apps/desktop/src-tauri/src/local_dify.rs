use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use drpa_protocol::RUNTIME_PROTOCOL_VERSION;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::local_dify_workflow::{
    self, WorkflowGraph, WorkflowNode, WorkflowValidationReport, default_graph, graph_from_dify,
    graph_to_dify, is_workflow_mode, normalize_graph, validate_graph,
};
use crate::{
    AppPaths, installed_package_catalog, knowledge_base, locate_runtime, locate_runtime_python,
};

const LOCAL_DIFY_SCHEMA: u32 = 2;
const MAX_APPS: usize = 200;
const MAX_PROVIDERS: usize = 50;
const MAX_INPUT_BYTES: usize = 1_000_000;
const MAX_DSL_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PROVIDER_HOPS: usize = 4;
const DEFAULT_SERVICE_PORT: u16 = 34_130;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub temperature: f32,
    pub streaming: bool,
    pub supports_tools: bool,
    pub supports_json: bool,
    pub supports_vision: bool,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub dify_provider: String,
    #[serde(default)]
    pub dify_model: String,
    #[serde(default)]
    pub has_api_key: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyProviderInput {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_true")]
    pub streaming: bool,
    #[serde(default)]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_json: bool,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub dify_provider: String,
    #[serde(default)]
    pub dify_model: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyApp {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub mode: String,
    pub provider_id: String,
    pub system_prompt: String,
    pub opening_statement: String,
    pub input_key: String,
    pub temperature: f32,
    pub max_output_tokens: u32,
    #[serde(default)]
    pub workflow: WorkflowGraph,
    pub published_version: u32,
    pub api_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateLocalDifyAppInput {
    pub name: String,
    #[serde(default = "default_app_mode")]
    pub mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyRunRequest {
    pub request_id: String,
    pub app_id: String,
    pub query: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default = "default_local_user")]
    pub user: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub conversation_id: String,
    #[serde(default)]
    pub provider_route: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyRunResult {
    pub run_id: String,
    pub app_id: String,
    pub answer: String,
    pub conversation_id: String,
    pub provider_id: String,
    pub model: String,
    pub usage: LocalDifyUsage,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum LocalDifyStreamEvent {
    Started {
        run_id: String,
    },
    NodeStarted {
        run_id: String,
        node_id: String,
        node_type: String,
        title: String,
    },
    NodeCompleted {
        run_id: String,
        node_id: String,
        outputs: Value,
        duration_ms: u64,
    },
    NodeFailed {
        run_id: String,
        node_id: String,
        error: String,
        duration_ms: u64,
    },
    Delta {
        content: String,
    },
    Completed {
        run_id: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyRunSummary {
    pub id: String,
    pub app_id: String,
    pub app_name: String,
    pub status: String,
    pub query: String,
    pub answer: String,
    pub provider_id: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub duration_ms: u64,
    pub error: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyProviderTest {
    pub ok: bool,
    pub message: String,
    pub model: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DifyCompatibilityIssue {
    pub level: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DifyCompatibilityReport {
    pub compatible: bool,
    pub target_version: String,
    pub issues: Vec<DifyCompatibilityIssue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalDifyServiceStatus {
    pub running: bool,
    pub port: u16,
    pub endpoint: String,
    pub started_at: Option<i64>,
    pub last_error: String,
}

struct ServiceControl {
    stop: Option<Arc<AtomicBool>>,
    running: bool,
    port: u16,
    started_at: Option<i64>,
    last_error: String,
}

impl Default for ServiceControl {
    fn default() -> Self {
        Self {
            stop: None,
            running: false,
            port: DEFAULT_SERVICE_PORT,
            started_at: None,
            last_error: String::new(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct LocalDifyServiceManager {
    state: Arc<Mutex<ServiceControl>>,
}

#[derive(Default, Deserialize, Serialize)]
struct LocalDifySecrets {
    #[serde(default)]
    provider_api_keys: HashMap<String, String>,
    #[serde(default)]
    app_api_tokens: HashMap<String, String>,
}

#[derive(Debug)]
struct ProviderCompletion {
    answer: String,
    usage: LocalDifyUsage,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

const fn default_context_window() -> u32 {
    393_216
}

const fn default_max_output_tokens() -> u32 {
    98_304
}

const fn default_temperature() -> f32 {
    0.2
}

const fn default_timeout_seconds() -> u64 {
    120
}

const fn default_true() -> bool {
    true
}

fn default_app_mode() -> String {
    "chat".to_owned()
}

fn default_local_user() -> String {
    "local-developer".to_owned()
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn local_dify_root(paths: &AppPaths) -> PathBuf {
    paths.workspace_root.join("local-dify")
}

fn apps_root(paths: &AppPaths) -> PathBuf {
    local_dify_root(paths).join("apps")
}

fn providers_path(paths: &AppPaths) -> PathBuf {
    local_dify_root(paths).join("providers.json")
}

fn secrets_path(paths: &AppPaths) -> PathBuf {
    local_dify_root(paths).join("secrets.json")
}

fn runtime_database_path(paths: &AppPaths) -> PathBuf {
    local_dify_root(paths).join("runtime.sqlite3")
}

fn app_path(paths: &AppPaths, app_id: &str) -> PathBuf {
    apps_root(paths).join(app_id).join("app.json")
}

fn source_dsl_path(paths: &AppPaths, app_id: &str) -> PathBuf {
    apps_root(paths).join(app_id).join("source.dify.yml")
}

fn ensure_root(paths: &AppPaths) -> Result<(), String> {
    fs::create_dir_all(apps_root(paths))
        .map_err(|error| format!("创建 Local Dify 目录失败：{error}"))?;
    if !providers_path(paths).exists() {
        let default_provider = LocalDifyProvider {
            id: "provider-openai-compatible".to_owned(),
            name: "OpenAI Compatible".to_owned(),
            base_url: "http://127.0.0.1/v1".to_owned(),
            model: "deepseek-v4-flash".to_owned(),
            context_window: default_context_window(),
            max_output_tokens: default_max_output_tokens(),
            temperature: default_temperature(),
            streaming: true,
            supports_tools: true,
            supports_json: true,
            supports_vision: false,
            timeout_seconds: default_timeout_seconds(),
            custom_headers: BTreeMap::new(),
            dify_provider: "langgenius/openai/openai".to_owned(),
            dify_model: "deepseek-v4-flash".to_owned(),
            has_api_key: false,
            updated_at: now_timestamp(),
        };
        write_json_atomic(&providers_path(paths), &vec![default_provider])?;
    }
    let _ = open_runtime_database(paths)?;
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("{label}标识无效"));
    }
    Ok(())
}

fn validate_app(app: &LocalDifyApp) -> Result<(), String> {
    validate_identifier(&app.id, "流程")?;
    if app.name.trim().is_empty() || app.name.chars().count() > 100 {
        return Err("流程名称应为 1 到 100 个字符".to_owned());
    }
    if !matches!(
        app.mode.as_str(),
        "chat" | "completion" | "advanced-chat" | "workflow"
    ) {
        return Err("应用模式无效".to_owned());
    }
    if !app.provider_id.is_empty() {
        validate_identifier(&app.provider_id, "Provider")?;
    }
    if app.system_prompt.len() > MAX_INPUT_BYTES || app.description.len() > 10_000 {
        return Err("应用配置内容过长".to_owned());
    }
    if app.input_key.trim().is_empty() || app.input_key.len() > 100 {
        return Err("输入变量名称无效".to_owned());
    }
    if !app.temperature.is_finite() || !(0.0..=2.0).contains(&app.temperature) {
        return Err("Temperature 必须位于 0 到 2 之间".to_owned());
    }
    if !(64..=131_072).contains(&app.max_output_tokens) {
        return Err("最大输出 tokens 必须位于 64 到 131072 之间".to_owned());
    }
    Ok(())
}

fn validate_provider(input: &LocalDifyProviderInput) -> Result<(), String> {
    if !input.id.is_empty() {
        validate_identifier(&input.id, "Provider")?;
    }
    if input.name.trim().is_empty() || input.name.chars().count() > 100 {
        return Err("Provider 名称应为 1 到 100 个字符".to_owned());
    }
    crate::provider::chat_completions_endpoint(&input.base_url)?;
    if input.model.trim().is_empty() || input.model.len() > 200 {
        return Err("模型名称无效".to_owned());
    }
    if !(1_024..=2_000_000).contains(&input.context_window) {
        return Err("上下文窗口必须位于 1024 到 2000000 tokens 之间".to_owned());
    }
    if !(64..=131_072).contains(&input.max_output_tokens)
        || input.max_output_tokens >= input.context_window
    {
        return Err("最大输出 tokens 必须小于上下文窗口".to_owned());
    }
    if !(5..=600).contains(&input.timeout_seconds) {
        return Err("请求超时必须位于 5 到 600 秒之间".to_owned());
    }
    for (name, value) in &input.custom_headers {
        if name.trim().is_empty()
            || name.len() > 100
            || value.len() > 4_096
            || name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("x-drpa-provider-route")
            || name.eq_ignore_ascii_case("x-drpa-hop-count")
        {
            return Err(format!("自定义 Header 无效：{name}"));
        }
    }
    Ok(())
}

fn read_json_or_default<T>(path: &Path) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Default,
{
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("读取 {} 失败：{error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(format!("读取 {} 失败：{error}", path.display())),
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "配置路径无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败：{error}"))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4().simple()
    ));
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs::write(&temporary, bytes).map_err(|error| format!("写入临时配置失败：{error}"))?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("替换旧配置失败：{error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("提交配置失败：{error}"))
}

fn load_providers(paths: &AppPaths) -> Result<Vec<LocalDifyProvider>, String> {
    let mut providers: Vec<LocalDifyProvider> = read_json_or_default(&providers_path(paths))?;
    let secrets: LocalDifySecrets = read_json_or_default(&secrets_path(paths))?;
    for provider in &mut providers {
        provider.has_api_key = secrets.provider_api_keys.contains_key(&provider.id);
    }
    providers.sort_by_key(|provider| provider.name.to_lowercase());
    Ok(providers)
}

fn load_provider(
    paths: &AppPaths,
    provider_id: &str,
) -> Result<(LocalDifyProvider, String), String> {
    validate_identifier(provider_id, "Provider")?;
    let provider = load_providers(paths)?
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| "Provider 不存在".to_owned())?;
    let secrets: LocalDifySecrets = read_json_or_default(&secrets_path(paths))?;
    let api_key = secrets
        .provider_api_keys
        .get(provider_id)
        .cloned()
        .unwrap_or_default();
    Ok((provider, api_key))
}

fn load_app(paths: &AppPaths, app_id: &str) -> Result<LocalDifyApp, String> {
    validate_identifier(app_id, "流程")?;
    let bytes =
        fs::read(app_path(paths, app_id)).map_err(|error| format!("读取流程失败：{error}"))?;
    let app: LocalDifyApp =
        serde_json::from_slice(&bytes).map_err(|error| format!("流程配置无效：{error}"))?;
    Ok(normalize_app(app))
}

fn normalize_app(mut app: LocalDifyApp) -> LocalDifyApp {
    if is_workflow_mode(&app.mode) && app.workflow.nodes.is_empty() {
        app.workflow = default_graph(&app.mode, &app.input_key);
    } else {
        normalize_graph(&mut app.workflow);
    }
    app
}

fn open_runtime_database(paths: &AppPaths) -> Result<Connection, String> {
    let path = runtime_database_path(paths);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建运行数据库目录失败：{error}"))?;
    }
    let connection =
        Connection::open(path).map_err(|error| format!("打开 Local Dify 数据库失败：{error}"))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS local_dify_runs (
               id TEXT PRIMARY KEY,
               app_id TEXT NOT NULL,
               app_name TEXT NOT NULL,
               status TEXT NOT NULL,
               query TEXT NOT NULL,
               answer TEXT NOT NULL DEFAULT '',
               provider_id TEXT NOT NULL DEFAULT '',
               model TEXT NOT NULL DEFAULT '',
               prompt_tokens INTEGER NOT NULL DEFAULT 0,
               completion_tokens INTEGER NOT NULL DEFAULT 0,
               duration_ms INTEGER NOT NULL DEFAULT 0,
               error TEXT NOT NULL DEFAULT '',
               created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_local_dify_runs_app_created
               ON local_dify_runs(app_id, created_at DESC);",
        )
        .map_err(|error| format!("初始化 Local Dify 数据库失败：{error}"))?;
    Ok(connection)
}

#[tauri::command]
pub(crate) fn list_local_dify_apps(
    paths: State<'_, AppPaths>,
) -> Result<Vec<LocalDifyApp>, String> {
    ensure_root(&paths)?;
    let mut apps = Vec::new();
    for entry in fs::read_dir(apps_root(&paths))
        .map_err(|error| error.to_string())?
        .flatten()
    {
        if !entry.path().is_dir() {
            continue;
        }
        let path = entry.path().join("app.json");
        if let Ok(bytes) = fs::read(path)
            && let Ok(app) = serde_json::from_slice::<LocalDifyApp>(&bytes)
        {
            apps.push(normalize_app(app));
        }
    }
    apps.sort_by_key(|app| std::cmp::Reverse(app.updated_at));
    Ok(apps)
}

#[tauri::command]
pub(crate) fn create_local_dify_app(
    input: CreateLocalDifyAppInput,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyApp, String> {
    ensure_root(&paths)?;
    if list_local_dify_apps(paths.clone())?.len() >= MAX_APPS {
        return Err(format!("本地流程最多保留 {MAX_APPS} 个"));
    }
    let mode = input.mode.trim().to_owned();
    let now = now_timestamp();
    let provider_id = load_providers(&paths)?
        .first()
        .map(|item| item.id.clone())
        .unwrap_or_default();
    let workflow = default_graph(&mode, "query");
    let app = LocalDifyApp {
        schema: LOCAL_DIFY_SCHEMA,
        id: format!("app-{}", Uuid::new_v4().simple()),
        name: input.name.trim().to_owned(),
        description: "用于本地测试与 Dify DSL 导出的流程。".to_owned(),
        mode,
        provider_id,
        system_prompt: "你是一个准确、简洁的 AI 助手。".to_owned(),
        opening_statement: "你好，我是本地流程助手。".to_owned(),
        input_key: "query".to_owned(),
        temperature: default_temperature(),
        max_output_tokens: default_max_output_tokens(),
        workflow,
        published_version: 0,
        api_enabled: false,
        created_at: now,
        updated_at: now,
    };
    validate_app(&app)?;
    write_json_atomic(&app_path(&paths, &app.id), &app)?;
    Ok(app)
}

#[tauri::command]
pub(crate) fn save_local_dify_app(
    mut app: LocalDifyApp,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyApp, String> {
    ensure_root(&paths)?;
    let existing = load_app(&paths, &app.id)?;
    app.schema = LOCAL_DIFY_SCHEMA;
    app.created_at = existing.created_at;
    app.updated_at = now_timestamp();
    if is_workflow_mode(&app.mode) && app.workflow.nodes.is_empty() {
        app.workflow = default_graph(&app.mode, &app.input_key);
    }
    normalize_graph(&mut app.workflow);
    validate_app(&app)?;
    if is_workflow_mode(&app.mode) {
        let report = validate_graph(&app.workflow, &app.mode);
        if !report.valid {
            let summary = report
                .issues
                .iter()
                .filter(|issue| issue.level == "error")
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join("；");
            return Err(format!("工作流校验失败：{summary}"));
        }
    }
    if !app.provider_id.is_empty() {
        let _ = load_provider(&paths, &app.provider_id)?;
    }
    write_json_atomic(&app_path(&paths, &app.id), &app)?;
    Ok(app)
}

#[tauri::command]
pub(crate) fn validate_local_dify_workflow(
    mut graph: WorkflowGraph,
    mode: String,
) -> Result<WorkflowValidationReport, String> {
    if !is_workflow_mode(&mode) {
        return Err("只有 Workflow 或 Chatflow 应用可以校验工作流".to_owned());
    }
    normalize_graph(&mut graph);
    Ok(validate_graph(&graph, &mode))
}

#[tauri::command]
pub(crate) fn create_local_dify_workflow_node(
    kind: String,
    x: f64,
    y: f64,
) -> Result<WorkflowNode, String> {
    if !matches!(
        kind.as_str(),
        "llm"
            | "template-transform"
            | "if-else"
            | "http-request"
            | "code"
            | "rpaz-package"
            | "question-classifier"
            | "parameter-extractor"
            | "variable-aggregator"
            | "list-operator"
            | "document-extractor"
            | "knowledge-retrieval"
            | "answer"
            | "end"
    ) {
        return Err(format!("工作流节点类型无效：{kind}"));
    }
    if !x.is_finite() || !y.is_finite() {
        return Err("工作流节点坐标无效".to_owned());
    }
    Ok(local_dify_workflow::new_node(&kind, x, y))
}

#[tauri::command]
pub(crate) fn delete_local_dify_app(
    app_id: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_identifier(&app_id, "流程")?;
    let root = apps_root(&paths).join(&app_id);
    if !root.is_dir() {
        return Err("流程不存在".to_owned());
    }
    fs::remove_dir_all(root).map_err(|error| format!("删除流程失败：{error}"))?;
    let mut secrets: LocalDifySecrets = read_json_or_default(&secrets_path(&paths))?;
    secrets.app_api_tokens.remove(&app_id);
    write_json_atomic(&secrets_path(&paths), &secrets)
}

#[tauri::command]
pub(crate) fn list_local_dify_providers(
    paths: State<'_, AppPaths>,
) -> Result<Vec<LocalDifyProvider>, String> {
    ensure_root(&paths)?;
    load_providers(&paths)
}

#[tauri::command]
pub(crate) fn save_local_dify_provider(
    input: LocalDifyProviderInput,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyProvider, String> {
    ensure_root(&paths)?;
    validate_provider(&input)?;
    let mut providers = load_providers(&paths)?;
    if input.id.is_empty() && providers.len() >= MAX_PROVIDERS {
        return Err(format!("Provider 最多保留 {MAX_PROVIDERS} 个"));
    }
    let id = if input.id.is_empty() {
        format!("provider-{}", Uuid::new_v4().simple())
    } else {
        input.id.clone()
    };
    let mut secrets: LocalDifySecrets = read_json_or_default(&secrets_path(&paths))?;
    if !input.api_key.is_empty() {
        secrets
            .provider_api_keys
            .insert(id.clone(), input.api_key.clone());
    }
    let provider = LocalDifyProvider {
        id: id.clone(),
        name: input.name.trim().to_owned(),
        base_url: input.base_url.trim().trim_end_matches('/').to_owned(),
        model: input.model.trim().to_owned(),
        context_window: input.context_window,
        max_output_tokens: input.max_output_tokens,
        temperature: input.temperature,
        streaming: input.streaming,
        supports_tools: input.supports_tools,
        supports_json: input.supports_json,
        supports_vision: input.supports_vision,
        timeout_seconds: input.timeout_seconds,
        custom_headers: input.custom_headers,
        // Dify export mapping is an implementation detail. The desktop UI only
        // asks for the direct OpenAI-compatible URL/model/key connection.
        dify_provider: if input.dify_provider.trim().is_empty() {
            "langgenius/openai/openai".to_owned()
        } else {
            input.dify_provider.trim().to_owned()
        },
        dify_model: if input.dify_model.trim().is_empty() {
            input.model.trim().to_owned()
        } else {
            input.dify_model.trim().to_owned()
        },
        has_api_key: secrets.provider_api_keys.contains_key(&id),
        updated_at: now_timestamp(),
    };
    if let Some(existing) = providers.iter_mut().find(|item| item.id == id) {
        *existing = provider.clone();
    } else {
        providers.push(provider.clone());
    }
    write_json_atomic(&providers_path(&paths), &providers)?;
    write_json_atomic(&secrets_path(&paths), &secrets)?;
    Ok(provider)
}

#[tauri::command]
pub(crate) fn delete_local_dify_provider(
    provider_id: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_identifier(&provider_id, "Provider")?;
    if list_local_dify_apps(paths.clone())?
        .iter()
        .any(|app| app.provider_id == provider_id)
    {
        return Err("该 Provider 正被流程使用".to_owned());
    }
    let mut providers = load_providers(&paths)?;
    let previous = providers.len();
    providers.retain(|provider| provider.id != provider_id);
    if previous == providers.len() {
        return Err("Provider 不存在".to_owned());
    }
    let mut secrets: LocalDifySecrets = read_json_or_default(&secrets_path(&paths))?;
    secrets.provider_api_keys.remove(&provider_id);
    write_json_atomic(&providers_path(&paths), &providers)?;
    write_json_atomic(&secrets_path(&paths), &secrets)
}

#[tauri::command]
pub(crate) async fn test_local_dify_provider(
    provider_id: String,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyProviderTest, String> {
    let paths = paths.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (provider, api_key) = load_provider(&paths, &provider_id)?;
        let started = Instant::now();
        let payload = json!({
            "model": provider.model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "temperature": 0,
            "max_tokens": 8,
            "stream": false,
        });
        let completion = call_provider(&provider, &api_key, &payload, &[], |_| {})?;
        Ok(LocalDifyProviderTest {
            ok: true,
            message: format!("Provider 连接成功：{}", completion.answer.trim()),
            model: provider.model,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    })
    .await
    .map_err(|error| format!("Provider 测试任务异常：{error}"))?
}

fn run_stream_event_name(request_id: &str) -> Result<String, String> {
    validate_identifier(request_id, "请求")?;
    Ok(format!("local-dify-stream-{request_id}"))
}

#[tauri::command]
pub(crate) async fn run_local_dify_app(
    request: LocalDifyRunRequest,
    app_handle: AppHandle,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyRunResult, String> {
    let event_name = run_stream_event_name(&request.request_id)?;
    let paths = paths.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_app_internal(&paths, request, |event| {
            let _ = app_handle.emit(&event_name, event);
        })
    })
    .await
    .map_err(|error| format!("流程执行任务异常：{error}"))?
}

fn validate_run_request(request: &LocalDifyRunRequest) -> Result<(), String> {
    run_stream_event_name(&request.request_id)?;
    validate_identifier(&request.app_id, "流程")?;
    if request.query.trim().is_empty() || request.query.len() > MAX_INPUT_BYTES {
        return Err("调试输入应为 1 到 1000000 字节".to_owned());
    }
    if request.inputs.len() > 100 {
        return Err("输入变量数量超过 100 个".to_owned());
    }
    if request.provider_route.len() >= MAX_PROVIDER_HOPS
        || request
            .provider_route
            .iter()
            .any(|item| item == &request.app_id)
    {
        return Err("Provider 调用链出现循环或超过最大跳数".to_owned());
    }
    Ok(())
}

fn workflow_requires_provider(app: &LocalDifyApp) -> bool {
    !is_workflow_mode(&app.mode)
        || app.workflow.nodes.iter().any(|node| {
            matches!(
                node.kind.as_str(),
                "llm" | "question-classifier" | "parameter-extractor"
            )
        })
}

fn local_only_workflow_provider() -> LocalDifyProvider {
    LocalDifyProvider {
        id: "local-workflow".to_owned(),
        name: "本地工作流".to_owned(),
        base_url: String::new(),
        model: "local-nodes".to_owned(),
        context_window: 0,
        max_output_tokens: 0,
        temperature: 0.0,
        streaming: false,
        supports_tools: false,
        supports_json: false,
        supports_vision: false,
        timeout_seconds: 0,
        custom_headers: BTreeMap::new(),
        dify_provider: "drpa/local".to_owned(),
        dify_model: "local-nodes".to_owned(),
        has_api_key: false,
        updated_at: now_timestamp(),
    }
}

fn run_app_internal<F>(
    paths: &AppPaths,
    request: LocalDifyRunRequest,
    mut emit: F,
) -> Result<LocalDifyRunResult, String>
where
    F: FnMut(LocalDifyStreamEvent),
{
    validate_run_request(&request)?;
    let app = load_app(paths, &request.app_id)?;
    if !matches!(
        app.mode.as_str(),
        "chat" | "completion" | "workflow" | "advanced-chat"
    ) {
        return Err("当前应用模式不受本地执行器支持".to_owned());
    }
    let (provider, api_key) = if app.provider_id.is_empty() {
        if workflow_requires_provider(&app) {
            return Err("当前流程包含模型节点，请先选择 Provider".to_owned());
        }
        (local_only_workflow_provider(), String::new())
    } else {
        load_provider(paths, &app.provider_id)?
    };
    let run_id = format!("dify-run-{}", Uuid::new_v4().simple());
    emit(LocalDifyStreamEvent::Started {
        run_id: run_id.clone(),
    });
    let started = Instant::now();
    let query = request.query.trim().to_owned();
    let mut route = request.provider_route.clone();
    route.push(app.id.clone());
    let completed = if is_workflow_mode(&app.mode) {
        run_workflow_internal(
            paths, &app, &provider, &api_key, &request, &run_id, &route, &mut emit,
        )
    } else {
        let mut messages = Vec::new();
        if !app.system_prompt.trim().is_empty() {
            messages.push(json!({"role": "system", "content": app.system_prompt}));
        }
        messages.push(json!({"role": "user", "content": query}));
        let stream = request.stream && provider.streaming;
        let payload = json!({
            "model": provider.model,
            "messages": messages,
            "temperature": app.temperature,
            "max_tokens": app.max_output_tokens.min(provider.max_output_tokens),
            "stream": stream,
            "stream_options": if stream { json!({"include_usage": true}) } else { Value::Null },
            "user": request.user,
        });
        call_provider(&provider, &api_key, &payload, &route, |content| {
            emit(LocalDifyStreamEvent::Delta { content });
        })
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    match completed {
        Ok(completion) => {
            record_run(
                paths,
                &run_id,
                &app,
                "success",
                &query,
                &completion.answer,
                &provider,
                &completion.usage,
                duration_ms,
                "",
            )?;
            emit(LocalDifyStreamEvent::Completed {
                run_id: run_id.clone(),
            });
            Ok(LocalDifyRunResult {
                run_id,
                app_id: app.id.clone(),
                answer: completion.answer,
                conversation_id: if request.conversation_id.is_empty() {
                    format!("conversation-{}", Uuid::new_v4().simple())
                } else {
                    request.conversation_id
                },
                provider_id: provider.id,
                model: provider.model,
                usage: completion.usage,
                duration_ms,
            })
        }
        Err(error) => {
            let _ = record_run(
                paths,
                &run_id,
                &app,
                "failed",
                &query,
                "",
                &provider,
                &LocalDifyUsage::default(),
                duration_ms,
                &error,
            );
            Err(error)
        }
    }
}

struct WorkflowNodeResult {
    outputs: BTreeMap<String, Value>,
    branch: Option<String>,
    answer: Option<String>,
    usage: LocalDifyUsage,
}

type WorkflowOutputs = HashMap<String, BTreeMap<String, Value>>;

#[allow(clippy::too_many_arguments)]
fn run_workflow_internal<F>(
    paths: &AppPaths,
    app: &LocalDifyApp,
    provider: &LocalDifyProvider,
    api_key: &str,
    request: &LocalDifyRunRequest,
    run_id: &str,
    route: &[String],
    emit: &mut F,
) -> Result<ProviderCompletion, String>
where
    F: FnMut(LocalDifyStreamEvent),
{
    let report = validate_graph(&app.workflow, &app.mode);
    if !report.valid {
        let summary = report
            .issues
            .iter()
            .filter(|issue| issue.level == "error")
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("；");
        return Err(format!("工作流校验失败：{summary}"));
    }
    let nodes: HashMap<&str, &WorkflowNode> = app
        .workflow
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut outgoing: HashMap<&str, Vec<&local_dify_workflow::WorkflowEdge>> = HashMap::new();
    for edge in &app.workflow.edges {
        outgoing.entry(edge.source.as_str()).or_default().push(edge);
    }
    let start = app
        .workflow
        .nodes
        .iter()
        .find(|node| node.kind == "start")
        .ok_or_else(|| "工作流缺少开始节点".to_owned())?;
    let mut queue = VecDeque::from([start.id.clone()]);
    let mut visited = HashSet::new();
    let mut outputs = WorkflowOutputs::new();
    let mut total_usage = LocalDifyUsage::default();
    let mut final_answer = String::new();
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.clone()) {
            continue;
        }
        let node = nodes
            .get(node_id.as_str())
            .copied()
            .ok_or_else(|| format!("工作流节点不存在：{node_id}"))?;
        emit(LocalDifyStreamEvent::NodeStarted {
            run_id: run_id.to_owned(),
            node_id: node.id.clone(),
            node_type: node.kind.clone(),
            title: node.title.clone(),
        });
        let started = Instant::now();
        let executed = execute_workflow_node(
            paths, app, provider, api_key, request, run_id, route, node, &outputs, emit,
        );
        let node_duration = started.elapsed().as_millis() as u64;
        let executed = match executed {
            Ok(executed) => executed,
            Err(error) => {
                emit(LocalDifyStreamEvent::NodeFailed {
                    run_id: run_id.to_owned(),
                    node_id: node.id.clone(),
                    error: error.clone(),
                    duration_ms: node_duration,
                });
                return Err(format!("节点“{}”执行失败：{error}", node.title));
            }
        };
        total_usage.prompt_tokens = total_usage
            .prompt_tokens
            .saturating_add(executed.usage.prompt_tokens);
        total_usage.completion_tokens = total_usage
            .completion_tokens
            .saturating_add(executed.usage.completion_tokens);
        total_usage.total_tokens = total_usage
            .total_tokens
            .saturating_add(executed.usage.total_tokens);
        if let Some(answer) = executed.answer.as_ref() {
            final_answer = answer.clone();
        }
        emit(LocalDifyStreamEvent::NodeCompleted {
            run_id: run_id.to_owned(),
            node_id: node.id.clone(),
            outputs: json!(executed.outputs),
            duration_ms: node_duration,
        });
        outputs.insert(node.id.clone(), executed.outputs);
        if let Some(edges) = outgoing.get(node.id.as_str()) {
            let selected: Vec<_> = if let Some(branch) = executed.branch.as_deref() {
                let matching: Vec<_> = edges
                    .iter()
                    .copied()
                    .filter(|edge| workflow_edge_matches_branch(&edge.source_handle, branch))
                    .collect();
                if matching.is_empty() {
                    edges
                        .iter()
                        .copied()
                        .filter(|edge| edge.source_handle.is_empty())
                        .collect()
                } else {
                    matching
                }
            } else {
                edges.clone()
            };
            for edge in selected {
                queue.push_back(edge.target.clone());
            }
        }
    }
    if final_answer.trim().is_empty() {
        final_answer = outputs
            .values()
            .find_map(|values| {
                ["answer", "result", "text", "output", "body"]
                    .iter()
                    .find_map(|key| values.get(*key).map(value_to_text))
            })
            .unwrap_or_default();
    }
    if final_answer.trim().is_empty() {
        return Err("工作流完成，但输出节点没有产生结果".to_owned());
    }
    Ok(ProviderCompletion {
        answer: final_answer,
        usage: total_usage,
    })
}

fn workflow_edge_matches_branch(handle: &str, branch: &str) -> bool {
    handle == branch
        || handle.ends_with(branch)
        || (branch == "true" && handle.contains("case"))
        || (branch == "false" && (handle.contains("else") || handle.contains("false")))
}

#[allow(clippy::too_many_arguments)]
fn execute_workflow_node<F>(
    paths: &AppPaths,
    app: &LocalDifyApp,
    provider: &LocalDifyProvider,
    api_key: &str,
    request: &LocalDifyRunRequest,
    _run_id: &str,
    route: &[String],
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    emit: &mut F,
) -> Result<WorkflowNodeResult, String>
where
    F: FnMut(LocalDifyStreamEvent),
{
    let empty_usage = LocalDifyUsage::default();
    match node.kind.as_str() {
        "start" => {
            let mut values = request.inputs.clone();
            values
                .entry(app.input_key.clone())
                .or_insert_with(|| json!(request.query));
            values
                .entry("query".to_owned())
                .or_insert_with(|| json!(request.query));
            Ok(WorkflowNodeResult {
                outputs: values,
                branch: None,
                answer: None,
                usage: empty_usage,
            })
        }
        "llm" => {
            let messages = workflow_llm_messages(node, outputs, request);
            let stream = request.stream && provider.streaming;
            let temperature = node
                .config
                .get("model")
                .and_then(|value| value.pointer("/completion_params/temperature"))
                .and_then(Value::as_f64)
                .unwrap_or(app.temperature as f64);
            let payload = json!({
                "model": provider.model,
                "messages": messages,
                "temperature": temperature,
                "max_tokens": app.max_output_tokens.min(provider.max_output_tokens),
                "stream": stream,
                "stream_options": if stream { json!({"include_usage": true}) } else { Value::Null },
                "user": request.user,
            });
            let completion = call_provider(provider, api_key, &payload, route, |content| {
                emit(LocalDifyStreamEvent::Delta { content });
            })?;
            Ok(WorkflowNodeResult {
                outputs: BTreeMap::from([
                    ("text".to_owned(), json!(completion.answer)),
                    ("result".to_owned(), json!(completion.answer)),
                ]),
                branch: None,
                answer: None,
                usage: completion.usage,
            })
        }
        "template-transform" => {
            let template = node
                .config
                .get("template")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let rendered = render_workflow_template(template, outputs, request);
            Ok(WorkflowNodeResult {
                outputs: BTreeMap::from([("output".to_owned(), json!(rendered))]),
                branch: None,
                answer: None,
                usage: empty_usage,
            })
        }
        "if-else" => {
            let (matched, case_id) = evaluate_workflow_condition(node, outputs, request);
            Ok(WorkflowNodeResult {
                outputs: BTreeMap::from([("result".to_owned(), json!(matched))]),
                branch: Some(if matched { case_id } else { "false".to_owned() }),
                answer: None,
                usage: empty_usage,
            })
        }
        "http-request" => {
            let result = execute_workflow_http(node, outputs, request)?;
            Ok(WorkflowNodeResult {
                outputs: result,
                branch: None,
                answer: None,
                usage: empty_usage,
            })
        }
        "code" => {
            let result = execute_workflow_python(paths, node, outputs, request)?;
            Ok(WorkflowNodeResult {
                outputs: result,
                branch: None,
                answer: None,
                usage: empty_usage,
            })
        }
        "rpaz-package" => {
            let result = execute_workflow_rpaz(paths, node, outputs, request)?;
            Ok(WorkflowNodeResult {
                outputs: result,
                branch: None,
                answer: None,
                usage: empty_usage,
            })
        }
        "question-classifier" => {
            let (result, branch, usage) = execute_workflow_question_classifier(
                provider, api_key, request, route, node, outputs,
            )?;
            Ok(WorkflowNodeResult {
                outputs: result,
                branch: Some(branch),
                answer: None,
                usage,
            })
        }
        "parameter-extractor" => {
            let (result, usage) = execute_workflow_parameter_extractor(
                provider, api_key, request, route, node, outputs,
            )?;
            Ok(WorkflowNodeResult {
                outputs: result,
                branch: None,
                answer: None,
                usage,
            })
        }
        "variable-aggregator" => Ok(WorkflowNodeResult {
            outputs: execute_workflow_variable_aggregator(node, outputs, request),
            branch: None,
            answer: None,
            usage: empty_usage,
        }),
        "list-operator" => Ok(WorkflowNodeResult {
            outputs: execute_workflow_list_operator(node, outputs, request),
            branch: None,
            answer: None,
            usage: empty_usage,
        }),
        "document-extractor" => Ok(WorkflowNodeResult {
            outputs: execute_workflow_document_extractor(paths, node, outputs, request)?,
            branch: None,
            answer: None,
            usage: empty_usage,
        }),
        "knowledge-retrieval" => Ok(WorkflowNodeResult {
            outputs: execute_workflow_knowledge_retrieval(paths, node, outputs, request)?,
            branch: None,
            answer: None,
            usage: empty_usage,
        }),
        "answer" => {
            let answer = node
                .config
                .get("answer")
                .and_then(Value::as_str)
                .map(|value| render_workflow_template(value, outputs, request))
                .unwrap_or_default();
            Ok(WorkflowNodeResult {
                outputs: BTreeMap::from([("answer".to_owned(), json!(answer))]),
                branch: None,
                answer: Some(answer),
                usage: empty_usage,
            })
        }
        "end" => {
            let values = workflow_end_outputs(node, outputs, request);
            let answer = values
                .get("answer")
                .or_else(|| values.get("result"))
                .or_else(|| values.values().next())
                .map(value_to_text)
                .unwrap_or_default();
            Ok(WorkflowNodeResult {
                outputs: values,
                branch: None,
                answer: Some(answer),
                usage: empty_usage,
            })
        }
        other => Err(format!("本地执行器尚未实现节点类型 {other}")),
    }
}

fn workflow_llm_messages(
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Vec<Value> {
    let prompt = node.config.get("prompt_template");
    let mut messages = Vec::new();
    if let Some(items) = prompt.and_then(Value::as_array) {
        for item in items {
            let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
            if !text.is_empty() {
                messages.push(json!({
                    "role": item.get("role").and_then(Value::as_str).unwrap_or("user"),
                    "content": render_workflow_template(text, outputs, request),
                }));
            }
        }
    } else if let Some(text) = prompt
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
    {
        messages.push(json!({
            "role": "user",
            "content": render_workflow_template(text, outputs, request),
        }));
    }
    if messages.is_empty() {
        messages.push(json!({"role": "user", "content": request.query}));
    }
    messages
}

fn workflow_end_outputs(
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> BTreeMap<String, Value> {
    let mut values = BTreeMap::new();
    if let Some(items) = node.config.get("outputs").and_then(Value::as_array) {
        for item in items {
            let name = item
                .get("variable")
                .and_then(Value::as_str)
                .unwrap_or("result");
            let value = item
                .get("value_selector")
                .and_then(Value::as_array)
                .and_then(|selector| lookup_selector(selector, outputs, request))
                .unwrap_or(Value::Null);
            values.insert(name.to_owned(), value);
        }
    }
    values
}

fn evaluate_workflow_condition(
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> (bool, String) {
    let case = node
        .config
        .get("cases")
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let case_id = case
        .and_then(|item| item.get("case_id"))
        .and_then(Value::as_str)
        .unwrap_or("true")
        .to_owned();
    let condition = case
        .and_then(|item| item.get("conditions"))
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let actual = condition
        .and_then(|item| item.get("variable_selector"))
        .and_then(Value::as_array)
        .and_then(|selector| lookup_selector(selector, outputs, request))
        .unwrap_or(Value::Null);
    let expected = condition
        .and_then(|item| item.get("value"))
        .cloned()
        .unwrap_or(Value::Null);
    let operator = condition
        .and_then(|item| item.get("comparison_operator"))
        .and_then(Value::as_str)
        .unwrap_or("is");
    let actual_text = value_to_text(&actual);
    let expected_text = value_to_text(&expected);
    let matched = match operator {
        "contains" => actual_text.contains(&expected_text),
        "not contains" | "not_contains" => !actual_text.contains(&expected_text),
        "start with" | "starts_with" => actual_text.starts_with(&expected_text),
        "end with" | "ends_with" => actual_text.ends_with(&expected_text),
        "is not" | "not_equal" => actual != expected && actual_text != expected_text,
        "empty" | "is_empty" => actual_text.is_empty(),
        "not empty" | "is_not_empty" => !actual_text.is_empty(),
        ">" | "greater_than" => numeric_value(&actual) > numeric_value(&expected),
        "<" | "less_than" => numeric_value(&actual) < numeric_value(&expected),
        ">=" | "greater_than_or_equal" => numeric_value(&actual) >= numeric_value(&expected),
        "<=" | "less_than_or_equal" => numeric_value(&actual) <= numeric_value(&expected),
        _ => actual == expected || actual_text == expected_text,
    };
    (matched, case_id)
}

fn numeric_value(value: &Value) -> f64 {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .unwrap_or_default()
}

fn lookup_selector(
    selector: &[Value],
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Option<Value> {
    let node_id = selector.first()?.as_str()?;
    let key = selector.get(1)?.as_str()?;
    if node_id == "sys" && key == "query" {
        return Some(json!(request.query));
    }
    outputs.get(node_id)?.get(key).cloned()
}

fn lookup_workflow_key(
    key: &str,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Option<Value> {
    let key = key.trim().trim_matches('#').trim();
    if matches!(key, "query" | "sys.query") {
        return Some(json!(request.query));
    }
    if let Some((node_id, output_key)) = key.split_once('.') {
        return outputs.get(node_id)?.get(output_key).cloned();
    }
    request
        .inputs
        .get(key)
        .cloned()
        .or_else(|| outputs.values().find_map(|values| values.get(key).cloned()))
}

fn render_workflow_template(
    template: &str,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> String {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        rendered.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let Some(end) = tail.find("}}") else {
            rendered.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let key = &tail[..end];
        if let Some(value) = lookup_workflow_key(key, outputs, request) {
            rendered.push_str(&value_to_text(&value));
        } else {
            rendered.push_str(&rest[start..start + end + 4]);
        }
        rest = &tail[end + 2..];
    }
    rendered.push_str(rest);
    rendered
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn configured_selector<'a>(node: &'a WorkflowNode, key: &str) -> Option<&'a [Value]> {
    node.config
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

fn selected_node_value(
    node: &WorkflowNode,
    key: &str,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Value {
    configured_selector(node, key)
        .and_then(|selector| lookup_selector(selector, outputs, request))
        .unwrap_or_else(|| json!(request.query))
}

fn parse_json_object_from_model(text: &str) -> Option<serde_json::Map<String, Value>> {
    let trimmed = text.trim();
    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    serde_json::from_str::<Value>(unfenced)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .or_else(|| {
            let start = trimmed.find('{')?;
            let end = trimmed.rfind('}')?;
            serde_json::from_str::<Value>(&trimmed[start..=end])
                .ok()?
                .as_object()
                .cloned()
        })
}

fn execute_workflow_question_classifier(
    provider: &LocalDifyProvider,
    api_key: &str,
    request: &LocalDifyRunRequest,
    route: &[String],
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
) -> Result<(BTreeMap<String, Value>, String, LocalDifyUsage), String> {
    let classes = node
        .config
        .get("classes")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(|| "问题分类器至少需要一个类别".to_owned())?;
    let query = configured_selector(node, "query_variable_selector")
        .and_then(|selector| lookup_selector(selector, outputs, request))
        .unwrap_or_else(|| json!(request.query));
    let class_descriptions = classes
        .iter()
        .map(|item| {
            json!({
                "id": item.get("id").and_then(Value::as_str).unwrap_or_default(),
                "name": item.get("name").and_then(Value::as_str).unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "model": provider.model,
        "messages": [{
            "role": "system",
            "content": "你是问题分类器。只能从给定类别中选择一个，并只返回 JSON：{\"class_id\":\"类别ID\"}。"
        }, {
            "role": "user",
            "content": format!(
                "类别：{}\n待分类内容：{}",
                serde_json::to_string(&class_descriptions).unwrap_or_default(),
                value_to_text(&query)
            )
        }],
        "temperature": 0,
        "max_tokens": 128,
        "stream": false,
        "user": request.user,
    });
    let completion = call_provider(provider, api_key, &payload, route, |_| {})?;
    let parsed = parse_json_object_from_model(&completion.answer);
    let requested_id = parsed
        .as_ref()
        .and_then(|value| value.get("class_id").or_else(|| value.get("id")))
        .and_then(Value::as_str)
        .unwrap_or_else(|| completion.answer.trim());
    let selected = classes
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(requested_id))
        .or_else(|| {
            classes.iter().find(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| completion.answer.contains(name))
            })
        })
        .unwrap_or(&classes[0]);
    let class_id = selected
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("1")
        .to_owned();
    let class_name = selected
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok((
        BTreeMap::from([
            ("class_id".to_owned(), json!(class_id)),
            ("class_name".to_owned(), json!(class_name)),
            ("query".to_owned(), query),
        ]),
        class_id,
        completion.usage,
    ))
}

fn execute_workflow_parameter_extractor(
    provider: &LocalDifyProvider,
    api_key: &str,
    request: &LocalDifyRunRequest,
    route: &[String],
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
) -> Result<(BTreeMap<String, Value>, LocalDifyUsage), String> {
    let parameters = node
        .config
        .get("parameters")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(|| "参数提取器至少需要一个参数定义".to_owned())?;
    let query = selected_node_value(node, "query", outputs, request);
    let instruction = node
        .config
        .get("instruction")
        .and_then(Value::as_str)
        .unwrap_or("从输入文本中提取结构化参数。");
    let payload = json!({
        "model": provider.model,
        "messages": [{
            "role": "system",
            "content": format!(
                "{instruction}\n严格返回一个 JSON 对象，不要附加解释。字段定义：{}",
                serde_json::to_string(parameters).unwrap_or_default()
            )
        }, {
            "role": "user",
            "content": value_to_text(&query)
        }],
        "temperature": 0,
        "max_tokens": provider.max_output_tokens.min(2048),
        "stream": false,
        "user": request.user,
    });
    let completion = call_provider(provider, api_key, &payload, route, |_| {})?;
    let parsed = parse_json_object_from_model(&completion.answer)
        .ok_or_else(|| "参数提取器模型输出不是 JSON 对象".to_owned())?;
    let mut result: BTreeMap<String, Value> = parsed.into_iter().collect();
    result.insert("__text".to_owned(), json!(completion.answer));
    Ok((result, completion.usage))
}

fn execute_workflow_variable_aggregator(
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> BTreeMap<String, Value> {
    let value = node
        .config
        .get("variables")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .filter_map(|selector| lookup_selector(selector, outputs, request))
        .find(|value| !value.is_null() && !matches!(value, Value::String(text) if text.is_empty()))
        .unwrap_or(Value::Null);
    BTreeMap::from([("output".to_owned(), value)])
}

fn list_item_field<'a>(item: &'a Value, key: &str) -> &'a Value {
    if key.trim().is_empty() {
        item
    } else {
        item.get(key).unwrap_or(&Value::Null)
    }
}

fn list_condition_matches(item: &Value, condition: &Value) -> bool {
    let key = condition
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let actual = list_item_field(item, key);
    let expected = condition.get("value").unwrap_or(&Value::Null);
    let operator = condition
        .get("comparison_operator")
        .and_then(Value::as_str)
        .unwrap_or("is");
    let actual_text = value_to_text(actual);
    let expected_text = value_to_text(expected);
    match operator {
        "contains" => actual_text.contains(&expected_text),
        "not_contains" | "not contains" => !actual_text.contains(&expected_text),
        "starts_with" | "start with" => actual_text.starts_with(&expected_text),
        "ends_with" | "end with" => actual_text.ends_with(&expected_text),
        "is_not" | "is not" => actual != expected && actual_text != expected_text,
        "greater_than" | ">" => numeric_value(actual) > numeric_value(expected),
        "less_than" | "<" => numeric_value(actual) < numeric_value(expected),
        "is_empty" | "empty" => actual_text.is_empty(),
        "is_not_empty" | "not empty" => !actual_text.is_empty(),
        _ => actual == expected || actual_text == expected_text,
    }
}

fn execute_workflow_list_operator(
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> BTreeMap<String, Value> {
    let selected = selected_node_value(node, "variable", outputs, request);
    let mut items = selected
        .as_array()
        .cloned()
        .or_else(|| {
            selected
                .as_str()
                .and_then(|text| serde_json::from_str::<Vec<Value>>(text).ok())
        })
        .unwrap_or_default();
    if let Some(filter) = node.config.get("filter_by").filter(|value| {
        value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }) {
        if let Some(conditions) = filter.get("conditions").and_then(Value::as_array) {
            items.retain(|item| {
                conditions
                    .iter()
                    .all(|condition| list_condition_matches(item, condition))
            });
        }
    }
    if let Some(order) = node.config.get("order_by").filter(|value| {
        value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }) {
        let key = order.get("key").and_then(Value::as_str).unwrap_or_default();
        let descending = order.get("value").and_then(Value::as_str) == Some("desc");
        items.sort_by(|left, right| {
            let ordering = value_to_text(list_item_field(left, key))
                .cmp(&value_to_text(list_item_field(right, key)));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    if let Some(limit) = node.config.get("limit").filter(|value| {
        value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }) {
        let size = limit
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .min(10_000) as usize;
        items.truncate(size);
    }
    BTreeMap::from([
        ("result".to_owned(), json!(items)),
        (
            "first_record".to_owned(),
            items.first().cloned().unwrap_or(Value::Null),
        ),
        (
            "last_record".to_owned(),
            items.last().cloned().unwrap_or(Value::Null),
        ),
    ])
}

fn workflow_document_paths(value: &Value) -> Vec<String> {
    match value {
        Value::String(path) => vec![path.clone()],
        Value::Array(items) => items.iter().flat_map(workflow_document_paths).collect(),
        Value::Object(object) => object
            .get("path")
            .or_else(|| object.get("local_path"))
            .map(workflow_document_paths)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn execute_workflow_document_extractor(
    paths: &AppPaths,
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Result<BTreeMap<String, Value>, String> {
    let selected = selected_node_value(node, "variable_selector", outputs, request);
    let document_paths = workflow_document_paths(&selected);
    if document_paths.is_empty() {
        return Err("文档提取器没有收到文件路径".to_owned());
    }
    let workspace = fs::canonicalize(&paths.workspace_root)
        .map_err(|error| format!("无法访问工作区目录：{error}"))?;
    let mut texts = Vec::new();
    for document_path in document_paths {
        let candidate = PathBuf::from(&document_path);
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            workspace.join(candidate)
        };
        let canonical = fs::canonicalize(&candidate)
            .map_err(|error| format!("无法访问文档 {document_path}：{error}"))?;
        if !canonical.starts_with(&workspace) {
            return Err(format!(
                "文档提取器只允许读取当前工作区文件：{document_path}"
            ));
        }
        let metadata = fs::metadata(&canonical).map_err(|error| error.to_string())?;
        if metadata.len() > 5 * 1024 * 1024 {
            return Err(format!("文档超过 5MB 限制：{document_path}"));
        }
        texts.push(
            fs::read_to_string(&canonical)
                .map_err(|error| format!("文档不是 UTF-8 文本 {document_path}：{error}"))?,
        );
    }
    let text = texts.join("\n\n");
    Ok(BTreeMap::from([
        ("text".to_owned(), json!(text)),
        ("documents".to_owned(), json!(texts)),
    ]))
}

fn collect_knowledge_text_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if files.len() >= 2_000 || !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_knowledge_text_files(&path, files)?;
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "md" | "txt" | "json" | "csv" | "yaml" | "yml"
                )
            })
        {
            files.push(path);
        }
        if files.len() >= 2_000 {
            break;
        }
    }
    Ok(())
}

fn execute_workflow_knowledge_retrieval(
    paths: &AppPaths,
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Result<BTreeMap<String, Value>, String> {
    let query = value_to_text(&selected_node_value(
        node,
        "query_variable_selector",
        outputs,
        request,
    ));
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return Err("知识检索查询不能为空".to_owned());
    }
    let top_k = node
        .config
        .get("top_k")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 50) as usize;
    let knowledge_base_ids = node
        .config
        .get("knowledge_base_ids")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let include_vector = node
        .config
        .get("include_knowledge_bases")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let include_documents = node
        .config
        .get("include_documents")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut matches: Vec<(f64, Value)> = Vec::new();
    if include_vector {
        for item in knowledge_base::search_for_agent(
            &paths.workspace_root,
            &knowledge_base_ids,
            &query,
            top_k,
        )? {
            matches.push((
                item.score as f64,
                json!({
                    "type": "knowledge-base",
                    "knowledgeBaseId": item.knowledge_base_id,
                    "knowledgeBaseName": item.knowledge_base_name,
                    "sourceId": item.source_id,
                    "title": item.source_name,
                    "chunkId": item.chunk_id,
                    "content": item.content,
                    "citation": item.citation,
                    "score": item.score,
                    "vectorScore": item.vector_score,
                    "keywordScore": item.keyword_score
                }),
            ));
        }
    }

    let knowledge_root = paths.workspace_root.join("knowledge");
    let mut files = Vec::new();
    if include_documents {
        collect_knowledge_text_files(&knowledge_root, &mut files)?;
    }
    let mut terms = normalized_query
        .split_whitespace()
        .filter(|term| term.chars().count() > 1)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        terms.push(normalized_query.clone());
    }
    for path in files {
        if fs::metadata(&path)
            .map(|value| value.len())
            .unwrap_or(u64::MAX)
            > 1024 * 1024
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let lower = content.to_lowercase();
        let score = terms
            .iter()
            .map(|term| lower.matches(term).count() as u64)
            .sum::<u64>();
        if score == 0 {
            continue;
        }
        let relative = path
            .strip_prefix(&knowledge_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let ranking_score = 0.25 + (score as f64 / (score as f64 + 4.0)) * 0.5;
        matches.push((
            ranking_score,
            json!({
                "type": "knowledge-document",
                "title": path.file_stem().and_then(|value| value.to_str()).unwrap_or_default(),
                "path": relative,
                "content": content.chars().take(1_200).collect::<String>(),
                "citation": format!("知识文档 / {relative}"),
                "score": ranking_score,
                "keywordMatches": score
            }),
        ));
    }
    matches.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let result = matches
        .into_iter()
        .take(top_k)
        .map(|(_, item)| item)
        .collect::<Vec<_>>();
    let text = result
        .iter()
        .filter_map(|item| item.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    Ok(BTreeMap::from([
        ("result".to_owned(), json!(result)),
        ("text".to_owned(), json!(text)),
    ]))
}

fn execute_workflow_http(
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Result<BTreeMap<String, Value>, String> {
    let method = node
        .config
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("get")
        .to_ascii_lowercase();
    let url = render_workflow_template(
        node.config
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        outputs,
        request,
    );
    let parsed = url::Url::parse(&url).map_err(|error| format!("HTTP URL 无效：{error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("HTTP 节点只允许 http/https URL".to_owned());
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let agent = ureq::Agent::new_with_config(agent);
    let headers: Vec<(String, String)> = node
        .config
        .get("headers")
        .and_then(Value::as_str)
        .into_iter()
        .flat_map(str::lines)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| {
            (
                name.trim().to_owned(),
                render_workflow_template(value, outputs, request),
            )
        })
        .collect();
    let body = node
        .config
        .get("body")
        .and_then(|value| value.get("data"))
        .map(value_to_text)
        .unwrap_or_default();
    let rendered_body = render_workflow_template(&body, outputs, request);
    let response = match method.as_str() {
        "post" => {
            let mut builder = agent.post(&url);
            for (name, value) in &headers {
                builder = builder.header(name, value);
            }
            builder.send(rendered_body.as_bytes())
        }
        "put" => {
            let mut builder = agent.put(&url);
            for (name, value) in &headers {
                builder = builder.header(name, value);
            }
            builder.send(rendered_body.as_bytes())
        }
        "patch" => {
            let mut builder = agent.patch(&url);
            for (name, value) in &headers {
                builder = builder.header(name, value);
            }
            builder.send(rendered_body.as_bytes())
        }
        "delete" => {
            let mut builder = agent.delete(&url);
            for (name, value) in &headers {
                builder = builder.header(name, value);
            }
            builder.call()
        }
        _ => {
            let mut builder = agent.get(&url);
            for (name, value) in &headers {
                builder = builder.header(name, value);
            }
            builder.call()
        }
    };
    let mut response = response.map_err(|error| format!("HTTP 请求失败：{error}"))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("读取 HTTP 响应失败：{error}"))?;
    let json_value = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
    Ok(BTreeMap::from([
        ("status_code".to_owned(), json!(status)),
        ("body".to_owned(), json!(body)),
        ("json".to_owned(), json_value),
    ]))
}

fn execute_workflow_python(
    paths: &AppPaths,
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Result<BTreeMap<String, Value>, String> {
    let language = node
        .config
        .get("code_language")
        .and_then(Value::as_str)
        .unwrap_or("python3");
    if !matches!(language, "python" | "python3") {
        return Err("本地代码节点当前执行 Python 3；JavaScript 节点可继续导出到 Dify".to_owned());
    }
    let code = node
        .config
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if code.is_empty() || code.len() > 100_000 {
        return Err("Python 代码应为 1 到 100000 个字符".to_owned());
    }
    let mut inputs = BTreeMap::new();
    if let Some(variables) = node.config.get("variables").and_then(Value::as_array) {
        for variable in variables {
            let name = variable
                .get("variable")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let value = variable
                .get("value_selector")
                .and_then(Value::as_array)
                .and_then(|selector| lookup_selector(selector, outputs, request))
                .unwrap_or(Value::Null);
            inputs.insert(name.to_owned(), value);
        }
    }
    if inputs.is_empty() {
        inputs.insert("input".to_owned(), json!(request.query));
    }
    let payload = json!({"code": code, "inputs": inputs});
    let python = locate_runtime_python(paths)?;
    let temporary = local_dify_root(paths).join("tmp");
    fs::create_dir_all(&temporary).map_err(|error| error.to_string())?;
    let id = Uuid::new_v4().simple().to_string();
    let stdout_path = temporary.join(format!("{id}.stdout"));
    let stderr_path = temporary.join(format!("{id}.stderr"));
    let stdout = fs::File::create(&stdout_path).map_err(|error| error.to_string())?;
    let stderr = fs::File::create(&stderr_path).map_err(|error| error.to_string())?;
    let wrapper = r#"import json, sys
payload = json.load(sys.stdin)
scope = {}
exec(compile(payload['code'], '<local-dify-code>', 'exec'), scope, scope)
main = scope.get('main')
if not callable(main):
    raise RuntimeError('code node must define main(...)')
result = main(**payload.get('inputs', {}))
if not isinstance(result, dict):
    result = {'result': result}
print(json.dumps(result, ensure_ascii=False))
"#;
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", wrapper])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    hide_workflow_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 Python 代码节点失败：{error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload.to_string().as_bytes())
            .map_err(|error| format!("写入 Python 输入失败：{error}"))?;
    }
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err("Python 代码节点执行超过 30 秒".to_owned());
        }
        thread::sleep(Duration::from_millis(40));
    };
    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    if !status.success() {
        return Err(format!("Python 代码执行失败：{}", stderr.trim()));
    }
    let value: Value = serde_json::from_str(stdout.trim())
        .map_err(|error| format!("Python 代码节点输出不是 JSON 对象：{error}"))?;
    value
        .as_object()
        .cloned()
        .map(|object| object.into_iter().collect())
        .ok_or_else(|| "Python 代码节点输出必须是对象".to_owned())
}

fn execute_workflow_rpaz(
    paths: &AppPaths,
    node: &WorkflowNode,
    outputs: &WorkflowOutputs,
    request: &LocalDifyRunRequest,
) -> Result<BTreeMap<String, Value>, String> {
    let package_id = node
        .config
        .get("package_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if package_id.is_empty() {
        return Err("RPAZ 包节点尚未选择包".to_owned());
    }
    let catalog = installed_package_catalog(paths)?;
    let descriptor = catalog
        .get(package_id)
        .ok_or_else(|| format!("找不到已安装 RPAZ 包：{package_id}"))?;
    let mut parameters = serde_json::Map::new();
    if let Some(config) = node.config.get("parameters").and_then(Value::as_object) {
        for (key, value) in config {
            parameters.insert(
                key.clone(),
                if let Some(template) = value.as_str() {
                    json!(render_workflow_template(template, outputs, request))
                } else {
                    value.clone()
                },
            );
        }
    }
    if parameters.is_empty() {
        parameters.insert("input".to_owned(), json!(request.query));
    }
    let invocation_id = Uuid::new_v4().simple().to_string();
    let root = local_dify_root(paths)
        .join("tmp")
        .join(format!("rpaz-{invocation_id}"));
    let output_dir = root.join("outputs");
    fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    let request_path = root.join("request.json");
    let result_path = root.join("result.json");
    let stdout_path = root.join("stdout.jsonl");
    let stderr_path = root.join("stderr.log");
    let runtime_request = json!({
        "protocol": RUNTIME_PROTOCOL_VERSION,
        "run_id": format!("workflow-{invocation_id}"),
        "package_id": package_id,
        "package_dir": descriptor.get("package_dir"),
        "output_dir": output_dir,
        "entrypoint": descriptor.get("entrypoint"),
        "callable": descriptor.get("callable"),
        "parameters": parameters,
        "database_path": paths.workspace_root.join("databases/workspace.sqlite3"),
        "package_catalog": catalog,
        "result_path": result_path,
    });
    fs::write(
        &request_path,
        serde_json::to_vec_pretty(&runtime_request).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("写入 RPAZ 节点请求失败：{error}"))?;
    let runtime = locate_runtime(paths)?;
    let stdout = fs::File::create(&stdout_path).map_err(|error| error.to_string())?;
    let stderr = fs::File::create(&stderr_path).map_err(|error| error.to_string())?;
    let mut command = Command::new(runtime.python);
    command
        .args(["-m", "drpa_runner.cli", "--request"])
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    if let Some(python_path) = runtime.python_path {
        command.env("PYTHONPATH", python_path);
    }
    if let Some(browser) = runtime.browser {
        command.env("DRPA_BROWSER_PATH", browser);
    }
    hide_workflow_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 RPAZ 包节点失败：{error}"))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(300) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("RPAZ 包节点执行超过 5 分钟".to_owned());
        }
        thread::sleep(Duration::from_millis(50));
    };
    if !status.success() {
        let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
        let events = fs::read_to_string(&stdout_path).unwrap_or_default();
        return Err(format!(
            "RPAZ 包执行失败：{}",
            if stderr.trim().is_empty() {
                events.trim()
            } else {
                stderr.trim()
            }
        ));
    }
    let result = if result_path.is_file() {
        serde_json::from_slice::<Value>(
            &fs::read(&result_path).map_err(|error| format!("读取 RPAZ 包结果失败：{error}"))?,
        )
        .map_err(|error| format!("RPAZ 包结果不是有效 JSON：{error}"))?
    } else {
        Value::Null
    };
    let _ = fs::remove_file(&request_path);
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    Ok(match result {
        Value::Object(object) => {
            let mut output: BTreeMap<String, Value> = object.clone().into_iter().collect();
            output.insert("result".to_owned(), Value::Object(object));
            output
        }
        other => BTreeMap::from([("result".to_owned(), other)]),
    })
}

#[cfg(windows)]
fn hide_workflow_child_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_workflow_child_window(_command: &mut Command) {}

fn call_provider<F>(
    provider: &LocalDifyProvider,
    api_key: &str,
    payload: &Value,
    route: &[String],
    on_delta: F,
) -> Result<ProviderCompletion, String>
where
    F: FnMut(String),
{
    let mut headers = provider.custom_headers.clone();
    headers.insert(
        "X-DRPA-Trace-Id".to_owned(),
        format!("trace-{}", Uuid::new_v4().simple()),
    );
    headers.insert("X-DRPA-Hop-Count".to_owned(), route.len().to_string());
    headers.insert("X-DRPA-Provider-Route".to_owned(), route.join(","));
    let profile = crate::provider::ProviderProfile {
        base_url: provider.base_url.clone(),
        api_key: api_key.to_owned(),
        timeout: Duration::from_secs(provider.timeout_seconds),
        user_agent: "DRPA-Local-Dify/1.0".to_owned(),
        headers,
    };
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let value = crate::provider::complete_blocking(&profile, payload, stream, on_delta)
        .map_err(|error| error.to_string())?;
    provider_completion_from_json(&value)
}

fn provider_completion_from_json(value: &Value) -> Result<ProviderCompletion, String> {
    let answer = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("answer").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    if answer.trim().is_empty() {
        return Err("Provider 返回了空消息".to_owned());
    }
    Ok(ProviderCompletion {
        answer,
        usage: parse_usage(
            value
                .get("usage")
                .or_else(|| value.pointer("/metadata/usage")),
        ),
    })
}

fn parse_usage(value: Option<&Value>) -> LocalDifyUsage {
    let prompt_tokens = value
        .and_then(|item| item.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let completion_tokens = value
        .and_then(|item| item.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let total_tokens = value
        .and_then(|item| item.get("total_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(prompt_tokens.saturating_add(completion_tokens));
    LocalDifyUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    }
}

#[allow(clippy::too_many_arguments)]
fn record_run(
    paths: &AppPaths,
    run_id: &str,
    app: &LocalDifyApp,
    status: &str,
    query: &str,
    answer: &str,
    provider: &LocalDifyProvider,
    usage: &LocalDifyUsage,
    duration_ms: u64,
    error: &str,
) -> Result<(), String> {
    let connection = open_runtime_database(paths)?;
    connection
        .execute(
            "INSERT INTO local_dify_runs
             (id, app_id, app_name, status, query, answer, provider_id, model,
              prompt_tokens, completion_tokens, duration_ms, error, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                run_id,
                app.id,
                app.name,
                status,
                query,
                answer,
                provider.id,
                provider.model,
                usage.prompt_tokens,
                usage.completion_tokens,
                duration_ms,
                error,
                now_timestamp(),
            ],
        )
        .map_err(|error| format!("记录流程运行失败：{error}"))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn list_local_dify_runs(
    app_id: Option<String>,
    limit: Option<usize>,
    paths: State<'_, AppPaths>,
) -> Result<Vec<LocalDifyRunSummary>, String> {
    ensure_root(&paths)?;
    if let Some(app_id) = &app_id {
        validate_identifier(app_id, "流程")?;
    }
    let connection = open_runtime_database(&paths)?;
    let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
    let sql = if app_id.is_some() {
        "SELECT id, app_id, app_name, status, query, answer, provider_id, model,
                prompt_tokens, completion_tokens, duration_ms, error, created_at
         FROM local_dify_runs WHERE app_id = ?1 ORDER BY created_at DESC LIMIT ?2"
    } else {
        "SELECT id, app_id, app_name, status, query, answer, provider_id, model,
                prompt_tokens, completion_tokens, duration_ms, error, created_at
         FROM local_dify_runs ORDER BY created_at DESC LIMIT ?2"
    };
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok(LocalDifyRunSummary {
            id: row.get(0)?,
            app_id: row.get(1)?,
            app_name: row.get(2)?,
            status: row.get(3)?,
            query: row.get(4)?,
            answer: row.get(5)?,
            provider_id: row.get(6)?,
            model: row.get(7)?,
            prompt_tokens: row.get(8)?,
            completion_tokens: row.get(9)?,
            duration_ms: row.get(10)?,
            error: row.get(11)?,
            created_at: row.get(12)?,
        })
    };
    let rows = if let Some(app_id) = app_id {
        statement.query_map(params![app_id, limit], mapper)
    } else {
        statement.query_map(params!["", limit], mapper)
    }
    .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn publish_local_dify_app(
    app_id: String,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyApp, String> {
    let mut app = load_app(&paths, &app_id)?;
    let report = compatibility_report(&app, &paths)?;
    if !report.compatible {
        return Err("应用存在阻止发布的兼容性问题".to_owned());
    }
    app.published_version = app.published_version.saturating_add(1);
    app.api_enabled = true;
    app.updated_at = now_timestamp();
    write_json_atomic(&app_path(&paths, &app.id), &app)?;
    let _ = ensure_app_token(&paths, &app.id)?;
    Ok(app)
}

#[tauri::command]
pub(crate) fn get_local_dify_app_api_token(
    app_id: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    let app = load_app(&paths, &app_id)?;
    if !app.api_enabled {
        return Err("请先发布应用以启用本地 API".to_owned());
    }
    ensure_app_token(&paths, &app.id)
}

fn ensure_app_token(paths: &AppPaths, app_id: &str) -> Result<String, String> {
    let mut secrets: LocalDifySecrets = read_json_or_default(&secrets_path(paths))?;
    let token = secrets
        .app_api_tokens
        .entry(app_id.to_owned())
        .or_insert_with(|| format!("app-{}", Uuid::new_v4().simple()))
        .clone();
    write_json_atomic(&secrets_path(paths), &secrets)?;
    Ok(token)
}

fn find_app_by_token(paths: &AppPaths, token: &str) -> Result<LocalDifyApp, String> {
    let secrets: LocalDifySecrets = read_json_or_default(&secrets_path(paths))?;
    let app_id = secrets
        .app_api_tokens
        .iter()
        .find_map(|(app_id, saved)| (saved == token).then_some(app_id))
        .ok_or_else(|| "本地 Dify API Token 无效".to_owned())?;
    let app = load_app(paths, app_id)?;
    if !app.api_enabled {
        return Err("本地 Dify 应用 API 尚未发布".to_owned());
    }
    Ok(app)
}

#[tauri::command]
pub(crate) fn check_local_dify_compatibility(
    app_id: String,
    paths: State<'_, AppPaths>,
) -> Result<DifyCompatibilityReport, String> {
    let app = load_app(&paths, &app_id)?;
    compatibility_report(&app, &paths)
}

fn compatibility_report(
    app: &LocalDifyApp,
    paths: &AppPaths,
) -> Result<DifyCompatibilityReport, String> {
    let mut issues = Vec::new();
    if is_workflow_mode(&app.mode) {
        let report = validate_graph(&app.workflow, &app.mode);
        issues.extend(
            report
                .issues
                .into_iter()
                .map(|issue| DifyCompatibilityIssue {
                    level: issue.level,
                    code: format!("workflow-{}", issue.code),
                    message: issue.message,
                }),
        );
    }
    if app.provider_id.is_empty() {
        issues.push(DifyCompatibilityIssue {
            level: "error".to_owned(),
            code: "provider-missing".to_owned(),
            message: "应用尚未选择 Provider。".to_owned(),
        });
    } else {
        let (provider, _) = load_provider(paths, &app.provider_id)?;
        if provider.dify_provider.trim().is_empty() {
            issues.push(DifyCompatibilityIssue {
                level: "warning".to_owned(),
                code: "cloud-provider-mapping".to_owned(),
                message: "尚未设置 Dify 云端 Provider 映射，导出后需要在云端重新选择模型。"
                    .to_owned(),
            });
        }
        if provider.base_url.contains("127.0.0.1") || provider.base_url.contains("localhost") {
            issues.push(DifyCompatibilityIssue {
                level: "warning".to_owned(),
                code: "local-provider-url".to_owned(),
                message: "Provider 指向本机地址，上传云端后需要替换。".to_owned(),
            });
        }
    }
    Ok(DifyCompatibilityReport {
        compatible: !issues.iter().any(|issue| issue.level == "error"),
        target_version: "0.3.1".to_owned(),
        issues,
    })
}

#[tauri::command]
pub(crate) fn export_local_dify_dsl(
    app_id: String,
    target_path: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    let app = load_app(&paths, &app_id)?;
    let provider = if app.provider_id.is_empty() {
        None
    } else {
        Some(load_provider(&paths, &app.provider_id)?.0)
    };
    let yaml = render_dify_dsl(&app, provider.as_ref())?;
    let mut target = PathBuf::from(target_path);
    if target.extension().is_none() {
        target.set_extension("yml");
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{error}"))?;
    }
    fs::write(&target, yaml).map_err(|error| format!("导出 Dify DSL 失败：{error}"))?;
    Ok(target.to_string_lossy().into_owned())
}

fn render_dify_dsl(
    app: &LocalDifyApp,
    provider: Option<&LocalDifyProvider>,
) -> Result<String, String> {
    let provider_name = provider
        .map(|item| {
            if item.dify_provider.is_empty() {
                item.id.as_str()
            } else {
                item.dify_provider.as_str()
            }
        })
        .unwrap_or("openai");
    let model_name = provider
        .map(|item| {
            if item.dify_model.is_empty() {
                item.model.as_str()
            } else {
                item.dify_model.as_str()
            }
        })
        .unwrap_or("deepseek-v4-flash");
    let mut value = json!({
        "version": "0.3.1",
        "kind": "app",
        "app": {
            "description": app.description,
            "icon": "🤖",
            "icon_background": "#E4E7FF",
            "mode": app.mode,
            "name": app.name,
            "use_icon_as_answer_icon": false,
        },
        "dependencies": [],
    });
    if is_workflow_mode(&app.mode) {
        let mut graph = app.workflow.clone();
        for node in graph.nodes.iter_mut().filter(|node| node.kind == "llm") {
            let mut model = node
                .config
                .get("model")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            model.insert("provider".to_owned(), json!(provider_name));
            model.insert("name".to_owned(), json!(model_name));
            model
                .entry("mode".to_owned())
                .or_insert_with(|| json!("chat"));
            model.insert(
                "completion_params".to_owned(),
                json!({
                    "max_tokens": app.max_output_tokens,
                    "temperature": app.temperature,
                }),
            );
            node.config.insert("model".to_owned(), Value::Object(model));
        }
        value.as_object_mut().expect("DSL root is object").insert(
            "workflow".to_owned(),
            json!({
                "conversation_variables": [],
                "environment_variables": [],
                "features": {
                    "file_upload": {"enabled": false},
                    "opening_statement": app.opening_statement,
                    "retriever_resource": {"enabled": false},
                    "sensitive_word_avoidance": {"enabled": false},
                    "speech_to_text": {"enabled": false},
                    "suggested_questions": [],
                    "suggested_questions_after_answer": {"enabled": false},
                    "text_to_speech": {"enabled": false},
                },
                "graph": graph_to_dify(&graph),
            }),
        );
    } else {
        value.as_object_mut().expect("DSL root is object").insert(
            "model_config".to_owned(),
            json!({
                "model": {
                    "completion_params": {
                        "max_tokens": app.max_output_tokens,
                        "temperature": app.temperature,
                    },
                    "mode": "chat",
                    "name": model_name,
                    "provider": provider_name,
                },
                "pre_prompt": app.system_prompt,
                "prompt_type": "simple",
                "user_input_form": [{
                    "paragraph": {
                        "default": "",
                        "label": app.input_key,
                        "max_length": 1000000,
                        "required": true,
                        "variable": app.input_key,
                    }
                }],
            }),
        );
    }
    serde_yaml::to_string(&value).map_err(|error| format!("生成 Dify DSL 失败：{error}"))
}

#[tauri::command]
pub(crate) fn import_local_dify_dsl(
    source_path: String,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyApp, String> {
    ensure_root(&paths)?;
    let source_path = PathBuf::from(source_path);
    let metadata =
        fs::metadata(&source_path).map_err(|error| format!("读取 DSL 文件失败：{error}"))?;
    if metadata.len() > MAX_DSL_BYTES {
        return Err("Dify DSL 文件超过 10MB".to_owned());
    }
    let source =
        fs::read_to_string(&source_path).map_err(|error| format!("读取 DSL 文件失败：{error}"))?;
    import_dsl_source(&source, &paths)
}

fn import_dsl_source(source: &str, paths: &AppPaths) -> Result<LocalDifyApp, String> {
    let value: Value =
        serde_yaml::from_str(source).map_err(|error| format!("Dify DSL YAML 无效：{error}"))?;
    if value.get("kind").and_then(Value::as_str) != Some("app") {
        return Err("Dify DSL 缺少 kind: app".to_owned());
    }
    let app_value = value
        .get("app")
        .ok_or_else(|| "Dify DSL 缺少 app 配置".to_owned())?;
    let name = app_value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("导入的 Dify 应用");
    let mode = app_value
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("chat");
    let model_config = value
        .get("model_config")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let imported_workflow = graph_from_dify(&value);
    let workflow_model = imported_workflow
        .as_ref()
        .and_then(|graph| graph.nodes.iter().find(|node| node.kind == "llm"))
        .and_then(|node| node.config.get("model"))
        .cloned();
    let model_name = model_config
        .pointer("/model/name")
        .and_then(Value::as_str)
        .or_else(|| {
            workflow_model
                .as_ref()
                .and_then(|model| model.get("name"))
                .and_then(Value::as_str)
        })
        .unwrap_or("deepseek-v4-flash");
    let provider_name = model_config
        .pointer("/model/provider")
        .and_then(Value::as_str)
        .or_else(|| {
            workflow_model
                .as_ref()
                .and_then(|model| model.get("provider"))
                .and_then(Value::as_str)
        })
        .unwrap_or("openai");
    let providers = load_providers(paths)?;
    let provider_id = providers
        .iter()
        .find(|provider| {
            provider.dify_provider == provider_name && provider.dify_model == model_name
        })
        .or_else(|| {
            providers
                .iter()
                .find(|provider| provider.model == model_name)
        })
        .map(|provider| provider.id.clone())
        .unwrap_or_default();
    let now = now_timestamp();
    let input_key = imported_workflow
        .as_ref()
        .map(extract_workflow_input_key)
        .unwrap_or_else(|| extract_input_key(&model_config));
    let workflow = imported_workflow.unwrap_or_else(|| default_graph(mode, &input_key));
    let app = LocalDifyApp {
        schema: LOCAL_DIFY_SCHEMA,
        id: format!("app-{}", Uuid::new_v4().simple()),
        name: name.to_owned(),
        description: app_value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        mode: mode.to_owned(),
        provider_id,
        system_prompt: model_config
            .get("pre_prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        opening_statement: model_config
            .get("opening_statement")
            .and_then(Value::as_str)
            .or_else(|| {
                value
                    .pointer("/workflow/features/opening_statement")
                    .and_then(Value::as_str)
            })
            .unwrap_or_default()
            .to_owned(),
        input_key,
        temperature: model_config
            .pointer("/model/completion_params/temperature")
            .and_then(Value::as_f64)
            .or_else(|| {
                workflow_model
                    .as_ref()
                    .and_then(|model| model.pointer("/completion_params/temperature"))
                    .and_then(Value::as_f64)
            })
            .unwrap_or(0.2) as f32,
        max_output_tokens: model_config
            .pointer("/model/completion_params/max_tokens")
            .and_then(Value::as_u64)
            .or_else(|| {
                workflow_model
                    .as_ref()
                    .and_then(|model| model.pointer("/completion_params/max_tokens"))
                    .and_then(Value::as_u64)
            })
            .unwrap_or(98_304)
            .clamp(64, 131_072) as u32,
        workflow,
        published_version: 0,
        api_enabled: false,
        created_at: now,
        updated_at: now,
    };
    validate_app(&app)?;
    write_json_atomic(&app_path(paths, &app.id), &app)?;
    fs::write(source_dsl_path(paths, &app.id), source)
        .map_err(|error| format!("保留原始 DSL 失败：{error}"))?;
    Ok(app)
}

fn extract_input_key(model_config: &Value) -> String {
    model_config
        .get("user_input_form")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
        .and_then(|entry| entry.values().next())
        .and_then(|field| field.get("variable"))
        .and_then(Value::as_str)
        .unwrap_or("query")
        .to_owned()
}

fn extract_workflow_input_key(graph: &WorkflowGraph) -> String {
    graph
        .nodes
        .iter()
        .find(|node| node.kind == "start")
        .and_then(|node| node.config.get("variables"))
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("variable"))
        .and_then(Value::as_str)
        .unwrap_or("query")
        .to_owned()
}

impl LocalDifyServiceManager {
    pub(crate) fn status(&self) -> Result<LocalDifyServiceStatus, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "Local Dify 服务状态已损坏".to_owned())?;
        Ok(LocalDifyServiceStatus {
            running: state.running,
            port: state.port,
            endpoint: format!("http://127.0.0.1:{}/v1", state.port),
            started_at: state.started_at,
            last_error: state.last_error.clone(),
        })
    }

    fn start(&self, port: u16, paths: AppPaths) -> Result<LocalDifyServiceStatus, String> {
        if port < 1_024 {
            return Err("Local Dify 服务端口必须大于等于 1024".to_owned());
        }
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|error| format!("启动 Local Dify 服务失败：{error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Local Dify 服务状态已损坏".to_owned())?;
            if state.running {
                return Err("Local Dify 服务已经在运行".to_owned());
            }
            state.running = true;
            state.port = port;
            state.started_at = Some(now_timestamp());
            state.last_error.clear();
            state.stop = Some(Arc::clone(&stop));
        }
        let manager = self.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let request_paths = paths.clone();
                        thread::spawn(move || {
                            let _ = handle_service_connection(stream, &request_paths);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(error) => {
                        if let Ok(mut state) = manager.state.lock() {
                            state.last_error = error.to_string();
                        }
                        break;
                    }
                }
            }
            if let Ok(mut state) = manager.state.lock() {
                state.running = false;
                state.stop = None;
            }
        });
        self.status()
    }

    fn stop(&self) -> Result<LocalDifyServiceStatus, String> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| "Local Dify 服务状态已损坏".to_owned())?;
            if let Some(stop) = &state.stop {
                stop.store(true, Ordering::Relaxed);
            }
        }
        for _ in 0..40 {
            if !self.status()?.running {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        self.status()
    }
}

#[tauri::command]
pub(crate) fn get_local_dify_service_status(
    manager: State<'_, LocalDifyServiceManager>,
) -> Result<LocalDifyServiceStatus, String> {
    manager.status()
}

#[tauri::command]
pub(crate) fn start_local_dify_service(
    port: u16,
    manager: State<'_, LocalDifyServiceManager>,
    paths: State<'_, AppPaths>,
) -> Result<LocalDifyServiceStatus, String> {
    ensure_root(&paths)?;
    manager.start(port, paths.inner().clone())
}

#[tauri::command]
pub(crate) fn stop_local_dify_service(
    manager: State<'_, LocalDifyServiceManager>,
) -> Result<LocalDifyServiceStatus, String> {
    manager.stop()
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| error.to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_owned();
    if method.is_empty() || path.is_empty() {
        return Err("HTTP 请求行无效".to_owned());
    }
    let mut headers = HashMap::new();
    let mut header_bytes = request_line.len();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        header_bytes += line.len();
        if header_bytes > 64 * 1024 {
            return Err("HTTP Header 过大".to_owned());
        }
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    if content_length > MAX_INPUT_BYTES {
        return Err("HTTP 请求体超过 1MB".to_owned());
    }
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn write_json_response(stream: &mut TcpStream, status: &str, value: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .map_err(|error| error.to_string())?;
    stream.write_all(&body).map_err(|error| error.to_string())
}

fn bearer_token(request: &HttpRequest) -> String {
    request
        .headers
        .get("authorization")
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn handle_service_connection(mut stream: TcpStream, paths: &AppPaths) -> Result<(), String> {
    let request = match read_http_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            return write_json_response(
                &mut stream,
                "400 Bad Request",
                &json!({"code": "bad_request", "message": error}),
            );
        }
    };
    if request.method == "OPTIONS" {
        return write_json_response(&mut stream, "200 OK", &json!({"ok": true}));
    }
    if request.method == "GET" && request.path == "/v1/health" {
        return write_json_response(
            &mut stream,
            "200 OK",
            &json!({"status": "ok", "version": "0.1.0"}),
        );
    }
    let app = match find_app_by_token(paths, &bearer_token(&request)) {
        Ok(app) => app,
        Err(error) => {
            return write_json_response(
                &mut stream,
                "401 Unauthorized",
                &json!({"code": "unauthorized", "message": error}),
            );
        }
    };
    if request.method == "GET" && request.path == "/v1/parameters" {
        return write_json_response(
            &mut stream,
            "200 OK",
            &json!({
                "opening_statement": app.opening_statement,
                "user_input_form": [{"paragraph": {"label": app.input_key, "variable": app.input_key, "required": true, "max_length": 1000000}}],
                "file_upload": {"enabled": false},
                "system_parameters": {"image_file_size_limit": 10, "video_file_size_limit": 100, "audio_file_size_limit": 50},
            }),
        );
    }
    let valid_path = matches!(
        request.path.as_str(),
        "/v1/chat-messages" | "/v1/completion-messages" | "/v1/workflows/run"
    );
    if request.method != "POST" || !valid_path {
        return write_json_response(
            &mut stream,
            "404 Not Found",
            &json!({"code": "not_found", "message": "Local Dify API 路由不存在"}),
        );
    }
    let payload: Value = match serde_json::from_slice(&request.body) {
        Ok(value) => value,
        Err(error) => {
            return write_json_response(
                &mut stream,
                "400 Bad Request",
                &json!({"code": "invalid_json", "message": error.to_string()}),
            );
        }
    };
    let inputs: BTreeMap<String, Value> = payload
        .get("inputs")
        .and_then(Value::as_object)
        .map(|items| {
            items
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    let query = payload
        .get("query")
        .and_then(Value::as_str)
        .or_else(|| inputs.get(&app.input_key).and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    let stream_response = payload.get("response_mode").and_then(Value::as_str) == Some("streaming");
    let route = request
        .headers
        .get("x-drpa-provider-route")
        .map(|value| {
            value
                .split(',')
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let run_request = LocalDifyRunRequest {
        request_id: format!("api-{}", Uuid::new_v4().simple()),
        app_id: app.id.clone(),
        query,
        inputs,
        user: payload
            .get("user")
            .and_then(Value::as_str)
            .unwrap_or("api-user")
            .to_owned(),
        stream: stream_response,
        conversation_id: payload
            .get("conversation_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        provider_route: route,
    };
    if stream_response {
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n")
            .map_err(|error| error.to_string())?;
        let result = run_app_internal(paths, run_request, |event| {
            if let LocalDifyStreamEvent::Delta { content } = event {
                let event = json!({"event": "message", "answer": content, "conversation_id": "", "message_id": ""});
                let _ = writeln!(stream, "data: {event}\n");
                let _ = stream.flush();
            }
        });
        match result {
            Ok(result) => {
                writeln!(stream, "data: {}\n", json!({"event": "message_end", "conversation_id": result.conversation_id, "message_id": result.run_id, "metadata": {"usage": result.usage}})).map_err(|error| error.to_string())?;
            }
            Err(error) => {
                writeln!(
                    stream,
                    "data: {}\n",
                    json!({"event": "error", "code": "local_dify_error", "message": error})
                )
                .map_err(|error| error.to_string())?;
            }
        }
        return Ok(());
    }
    match run_app_internal(paths, run_request, |_| {}) {
        Ok(result) if request.path == "/v1/workflows/run" => write_json_response(
            &mut stream,
            "200 OK",
            &json!({"workflow_run_id": result.run_id, "task_id": result.run_id, "data": {"id": result.run_id, "status": "succeeded", "outputs": {"answer": result.answer}, "elapsed_time": result.duration_ms as f64 / 1000.0, "total_tokens": result.usage.total_tokens}}),
        ),
        Ok(result) => write_json_response(
            &mut stream,
            "200 OK",
            &json!({"event": "message", "message_id": result.run_id, "conversation_id": result.conversation_id, "mode": app.mode, "answer": result.answer, "metadata": {"usage": result.usage}, "created_at": now_timestamp()}),
        ),
        Err(error) => write_json_response(
            &mut stream,
            "500 Internal Server Error",
            &json!({"code": "local_dify_error", "message": error}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> AppPaths {
        AppPaths {
            data_root: std::env::temp_dir(),
            workspace_root: std::env::temp_dir()
                .join(format!("drpa-local-dify-test-{}", Uuid::new_v4())),
            resource_dir: None,
        }
    }

    fn provider_input() -> LocalDifyProviderInput {
        LocalDifyProviderInput {
            id: String::new(),
            name: "Test Provider".to_owned(),
            base_url: "http://127.0.0.1:39001/v1".to_owned(),
            model: "test-model".to_owned(),
            context_window: 8_192,
            max_output_tokens: 1_024,
            temperature: 0.2,
            streaming: true,
            supports_tools: true,
            supports_json: true,
            supports_vision: false,
            timeout_seconds: 30,
            custom_headers: BTreeMap::new(),
            dify_provider: "langgenius/openai/openai".to_owned(),
            dify_model: "gpt-4o-mini".to_owned(),
            api_key: "secret".to_owned(),
        }
    }

    fn save_provider_for_test(
        paths: &AppPaths,
        input: LocalDifyProviderInput,
    ) -> LocalDifyProvider {
        let mut providers = Vec::new();
        let id = format!("provider-{}", Uuid::new_v4().simple());
        let provider = LocalDifyProvider {
            id: id.clone(),
            name: input.name,
            base_url: input.base_url,
            model: input.model,
            context_window: input.context_window,
            max_output_tokens: input.max_output_tokens,
            temperature: input.temperature,
            streaming: input.streaming,
            supports_tools: input.supports_tools,
            supports_json: input.supports_json,
            supports_vision: input.supports_vision,
            timeout_seconds: input.timeout_seconds,
            custom_headers: input.custom_headers,
            dify_provider: input.dify_provider,
            dify_model: input.dify_model,
            has_api_key: true,
            updated_at: now_timestamp(),
        };
        providers.push(provider.clone());
        write_json_atomic(&providers_path(paths), &providers).unwrap();
        let mut secrets = LocalDifySecrets::default();
        secrets.provider_api_keys.insert(id, input.api_key);
        write_json_atomic(&secrets_path(paths), &secrets).unwrap();
        provider
    }

    #[test]
    fn local_apps_are_file_backed_and_dsl_round_trips() {
        let paths = test_paths();
        ensure_root(&paths).unwrap();
        let provider = save_provider_for_test(&paths, provider_input());
        let source = r#"version: '0.3.1'
kind: app
app:
  name: Imported Chat
  description: test
  mode: chat
model_config:
  pre_prompt: Be concise.
  model:
    provider: langgenius/openai/openai
    name: gpt-4o-mini
    completion_params:
      temperature: 0.3
      max_tokens: 512
  user_input_form:
    - paragraph:
        variable: question
"#;
        let app = import_dsl_source(source, &paths).unwrap();
        assert_eq!(app.name, "Imported Chat");
        assert_eq!(app.provider_id, provider.id);
        assert_eq!(app.input_key, "question");
        let rendered = render_dify_dsl(&app, Some(&provider)).unwrap();
        assert!(rendered.contains("kind: app"));
        assert!(rendered.contains("Imported Chat"));
        assert!(source_dsl_path(&paths, &app.id).is_file());
        let _ = fs::remove_dir_all(paths.workspace_root);
    }

    #[test]
    fn provider_route_rejects_cycles() {
        let request = LocalDifyRunRequest {
            request_id: "request-test".to_owned(),
            app_id: "app-test".to_owned(),
            query: "hello".to_owned(),
            inputs: BTreeMap::new(),
            user: "tester".to_owned(),
            stream: false,
            conversation_id: String::new(),
            provider_route: vec!["app-test".to_owned()],
        };
        assert!(validate_run_request(&request).unwrap_err().contains("循环"));
    }

    #[test]
    fn workflow_node_factory_supports_rpaz_and_dify_transform_nodes() {
        for kind in [
            "rpaz-package",
            "question-classifier",
            "parameter-extractor",
            "variable-aggregator",
            "list-operator",
            "document-extractor",
            "knowledge-retrieval",
        ] {
            let node = create_local_dify_workflow_node(kind.to_owned(), 20.0, 30.0).unwrap();
            assert_eq!(node.kind, kind);
            assert_ne!(node.title, "结束");
        }
    }

    #[test]
    fn local_transform_nodes_aggregate_filter_extract_and_retrieve() {
        let paths = test_paths();
        ensure_root(&paths).unwrap();
        let request = LocalDifyRunRequest {
            request_id: "request-transforms".to_owned(),
            app_id: "app-transforms".to_owned(),
            query: "DRPA".to_owned(),
            inputs: BTreeMap::new(),
            user: "tester".to_owned(),
            stream: false,
            conversation_id: String::new(),
            provider_route: Vec::new(),
        };
        let outputs = HashMap::from([
            (
                "left".to_owned(),
                BTreeMap::from([("result".to_owned(), Value::Null)]),
            ),
            (
                "right".to_owned(),
                BTreeMap::from([
                    ("result".to_owned(), json!("selected")),
                    (
                        "items".to_owned(),
                        json!([
                            {"name": "b", "score": 1},
                            {"name": "a", "score": 3},
                            {"name": "c", "score": 2}
                        ]),
                    ),
                ]),
            ),
        ]);

        let mut aggregator = local_dify_workflow::new_node("variable-aggregator", 0.0, 0.0);
        aggregator.config.insert(
            "variables".to_owned(),
            json!([["left", "result"], ["right", "result"]]),
        );
        assert_eq!(
            execute_workflow_variable_aggregator(&aggregator, &outputs, &request)["output"],
            json!("selected")
        );

        let mut list = local_dify_workflow::new_node("list-operator", 0.0, 0.0);
        list.config
            .insert("variable".to_owned(), json!(["right", "items"]));
        list.config.insert(
            "filter_by".to_owned(),
            json!({"enabled": true, "conditions": [{"key": "score", "comparison_operator": "greater_than", "value": 1}]}),
        );
        list.config.insert(
            "order_by".to_owned(),
            json!({"enabled": true, "key": "name", "value": "asc"}),
        );
        list.config
            .insert("limit".to_owned(), json!({"enabled": true, "size": 1}));
        let list_result = execute_workflow_list_operator(&list, &outputs, &request);
        assert_eq!(list_result["result"], json!([{"name": "a", "score": 3}]));

        let document_root = paths.workspace_root.join("documents");
        fs::create_dir_all(&document_root).unwrap();
        fs::write(document_root.join("sample.txt"), "document body").unwrap();
        let document_outputs = HashMap::from([(
            "start".to_owned(),
            BTreeMap::from([("file_path".to_owned(), json!("documents/sample.txt"))]),
        )]);
        let document = local_dify_workflow::new_node("document-extractor", 0.0, 0.0);
        assert_eq!(
            execute_workflow_document_extractor(&paths, &document, &document_outputs, &request)
                .unwrap()["text"],
            json!("document body")
        );

        let knowledge_root = paths.workspace_root.join("knowledge");
        fs::create_dir_all(&knowledge_root).unwrap();
        fs::write(
            knowledge_root.join("guide.md"),
            "# DRPA Guide\nDRPA workflow knowledge.",
        )
        .unwrap();
        let knowledge = local_dify_workflow::new_node("knowledge-retrieval", 0.0, 0.0);
        let knowledge_result =
            execute_workflow_knowledge_retrieval(&paths, &knowledge, &HashMap::new(), &request)
                .unwrap();
        assert_eq!(knowledge_result["result"].as_array().unwrap().len(), 1);
        assert!(knowledge_result["text"].as_str().unwrap().contains("DRPA"));
        let _ = fs::remove_dir_all(paths.workspace_root);
    }

    #[test]
    fn runtime_database_persists_run_history() {
        let paths = test_paths();
        ensure_root(&paths).unwrap();
        let provider = save_provider_for_test(&paths, provider_input());
        let now = now_timestamp();
        let app = LocalDifyApp {
            schema: 1,
            id: "app-history".to_owned(),
            name: "History".to_owned(),
            description: String::new(),
            mode: "chat".to_owned(),
            provider_id: provider.id.clone(),
            system_prompt: String::new(),
            opening_statement: String::new(),
            input_key: "query".to_owned(),
            temperature: 0.2,
            max_output_tokens: 512,
            workflow: WorkflowGraph::default(),
            published_version: 0,
            api_enabled: false,
            created_at: now,
            updated_at: now,
        };
        record_run(
            &paths,
            "dify-run-test",
            &app,
            "success",
            "hello",
            "world",
            &provider,
            &LocalDifyUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
            },
            12,
            "",
        )
        .unwrap();
        let connection = open_runtime_database(&paths).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM local_dify_runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let _ = fs::remove_dir_all(paths.workspace_root);
    }

    #[test]
    fn workflow_dsl_imports_and_exports_graph_nodes() {
        let paths = test_paths();
        ensure_root(&paths).unwrap();
        let provider = save_provider_for_test(&paths, provider_input());
        let source = r#"version: '0.3.1'
kind: app
app:
  name: Imported Workflow
  description: workflow test
  mode: workflow
workflow:
  graph:
    viewport: {x: 20, y: 30, zoom: 1}
    nodes:
      - id: start
        position: {x: 80, y: 120}
        data:
          type: start
          title: Start
          variables:
            - {label: query, variable: query, type: paragraph, required: true}
      - id: template
        position: {x: 380, y: 120}
        data:
          type: template-transform
          title: Template
          template: 'Hello {{#start.query#}}'
      - id: end
        position: {x: 680, y: 120}
        data:
          type: end
          title: End
          outputs:
            - {variable: answer, value_selector: [template, output]}
    edges:
      - {id: edge-start-template, source: start, target: template}
      - {id: edge-template-end, source: template, target: end}
"#;
        let app = import_dsl_source(source, &paths).unwrap();
        assert_eq!(app.mode, "workflow");
        assert_eq!(app.workflow.nodes.len(), 3);
        assert_eq!(app.input_key, "query");
        let rendered = render_dify_dsl(&app, Some(&provider)).unwrap();
        assert!(rendered.contains("template-transform"));
        assert!(rendered.contains("graph:"));
        assert!(rendered.contains("edge-start-template"));
        let _ = fs::remove_dir_all(paths.workspace_root);
    }

    #[test]
    fn workflow_executor_runs_template_and_end_nodes_locally() {
        let paths = test_paths();
        ensure_root(&paths).unwrap();
        let now = now_timestamp();
        let mut graph = default_graph("workflow", "query");
        graph.nodes.retain(|node| node.kind != "llm");
        let mut template = local_dify_workflow::new_node("template-transform", 360.0, 210.0);
        template.id = "template".to_owned();
        template
            .config
            .insert("template".to_owned(), json!("Hello {{#start.query#}}"));
        let end = graph
            .nodes
            .iter_mut()
            .find(|node| node.kind == "end")
            .unwrap();
        end.config.insert(
            "outputs".to_owned(),
            json!([{"variable": "answer", "value_selector": ["template", "output"]}]),
        );
        graph.nodes.insert(1, template);
        graph.edges = vec![
            local_dify_workflow::WorkflowEdge {
                id: "edge-start-template".to_owned(),
                source: "start".to_owned(),
                target: "template".to_owned(),
                source_handle: "source".to_owned(),
                target_handle: "target".to_owned(),
                label: String::new(),
                data: BTreeMap::new(),
            },
            local_dify_workflow::WorkflowEdge {
                id: "edge-template-end".to_owned(),
                source: "template".to_owned(),
                target: "end".to_owned(),
                source_handle: "source".to_owned(),
                target_handle: "target".to_owned(),
                label: String::new(),
                data: BTreeMap::new(),
            },
        ];
        let app = LocalDifyApp {
            schema: LOCAL_DIFY_SCHEMA,
            id: "app-workflow".to_owned(),
            name: "Workflow".to_owned(),
            description: String::new(),
            mode: "workflow".to_owned(),
            provider_id: String::new(),
            system_prompt: String::new(),
            opening_statement: String::new(),
            input_key: "query".to_owned(),
            temperature: 0.2,
            max_output_tokens: 512,
            workflow: graph,
            published_version: 0,
            api_enabled: false,
            created_at: now,
            updated_at: now,
        };
        write_json_atomic(&app_path(&paths, &app.id), &app).unwrap();
        let request = LocalDifyRunRequest {
            request_id: "request-workflow".to_owned(),
            app_id: app.id,
            query: "DRPA".to_owned(),
            inputs: BTreeMap::new(),
            user: "test".to_owned(),
            stream: false,
            conversation_id: String::new(),
            provider_route: Vec::new(),
        };
        let mut events = Vec::new();
        let result = run_app_internal(&paths, request, |event| events.push(event)).unwrap();
        assert_eq!(result.answer, "Hello DRPA");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, LocalDifyStreamEvent::NodeCompleted { .. }))
                .count(),
            3
        );
        let _ = fs::remove_dir_all(paths.workspace_root);
    }

    #[test]
    fn local_service_executes_a_published_app_through_openai_provider() {
        let provider_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let provider_port = provider_listener.local_addr().unwrap().port();
        let provider_thread = thread::spawn(move || {
            let (mut stream, _) = provider_listener.accept().unwrap();
            let request = read_http_request(&mut stream).unwrap();
            assert_eq!(request.path, "/v1/chat/completions");
            assert_eq!(
                request.headers.get("x-drpa-provider-route").unwrap(),
                "app-service"
            );
            write_json_response(
                &mut stream,
                "200 OK",
                &json!({
                    "choices": [{"message": {"role": "assistant", "content": "service ok"}}],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
                }),
            )
            .unwrap();
        });

        let paths = test_paths();
        ensure_root(&paths).unwrap();
        let mut input = provider_input();
        input.base_url = format!("http://127.0.0.1:{provider_port}/v1");
        input.streaming = false;
        let provider = save_provider_for_test(&paths, input);
        let now = now_timestamp();
        let app = LocalDifyApp {
            schema: 1,
            id: "app-service".to_owned(),
            name: "Service App".to_owned(),
            description: String::new(),
            mode: "chat".to_owned(),
            provider_id: provider.id,
            system_prompt: "Be concise".to_owned(),
            opening_statement: String::new(),
            input_key: "query".to_owned(),
            temperature: 0.2,
            max_output_tokens: 512,
            workflow: WorkflowGraph::default(),
            published_version: 1,
            api_enabled: true,
            created_at: now,
            updated_at: now,
        };
        write_json_atomic(&app_path(&paths, &app.id), &app).unwrap();
        let mut secrets: LocalDifySecrets = read_json_or_default(&secrets_path(&paths)).unwrap();
        secrets
            .app_api_tokens
            .insert(app.id.clone(), "app-service-token".to_owned());
        write_json_atomic(&secrets_path(&paths), &secrets).unwrap();

        let port_probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let service_port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        let manager = LocalDifyServiceManager::default();
        manager.start(service_port, paths.clone()).unwrap();
        let mut response = ureq::post(format!("http://127.0.0.1:{service_port}/v1/chat-messages"))
            .header("Authorization", "Bearer app-service-token")
            .send_json(json!({
                "inputs": {},
                "query": "hello",
                "response_mode": "blocking",
                "user": "test-user"
            }))
            .unwrap();
        let body: Value = response.body_mut().read_json().unwrap();
        assert_eq!(body["answer"], "service ok");
        assert_eq!(body["metadata"]["usage"]["totalTokens"], 5);
        manager.stop().unwrap();
        provider_thread.join().unwrap();
        let connection = open_runtime_database(&paths).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM local_dify_runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let _ = fs::remove_dir_all(paths.workspace_root);
    }
}
