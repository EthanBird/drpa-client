use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::{AppPaths, agent};

const LOCAL_DIFY_SCHEMA: u32 = 1;
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
    Started { run_id: String },
    Delta { content: String },
    Completed { run_id: String },
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
    128_000
}

const fn default_max_output_tokens() -> u32 {
    4_096
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
            base_url: "https://api.openai.com/v1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
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
            dify_model: "gpt-4o-mini".to_owned(),
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
    validate_identifier(&app.id, "AI 应用")?;
    if app.name.trim().is_empty() || app.name.chars().count() > 100 {
        return Err("AI 应用名称应为 1 到 100 个字符".to_owned());
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
    agent::chat_completions_endpoint(&input.base_url)?;
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
    validate_identifier(app_id, "AI 应用")?;
    let bytes =
        fs::read(app_path(paths, app_id)).map_err(|error| format!("读取 AI 应用失败：{error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("AI 应用配置无效：{error}"))
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
            apps.push(app);
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
        return Err(format!("本地 AI 应用最多保留 {MAX_APPS} 个"));
    }
    let mode = input.mode.trim().to_owned();
    let now = now_timestamp();
    let provider_id = load_providers(&paths)?
        .first()
        .map(|item| item.id.clone())
        .unwrap_or_default();
    let app = LocalDifyApp {
        schema: LOCAL_DIFY_SCHEMA,
        id: format!("app-{}", Uuid::new_v4().simple()),
        name: input.name.trim().to_owned(),
        description: "用于本地测试与 Dify DSL 导出的 AI 应用。".to_owned(),
        mode,
        provider_id,
        system_prompt: "你是一个准确、简洁的 AI 助手。".to_owned(),
        opening_statement: "你好，我是本地 AI 应用。".to_owned(),
        input_key: "query".to_owned(),
        temperature: default_temperature(),
        max_output_tokens: default_max_output_tokens(),
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
    validate_app(&app)?;
    if !app.provider_id.is_empty() {
        let _ = load_provider(&paths, &app.provider_id)?;
    }
    write_json_atomic(&app_path(&paths, &app.id), &app)?;
    Ok(app)
}

#[tauri::command]
pub(crate) fn delete_local_dify_app(
    app_id: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_identifier(&app_id, "AI 应用")?;
    let root = apps_root(&paths).join(&app_id);
    if !root.is_dir() {
        return Err("AI 应用不存在".to_owned());
    }
    fs::remove_dir_all(root).map_err(|error| format!("删除 AI 应用失败：{error}"))?;
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
        dify_provider: input.dify_provider.trim().to_owned(),
        dify_model: input.dify_model.trim().to_owned(),
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
        return Err("该 Provider 正被 AI 应用使用".to_owned());
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
    .map_err(|error| format!("AI 应用执行任务异常：{error}"))?
}

fn validate_run_request(request: &LocalDifyRunRequest) -> Result<(), String> {
    run_stream_event_name(&request.request_id)?;
    validate_identifier(&request.app_id, "AI 应用")?;
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
    if !matches!(app.mode.as_str(), "chat" | "completion") {
        return Err("当前阶段只执行 Chat 与 Completion 应用".to_owned());
    }
    if app.provider_id.is_empty() {
        return Err("请先为应用选择 Provider".to_owned());
    }
    let (provider, api_key) = load_provider(paths, &app.provider_id)?;
    let run_id = format!("dify-run-{}", Uuid::new_v4().simple());
    emit(LocalDifyStreamEvent::Started {
        run_id: run_id.clone(),
    });
    let started = Instant::now();
    let query = request.query.trim().to_owned();
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
    let mut route = request.provider_route.clone();
    route.push(app.id.clone());
    let completed = call_provider(&provider, &api_key, &payload, &route, |content| {
        emit(LocalDifyStreamEvent::Delta { content });
    });
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
    let endpoint = agent::chat_completions_endpoint(&provider.base_url)?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(provider.timeout_seconds)))
        .build();
    let http = ureq::Agent::new_with_config(config);
    let mut request = http
        .post(&endpoint)
        .header("User-Agent", "DRPA-Local-Dify/0.1")
        .header(
            "X-DRPA-Trace-Id",
            &format!("trace-{}", Uuid::new_v4().simple()),
        )
        .header("X-DRPA-Hop-Count", &route.len().to_string())
        .header("X-DRPA-Provider-Route", &route.join(","));
    if payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        request = request.header("Accept", "text/event-stream");
    } else {
        request = request.header("Accept", "application/json");
    }
    if !api_key.trim().is_empty() {
        request = request.header("Authorization", &format!("Bearer {}", api_key.trim()));
    }
    for (name, value) in &provider.custom_headers {
        request = request.header(name, value);
    }
    let mut response = request
        .send_json(payload)
        .map_err(|error| format!("Provider 请求失败：{error}"))?;
    if payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        parse_provider_stream(BufReader::new(response.body_mut().as_reader()), on_delta)
    } else {
        let value = response
            .body_mut()
            .read_json::<Value>()
            .map_err(|error| format!("Provider JSON 响应无效：{error}"))?;
        provider_completion_from_json(&value)
    }
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

fn parse_provider_stream<R, F>(reader: R, mut on_delta: F) -> Result<ProviderCompletion, String>
where
    R: BufRead,
    F: FnMut(String),
{
    let mut answer = String::new();
    let mut usage = LocalDifyUsage::default();
    let mut event_data = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("读取 Provider 流失败：{error}"))?;
        if line.trim().is_empty() {
            consume_provider_stream_event(
                &event_data.join("\n"),
                &mut answer,
                &mut usage,
                &mut on_delta,
            )?;
            event_data.clear();
        } else if let Some(data) = line.strip_prefix("data:") {
            event_data.push(data.trim_start().to_owned());
        }
    }
    if !event_data.is_empty() {
        consume_provider_stream_event(
            &event_data.join("\n"),
            &mut answer,
            &mut usage,
            &mut on_delta,
        )?;
    }
    if answer.trim().is_empty() {
        return Err("Provider 流式响应没有消息内容".to_owned());
    }
    Ok(ProviderCompletion { answer, usage })
}

fn consume_provider_stream_event<F>(
    data: &str,
    answer: &mut String,
    usage: &mut LocalDifyUsage,
    on_delta: &mut F,
) -> Result<(), String>
where
    F: FnMut(String),
{
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let value: Value =
        serde_json::from_str(data).map_err(|error| format!("Provider SSE 数据无效：{error}"))?;
    if let Some(chunk) = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("answer").and_then(Value::as_str))
    {
        answer.push_str(chunk);
        on_delta(chunk.to_owned());
    }
    if let Some(parsed) = value
        .get("usage")
        .or_else(|| value.pointer("/metadata/usage"))
    {
        *usage = parse_usage(Some(parsed));
    }
    Ok(())
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
        .map_err(|error| format!("记录 AI 应用运行失败：{error}"))?;
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
        validate_identifier(app_id, "AI 应用")?;
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
    if !matches!(app.mode.as_str(), "chat" | "completion") {
        issues.push(DifyCompatibilityIssue {
            level: "error".to_owned(),
            code: "mode-not-executable".to_owned(),
            message: "当前运行时仅执行 Chat 与 Completion；Workflow 画布将在下一阶段接入。"
                .to_owned(),
        });
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
        .unwrap_or("gpt-4o-mini");
    let value = json!({
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
        "model_config": {
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
        },
    });
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
    let model_name = model_config
        .pointer("/model/name")
        .and_then(Value::as_str)
        .unwrap_or("gpt-4o-mini");
    let provider_name = model_config
        .pointer("/model/provider")
        .and_then(Value::as_str)
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
            .unwrap_or_default()
            .to_owned(),
        input_key: extract_input_key(&model_config),
        temperature: model_config
            .pointer("/model/completion_params/temperature")
            .and_then(Value::as_f64)
            .unwrap_or(0.2) as f32,
        max_output_tokens: model_config
            .pointer("/model/completion_params/max_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(4_096)
            .clamp(64, 131_072) as u32,
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
                let _ = writeln!(stream, "data: {}\n", event);
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
