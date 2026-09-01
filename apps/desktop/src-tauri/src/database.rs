use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use calamine::{Data, Reader, open_workbook_auto};
use futures_util::TryStreamExt;
use rusqlite::types::Value as SqliteValue;
use rusqlite::types::ValueRef as SqliteValueRef;
use rusqlite::{Connection, OpenFlags, params, params_from_iter};
use serde::{Deserialize, Serialize};
use sqlx::any::{AnyPoolOptions, AnyRow};
use sqlx::{AnyConnection, AnyPool, Column, Row, TypeInfo, ValueRef as SqlxValueRef};
use tauri::State;
use url::Url;
use uuid::Uuid;

use crate::AppPaths;

const RESULT_ROW_LIMIT: usize = 1_000;
const MAX_QUERY_PAGE_SIZE: usize = 20_000;
const RESULT_BYTE_LIMIT: usize = 64 * 1024 * 1024;
const CELL_BYTE_LIMIT: usize = 256 * 1024;
const SCHEMA_CONTEXT_BYTE_LIMIT: usize = 100 * 1024;
const REMOTE_CONNECTION_LIMIT: usize = 50;
const REMOTE_TABLE_LIMIT: usize = 2_000;
const SCHEMA_TABLE_LIMIT: usize = 200;
const EXCEL_FILE_BYTE_LIMIT: u64 = 100 * 1024 * 1024;
const EXCEL_SHEET_LIMIT: usize = 100;
const EXCEL_ROW_LIMIT: usize = 100_000;
const EXCEL_COLUMN_LIMIT: usize = 512;
const TABULAR_FILE_BYTE_LIMIT: u64 = 256 * 1024 * 1024;
const TABULAR_ROW_LIMIT: usize = 1_000_000;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteDatabaseProfile {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) engine: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) database: String,
    pub(crate) username: String,
    pub(crate) tls_mode: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteConnectionTest {
    server_version: String,
    latency_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseInfo {
    name: String,
    engine: &'static str,
    path: String,
    size_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseTable {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    row_count: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseColumn {
    ordinal: i64,
    name: String,
    data_type: String,
    not_null: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_value: Option<String>,
    primary_key: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseQueryResult {
    columns: Vec<String>,
    pub(crate) rows: Vec<Vec<serde_json::Value>>,
    affected_rows: usize,
    duration_ms: u64,
    truncated: bool,
    statement_type: String,
    offset: usize,
    limit: usize,
    has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseExportResult {
    path: String,
    format: String,
    row_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct QueryWindow {
    offset: usize,
    limit: usize,
}

impl Default for QueryWindow {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: RESULT_ROW_LIMIT,
        }
    }
}

impl QueryWindow {
    fn from_request(offset: Option<usize>, limit: Option<usize>) -> Self {
        Self {
            offset: offset.unwrap_or(0),
            limit: limit
                .unwrap_or(RESULT_ROW_LIMIT)
                .clamp(1, MAX_QUERY_PAGE_SIZE),
        }
    }
}

#[tauri::command(async)]
pub(crate) fn get_workspace_database_info(
    paths: State<'_, AppPaths>,
) -> Result<DatabaseInfo, String> {
    let path = workspace_database_path(&paths);
    let _ = open_database(&path)?;
    let size_bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(DatabaseInfo {
        name: "工作区数据库".to_owned(),
        engine: "SQLite",
        path: path.to_string_lossy().into_owned(),
        size_bytes,
    })
}

#[tauri::command(async)]
pub(crate) fn list_database_tables(
    paths: State<'_, AppPaths>,
) -> Result<Vec<DatabaseTable>, String> {
    list_tables_at(&workspace_database_path(&paths))
}

#[tauri::command(async)]
pub(crate) fn describe_database_table(
    table_name: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<DatabaseColumn>, String> {
    describe_table_at(&workspace_database_path(&paths), &table_name)
}

#[tauri::command(async)]
pub(crate) fn execute_database_sql(
    sql: String,
    offset: Option<usize>,
    limit: Option<usize>,
    paths: State<'_, AppPaths>,
) -> Result<DatabaseQueryResult, String> {
    execute_sql_at_window(
        &workspace_database_path(&paths),
        &sql,
        QueryWindow::from_request(offset, limit),
    )
}

#[tauri::command(async)]
pub(crate) fn export_database_query_result(
    result: DatabaseQueryResult,
    format: String,
    target_path: String,
    table_name: String,
) -> Result<DatabaseExportResult, String> {
    export_query_result(&result, &format, Path::new(&target_path), &table_name)?;
    Ok(DatabaseExportResult {
        path: target_path,
        format: format.to_ascii_lowercase(),
        row_count: result.rows.len(),
    })
}

#[tauri::command(async)]
pub(crate) fn get_database_schema_context(paths: State<'_, AppPaths>) -> Result<String, String> {
    schema_context_at(&workspace_database_path(&paths))
}

#[tauri::command(async)]
pub(crate) fn list_remote_database_profiles(
    paths: State<'_, AppPaths>,
) -> Result<Vec<RemoteDatabaseProfile>, String> {
    load_remote_profiles(&remote_profiles_path(&paths))
}

#[tauri::command(async)]
pub(crate) fn save_remote_database_profile(
    profile: RemoteDatabaseProfile,
    paths: State<'_, AppPaths>,
) -> Result<RemoteDatabaseProfile, String> {
    save_remote_profile_at(&paths.workspace_root, profile)
}

pub(crate) fn agent_list_database_profiles(
    workspace_root: &Path,
) -> Result<Vec<RemoteDatabaseProfile>, String> {
    load_remote_profiles(&remote_profiles_path_at(workspace_root))
}

pub(crate) fn agent_save_database_profile(
    workspace_root: &Path,
    profile: RemoteDatabaseProfile,
) -> Result<RemoteDatabaseProfile, String> {
    save_remote_profile_at(workspace_root, profile)
}

fn save_remote_profile_at(
    workspace_root: &Path,
    mut profile: RemoteDatabaseProfile,
) -> Result<RemoteDatabaseProfile, String> {
    if profile.id.trim().is_empty() {
        profile.id = format!("database-{}", Uuid::new_v4().simple());
    }
    validate_remote_profile(&profile)?;
    let path = remote_profiles_path_at(workspace_root);
    let mut profiles = load_remote_profiles(&path)?;
    if let Some(existing) = profiles.iter_mut().find(|item| item.id == profile.id) {
        *existing = profile.clone();
    } else {
        if profiles.len() >= REMOTE_CONNECTION_LIMIT {
            return Err(format!(
                "远程数据库连接最多保留 {REMOTE_CONNECTION_LIMIT} 个"
            ));
        }
        profiles.push(profile.clone());
    }
    profiles.sort_by_key(|profile| profile.name.to_lowercase());
    write_remote_profiles(&path, &profiles)?;
    Ok(profile)
}

pub(crate) async fn agent_get_database_schema(
    workspace_root: &Path,
    profile_id: &str,
    password: &str,
) -> Result<String, String> {
    if profile_id == "workspace" {
        return schema_context_read_only_at(&workspace_database_path_at(workspace_root));
    }
    let profile = load_remote_profile(&remote_profiles_path_at(workspace_root), profile_id)?;
    if profile.engine == "sqlite" {
        let connection = open_external_database_read_only(Path::new(&profile.database))?;
        return schema_context_with_connection(&connection, "SQLite 外部数据库结构");
    }
    if is_tabular_file_engine(&profile.engine) {
        let connection = open_tabular_file_as_sqlite(&profile)?;
        return schema_context_with_connection(&connection, tabular_schema_label(&profile.engine));
    }
    let pool = connect_remote_database(&profile, password).await?;
    let result = remote_schema_context_with_pool(&profile, &pool).await;
    pool.close().await;
    result
}

pub(crate) async fn agent_execute_read_only_query(
    workspace_root: &Path,
    profile_id: &str,
    password: &str,
    sql: &str,
) -> Result<DatabaseQueryResult, String> {
    ensure_read_only_sql(sql)?;
    if profile_id == "workspace" {
        let connection = open_database_read_only(&workspace_database_path_at(workspace_root))?;
        return execute_sql_with_connection_mode(&connection, sql, true);
    }
    let profile = load_remote_profile(&remote_profiles_path_at(workspace_root), profile_id)?;
    if profile.engine == "sqlite" {
        let connection = open_external_database_read_only(Path::new(&profile.database))?;
        return execute_sql_with_connection_mode(&connection, sql, true);
    }
    if is_tabular_file_engine(&profile.engine) {
        let connection = open_tabular_file_as_sqlite(&profile)?;
        return execute_sql_with_connection_mode(&connection, sql, true);
    }
    let pool = connect_remote_database(&profile, password).await?;
    let result = execute_remote_read_only_with_pool(&profile, &pool, sql).await;
    pool.close().await;
    result
}

#[tauri::command]
pub(crate) async fn execute_dashboard_database_query(
    profile_id: String,
    password: String,
    sql: String,
    paths: State<'_, AppPaths>,
) -> Result<DatabaseQueryResult, String> {
    agent_execute_read_only_query(&paths.workspace_root, &profile_id, &password, &sql).await
}

#[tauri::command(async)]
pub(crate) fn delete_remote_database_profile(
    profile_id: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_profile_id(&profile_id)?;
    let path = remote_profiles_path(&paths);
    let mut profiles = load_remote_profiles(&path)?;
    let previous_len = profiles.len();
    profiles.retain(|profile| profile.id != profile_id);
    if profiles.len() == previous_len {
        return Err("数据库连接不存在".to_owned());
    }
    write_remote_profiles(&path, &profiles)
}

#[tauri::command]
pub(crate) async fn test_remote_database_connection(
    profile: RemoteDatabaseProfile,
    password: String,
) -> Result<RemoteConnectionTest, String> {
    validate_remote_profile(&profile)?;
    let started = Instant::now();
    if profile.engine == "sqlite" {
        let connection = open_external_database(Path::new(&profile.database))?;
        let version = connection
            .query_row("SELECT sqlite_version()", [], |row| row.get::<_, String>(0))
            .map_err(|error| format!("读取 SQLite 版本失败：{error}"))?;
        return Ok(RemoteConnectionTest {
            server_version: format!("SQLite {version}"),
            latency_ms: started.elapsed().as_millis() as u64,
        });
    }
    if is_tabular_file_engine(&profile.engine) {
        let connection = open_tabular_file_as_sqlite(&profile)?;
        let table_count = list_tables_with_connection(&connection)?.len();
        return Ok(RemoteConnectionTest {
            server_version: format!(
                "{} · {table_count} 张表（只读）",
                database_engine_name(&profile.engine)
            ),
            latency_ms: started.elapsed().as_millis() as u64,
        });
    }
    let pool = connect_remote_database(&profile, &password).await?;
    let version_sql = match profile.engine.as_str() {
        "postgresql" => "SELECT version()",
        "mysql" | "mariadb" => "SELECT VERSION()",
        _ => return Err("远程数据库类型无效".to_owned()),
    };
    let version = sqlx::query_scalar::<_, String>(version_sql)
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("读取数据库版本失败：{error}"))?;
    pool.close().await;
    Ok(RemoteConnectionTest {
        server_version: version,
        latency_ms: started.elapsed().as_millis() as u64,
    })
}

#[tauri::command]
pub(crate) async fn list_remote_database_tables(
    profile_id: String,
    password: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<DatabaseTable>, String> {
    let profile = load_remote_profile(&remote_profiles_path(&paths), &profile_id)?;
    if profile.engine == "sqlite" {
        return list_tables_external(Path::new(&profile.database));
    }
    if is_tabular_file_engine(&profile.engine) {
        let connection = open_tabular_file_as_sqlite(&profile)?;
        return list_tables_with_connection(&connection);
    }
    let pool = connect_remote_database(&profile, &password).await?;
    let result = list_remote_tables_with_pool(&profile, &pool).await;
    pool.close().await;
    result
}

#[tauri::command]
pub(crate) async fn describe_remote_database_table(
    profile_id: String,
    password: String,
    table_name: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<DatabaseColumn>, String> {
    let profile = load_remote_profile(&remote_profiles_path(&paths), &profile_id)?;
    if profile.engine == "sqlite" {
        return describe_table_external(Path::new(&profile.database), &table_name);
    }
    if is_tabular_file_engine(&profile.engine) {
        let connection = open_tabular_file_as_sqlite(&profile)?;
        return describe_table_with_connection(&connection, &table_name);
    }
    let pool = connect_remote_database(&profile, &password).await?;
    let result = describe_remote_table_with_pool(&profile, &pool, &table_name).await;
    pool.close().await;
    result
}

#[tauri::command]
pub(crate) async fn execute_remote_database_sql(
    profile_id: String,
    password: String,
    sql: String,
    offset: Option<usize>,
    limit: Option<usize>,
    paths: State<'_, AppPaths>,
) -> Result<DatabaseQueryResult, String> {
    let window = QueryWindow::from_request(offset, limit);
    let profile = load_remote_profile(&remote_profiles_path(&paths), &profile_id)?;
    if profile.engine == "sqlite" {
        return execute_sql_external_window(Path::new(&profile.database), &sql, window);
    }
    if is_tabular_file_engine(&profile.engine) {
        let statement_type = first_sql_keyword(&sql);
        if !matches!(
            statement_type.as_str(),
            "SELECT" | "WITH" | "EXPLAIN" | "PRAGMA"
        ) {
            return Err(format!(
                "{} 是只读数据源，仅支持 SELECT、WITH、EXPLAIN 或 PRAGMA 查询",
                database_engine_name(&profile.engine)
            ));
        }
        let connection = open_tabular_file_as_sqlite(&profile)?;
        return execute_sql_with_connection_window(&connection, &sql, window);
    }
    let pool = connect_remote_database(&profile, &password).await?;
    let result = execute_remote_sql_with_pool(&pool, &sql, window).await;
    pool.close().await;
    result
}

#[tauri::command]
pub(crate) async fn get_remote_database_schema_context(
    profile_id: String,
    password: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    let profile = load_remote_profile(&remote_profiles_path(&paths), &profile_id)?;
    if profile.engine == "sqlite" {
        return schema_context_external(Path::new(&profile.database), "SQLite 外部数据库结构");
    }
    if is_tabular_file_engine(&profile.engine) {
        let connection = open_tabular_file_as_sqlite(&profile)?;
        return schema_context_with_connection(&connection, tabular_schema_label(&profile.engine));
    }
    let pool = connect_remote_database(&profile, &password).await?;
    let result = remote_schema_context_with_pool(&profile, &pool).await;
    pool.close().await;
    result
}

#[tauri::command(async)]
pub(crate) fn open_workspace_database_directory(paths: State<'_, AppPaths>) -> Result<(), String> {
    let directory = workspace_database_path(&paths)
        .parent()
        .ok_or_else(|| "数据库目录无效".to_owned())?
        .to_path_buf();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    crate::open_directory_in_file_explorer(&directory)
}

fn remote_profiles_path(paths: &AppPaths) -> PathBuf {
    remote_profiles_path_at(&paths.workspace_root)
}

fn remote_profiles_path_at(workspace_root: &Path) -> PathBuf {
    workspace_root.join("databases").join("connections.json")
}

fn load_remote_profiles(path: &Path) -> Result<Vec<RemoteDatabaseProfile>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取数据库连接配置失败：{error}"))?;
    let profiles = serde_json::from_str::<Vec<RemoteDatabaseProfile>>(&content)
        .map_err(|error| format!("数据库连接配置格式无效：{error}"))?;
    if profiles.len() > REMOTE_CONNECTION_LIMIT {
        return Err("数据库连接配置数量超出限制".to_owned());
    }
    for profile in &profiles {
        validate_remote_profile(profile)?;
    }
    Ok(profiles)
}

fn write_remote_profiles(path: &Path, profiles: &[RemoteDatabaseProfile]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(profiles)
        .map_err(|error| format!("序列化数据库连接配置失败：{error}"))?;
    fs::write(path, format!("{content}\n"))
        .map_err(|error| format!("保存数据库连接配置失败：{error}"))
}

fn load_remote_profile(path: &Path, profile_id: &str) -> Result<RemoteDatabaseProfile, String> {
    validate_profile_id(profile_id)?;
    load_remote_profiles(path)?
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "数据库连接不存在".to_owned())
}

fn validate_profile_id(profile_id: &str) -> Result<(), String> {
    if profile_id.len() < 4
        || profile_id.len() > 96
        || !profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("数据库连接标识无效".to_owned());
    }
    Ok(())
}

fn validate_remote_profile(profile: &RemoteDatabaseProfile) -> Result<(), String> {
    validate_profile_id(&profile.id)?;
    if profile.name.trim().is_empty() || profile.name.chars().count() > 80 {
        return Err("连接名称应为 1 到 80 个字符".to_owned());
    }
    if !matches!(
        profile.engine.as_str(),
        "postgresql" | "mysql" | "mariadb" | "sqlite" | "excel" | "csv" | "json"
    ) {
        return Err(
            "仅支持 PostgreSQL、MySQL、MariaDB、SQLite、Excel、CSV/TSV 或 JSON/JSONL 数据源"
                .to_owned(),
        );
    }
    if profile.engine == "sqlite" || is_tabular_file_engine(&profile.engine) {
        let source = profile.database.trim();
        if source.is_empty() || source.len() > 32_767 {
            return Err("数据源文件路径无效".to_owned());
        }
        let extension = Path::new(source)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let valid_extension = match profile.engine.as_str() {
            "sqlite" => matches!(extension.as_str(), "db" | "sqlite" | "sqlite3"),
            "excel" => matches!(extension.as_str(), "xls" | "xlsx" | "xlsb" | "ods"),
            "csv" => matches!(extension.as_str(), "csv" | "tsv"),
            "json" => matches!(extension.as_str(), "json" | "jsonl" | "ndjson"),
            _ => false,
        };
        if !valid_extension {
            return Err(match profile.engine.as_str() {
                "sqlite" => "SQLite 文件应使用 .db、.sqlite 或 .sqlite3 扩展名".to_owned(),
                "excel" => "工作簿应使用 .xls、.xlsx、.xlsb 或 .ods 扩展名".to_owned(),
                "csv" => "分隔文本应使用 .csv 或 .tsv 扩展名".to_owned(),
                "json" => "JSON 数据应使用 .json、.jsonl 或 .ndjson 扩展名".to_owned(),
                _ => unreachable!(),
            });
        }
        return Ok(());
    }
    if profile.host.trim().is_empty()
        || profile.host.len() > 253
        || profile.host.contains(char::is_whitespace)
        || profile.host.contains(['/', '\\', '@'])
    {
        return Err("数据库主机地址无效".to_owned());
    }
    if profile.port == 0 {
        return Err("数据库端口无效".to_owned());
    }
    if profile.database.trim().is_empty() || profile.database.len() > 128 {
        return Err("数据库名称无效".to_owned());
    }
    if profile.username.trim().is_empty() || profile.username.len() > 128 {
        return Err("数据库用户名无效".to_owned());
    }
    if !matches!(profile.tls_mode.as_str(), "disable" | "prefer" | "require") {
        return Err("TLS 模式无效".to_owned());
    }
    Ok(())
}

fn remote_connection_url(
    profile: &RemoteDatabaseProfile,
    password: &str,
) -> Result<String, String> {
    validate_remote_profile(profile)?;
    let scheme = match profile.engine.as_str() {
        "postgresql" => "postgresql",
        "mysql" | "mariadb" => "mysql",
        _ => return Err("远程数据库类型无效".to_owned()),
    };
    let mut url = Url::parse(&format!("{scheme}://localhost"))
        .map_err(|error| format!("创建数据库连接地址失败：{error}"))?;
    url.set_username(&profile.username)
        .map_err(|_| "数据库用户名无法编码".to_owned())?;
    url.set_password(Some(password))
        .map_err(|_| "数据库密码无法编码".to_owned())?;
    url.set_host(Some(&profile.host))
        .map_err(|error| format!("数据库主机地址无效：{error}"))?;
    url.set_port(Some(profile.port))
        .map_err(|_| "数据库端口无效".to_owned())?;
    url.set_path(&format!("/{}", profile.database));
    let (key, value) = match profile.engine.as_str() {
        "postgresql" => ("sslmode", profile.tls_mode.as_str()),
        "mysql" | "mariadb" => (
            "ssl-mode",
            match profile.tls_mode.as_str() {
                "disable" => "disabled",
                "prefer" => "preferred",
                "require" => "required",
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };
    url.query_pairs_mut().append_pair(key, value);
    Ok(url.into())
}

async fn connect_remote_database(
    profile: &RemoteDatabaseProfile,
    password: &str,
) -> Result<AnyPool, String> {
    sqlx::any::install_default_drivers();
    AnyPoolOptions::new()
        .max_connections(3)
        .acquire_timeout(std::time::Duration::from_secs(15))
        .idle_timeout(std::time::Duration::from_secs(30))
        .connect(&remote_connection_url(profile, password)?)
        .await
        .map_err(|error| format!("连接 {} 失败：{error}", profile.name))
}

async fn list_remote_tables_with_pool(
    profile: &RemoteDatabaseProfile,
    pool: &AnyPool,
) -> Result<Vec<DatabaseTable>, String> {
    let sql = match profile.engine.as_str() {
        "postgresql" => {
            "SELECT table_schema || '.' || table_name AS qualified_name, table_type \
             FROM information_schema.tables \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
             ORDER BY table_schema, table_name LIMIT 2000"
        }
        "mysql" | "mariadb" => {
            "SELECT table_name AS qualified_name, table_type \
             FROM information_schema.tables WHERE table_schema = DATABASE() \
             ORDER BY table_name LIMIT 2000"
        }
        _ => return Err("远程数据库类型无效".to_owned()),
    };
    let rows = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("读取远程数据库结构失败：{error}"))?;
    if rows.len() > REMOTE_TABLE_LIMIT {
        return Err("远程数据库对象数量超出读取限制".to_owned());
    }
    rows.into_iter()
        .map(|row| {
            let name = row
                .try_get::<String, _>("qualified_name")
                .map_err(|error| error.to_string())?;
            let table_type = row
                .try_get::<String, _>("table_type")
                .map_err(|error| error.to_string())?;
            Ok(DatabaseTable {
                name,
                kind: if table_type.to_ascii_uppercase().contains("VIEW") {
                    "view".to_owned()
                } else {
                    "table".to_owned()
                },
                row_count: None,
            })
        })
        .collect()
}

async fn describe_remote_table_with_pool(
    profile: &RemoteDatabaseProfile,
    pool: &AnyPool,
    table_name: &str,
) -> Result<Vec<DatabaseColumn>, String> {
    if table_name.trim().is_empty() {
        return Err("请选择数据表".to_owned());
    }
    let (schema, table) = split_qualified_name(profile, table_name);
    let sql = match profile.engine.as_str() {
        "postgresql" => {
            "SELECT ordinal_position::BIGINT AS ordinal_position, column_name, data_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position"
        }
        "mysql" | "mariadb" => {
            "SELECT CAST(ordinal_position AS SIGNED) AS ordinal_position, column_name, column_type AS data_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position"
        }
        _ => return Err("远程数据库类型无效".to_owned()),
    };
    let rows = sqlx::query(sql)
        .bind(schema)
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("读取字段结构失败：{error}"))?;
    if rows.is_empty() {
        return Err(format!("数据表不存在：{table_name}"));
    }
    rows.into_iter()
        .map(|row| {
            let nullable = row
                .try_get::<String, _>("is_nullable")
                .map_err(|error| error.to_string())?;
            Ok(DatabaseColumn {
                ordinal: row
                    .try_get::<i64, _>("ordinal_position")
                    .map_err(|error| error.to_string())?
                    - 1,
                name: row
                    .try_get::<String, _>("column_name")
                    .map_err(|error| error.to_string())?,
                data_type: row
                    .try_get::<String, _>("data_type")
                    .map_err(|error| error.to_string())?,
                not_null: nullable.eq_ignore_ascii_case("NO"),
                default_value: row
                    .try_get::<Option<String>, _>("column_default")
                    .ok()
                    .flatten(),
                primary_key: false,
            })
        })
        .collect()
}

fn split_qualified_name<'a>(
    profile: &'a RemoteDatabaseProfile,
    table_name: &'a str,
) -> (&'a str, &'a str) {
    if profile.engine == "postgresql" {
        table_name.split_once('.').unwrap_or(("public", table_name))
    } else {
        (
            &profile.database,
            table_name
                .rsplit_once('.')
                .map_or(table_name, |(_, table)| table),
        )
    }
}

async fn remote_schema_context_with_pool(
    profile: &RemoteDatabaseProfile,
    pool: &AnyPool,
) -> Result<String, String> {
    let tables = list_remote_tables_with_pool(profile, pool).await?;
    let mut output = format!(
        "-- {} 数据库结构：{} / {}\n",
        database_engine_name(&profile.engine),
        profile.name,
        profile.database
    );
    let table_count = tables.len();
    for table in tables.into_iter().take(SCHEMA_TABLE_LIMIT) {
        let columns = describe_remote_table_with_pool(profile, pool, &table.name).await?;
        let definition = columns
            .iter()
            .map(|column| {
                format!(
                    "  {} {}{}",
                    quote_remote_identifier(profile, &column.name),
                    column.data_type,
                    if column.not_null { " NOT NULL" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        let block = format!(
            "\nCREATE {} {} (\n{}\n);\n",
            if table.kind == "view" {
                "VIEW"
            } else {
                "TABLE"
            },
            quote_qualified_identifier(profile, &table.name),
            definition
        );
        if output.len().saturating_add(block.len()) > SCHEMA_CONTEXT_BYTE_LIMIT {
            output.push_str("\n-- 结构内容过长，已截断\n");
            break;
        }
        output.push_str(&block);
    }
    if table_count > SCHEMA_TABLE_LIMIT {
        output.push_str("\n-- 数据库对象较多，AI 结构上下文只包含前 200 个对象\n");
    }
    Ok(output)
}

fn quote_remote_identifier(profile: &RemoteDatabaseProfile, value: &str) -> String {
    if matches!(profile.engine.as_str(), "mysql" | "mariadb") {
        format!("`{}`", value.replace('`', "``"))
    } else {
        format!("\"{}\"", value.replace('"', "\"\""))
    }
}

fn quote_qualified_identifier(profile: &RemoteDatabaseProfile, value: &str) -> String {
    value
        .split('.')
        .map(|part| quote_remote_identifier(profile, part))
        .collect::<Vec<_>>()
        .join(".")
}

async fn execute_remote_sql_with_pool(
    pool: &AnyPool,
    sql: &str,
    window: QueryWindow,
) -> Result<DatabaseQueryResult, String> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err("请输入要执行的 SQL".to_owned());
    }
    let started = Instant::now();
    let statement_type = first_sql_keyword(sql);
    let returns_rows = matches!(
        statement_type.as_str(),
        "SELECT" | "WITH" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "VALUES"
    );
    if !returns_rows {
        let result = sqlx::query(sql)
            .execute(pool)
            .await
            .map_err(|error| format!("SQL 执行失败：{error}"))?;
        return Ok(DatabaseQueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: result.rows_affected() as usize,
            duration_ms: started.elapsed().as_millis() as u64,
            truncated: false,
            statement_type,
            offset: 0,
            limit: window.limit,
            has_more: false,
        });
    }

    let mut stream = sqlx::query(sql).fetch(pool);
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let mut seen_rows = 0_usize;
    let mut result_bytes = 0_usize;
    let mut truncated = false;
    let mut has_more = false;
    while let Some(row) = stream
        .try_next()
        .await
        .map_err(|error| format!("SQL 查询失败：{error}"))?
    {
        if seen_rows < window.offset {
            seen_rows += 1;
            continue;
        }
        if rows.len() >= window.limit {
            has_more = true;
            break;
        }
        if columns.is_empty() {
            columns = row
                .columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect();
        }
        let mut values = Vec::with_capacity(row.len());
        for index in 0..row.len() {
            let value = remote_value_to_json(&row, index);
            let value_bytes = value.to_string().len();
            if result_bytes.saturating_add(value_bytes) > RESULT_BYTE_LIMIT {
                truncated = true;
                has_more = true;
                break;
            }
            result_bytes += value_bytes;
            values.push(value);
        }
        if truncated && values.len() != row.len() {
            break;
        }
        rows.push(values);
    }
    Ok(DatabaseQueryResult {
        columns,
        rows,
        affected_rows: 0,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated,
        statement_type,
        offset: window.offset,
        limit: window.limit,
        has_more,
    })
}

async fn execute_remote_read_only_with_pool(
    profile: &RemoteDatabaseProfile,
    pool: &AnyPool,
    sql: &str,
) -> Result<DatabaseQueryResult, String> {
    ensure_read_only_sql(sql)?;
    let mut connection = pool
        .acquire()
        .await
        .map_err(|error| format!("获取只读数据库连接失败：{error}"))?;
    let begin = match profile.engine.as_str() {
        "postgresql" => "BEGIN READ ONLY",
        "mysql" | "mariadb" => "START TRANSACTION READ ONLY",
        _ => return Err("远程数据库类型无效".to_owned()),
    };
    sqlx::query(begin)
        .execute(&mut *connection)
        .await
        .map_err(|error| format!("启动只读事务失败：{error}"))?;
    let started = Instant::now();
    let query_result = execute_remote_read_only_with_connection(&mut connection, sql).await;
    let rollback_result = sqlx::query("ROLLBACK")
        .execute(&mut *connection)
        .await
        .map_err(|error| format!("回滚只读事务失败：{error}"));
    match (query_result, rollback_result) {
        (Ok(rows), Ok(_)) => Ok(remote_rows_to_result(
            rows,
            first_sql_keyword(sql),
            started.elapsed().as_millis() as u64,
            QueryWindow::default(),
        )),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

async fn execute_remote_read_only_with_connection(
    connection: &mut AnyConnection,
    sql: &str,
) -> Result<Vec<AnyRow>, String> {
    sqlx::query(sql)
        .fetch_all(connection)
        .await
        .map_err(|error| format!("只读 SQL 查询失败：{error}"))
}

fn remote_rows_to_result(
    remote_rows: Vec<AnyRow>,
    statement_type: String,
    duration_ms: u64,
    window: QueryWindow,
) -> DatabaseQueryResult {
    let columns = remote_rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect()
        })
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut result_bytes = 0_usize;
    let mut truncated = false;
    let mut has_more = false;
    for row in remote_rows.into_iter().skip(window.offset) {
        if rows.len() >= window.limit {
            has_more = true;
            break;
        }
        let mut values = Vec::with_capacity(row.len());
        for index in 0..row.len() {
            let value = remote_value_to_json(&row, index);
            let value_bytes = value.to_string().len();
            if result_bytes.saturating_add(value_bytes) > RESULT_BYTE_LIMIT {
                truncated = true;
                has_more = true;
                break;
            }
            result_bytes += value_bytes;
            values.push(value);
        }
        if truncated && values.len() != row.len() {
            break;
        }
        rows.push(values);
    }
    DatabaseQueryResult {
        columns,
        rows,
        affected_rows: 0,
        duration_ms,
        truncated,
        statement_type,
        offset: window.offset,
        limit: window.limit,
        has_more,
    }
}

fn remote_value_to_json(row: &AnyRow, index: usize) -> serde_json::Value {
    let raw = match row.try_get_raw(index) {
        Ok(value) => value,
        Err(error) => return serde_json::Value::String(format!("[读取失败：{error}]")),
    };
    if raw.is_null() {
        return serde_json::Value::Null;
    }
    let type_name = raw.type_info().name().to_ascii_uppercase();
    if matches!(type_name.as_str(), "BOOL" | "BOOLEAN" | "TINYINT(1)")
        && let Ok(value) = row.try_get::<bool, _>(index)
    {
        return serde_json::Value::Bool(value);
    }
    if (type_name.contains("INT") || matches!(type_name.as_str(), "SERIAL" | "BIGSERIAL"))
        && let Ok(value) = row.try_get::<i64, _>(index)
    {
        return serde_json::Value::from(value);
    }
    if type_name.contains("INT") {
        if let Ok(value) = row.try_get::<i32, _>(index) {
            return serde_json::Value::from(value);
        }
        if let Ok(value) = row.try_get::<i16, _>(index) {
            return serde_json::Value::from(value);
        }
    }
    if (type_name.contains("REAL")
        || type_name.contains("FLOAT")
        || type_name.contains("DOUBLE")
        || type_name.contains("NUMERIC")
        || type_name.contains("DECIMAL"))
        && let Ok(value) = row.try_get::<f64, _>(index)
    {
        return serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }
    if (type_name.contains("BLOB") || type_name.contains("BINARY") || type_name == "BYTEA")
        && let Ok(value) = row.try_get::<Vec<u8>, _>(index)
    {
        let truncated = value.len() > CELL_BYTE_LIMIT;
        let displayed = &value[..value.len().min(CELL_BYTE_LIMIT)];
        return serde_json::Value::String(format!(
            "0x{}{}",
            hex(displayed),
            if truncated {
                "… [BLOB 已截断]"
            } else {
                ""
            }
        ));
    }
    if let Ok(value) = row.try_get::<String, _>(index) {
        return truncate_remote_text(value);
    }
    if let Some(value) = [
        row.try_get::<i64, _>(index)
            .ok()
            .map(serde_json::Value::from),
        row.try_get::<f64, _>(index)
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number),
        row.try_get::<bool, _>(index)
            .ok()
            .map(serde_json::Value::Bool),
    ]
    .into_iter()
    .flatten()
    .next()
    {
        return value;
    }
    serde_json::Value::String(format!("[{}]", raw.type_info().name()))
}

fn truncate_remote_text(value: String) -> serde_json::Value {
    if value.len() <= CELL_BYTE_LIMIT {
        return serde_json::Value::String(value);
    }
    let mut end = CELL_BYTE_LIMIT;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    serde_json::Value::String(format!("{}… [单元格已截断]", &value[..end]))
}

fn workspace_database_path(paths: &AppPaths) -> PathBuf {
    workspace_database_path_at(&paths.workspace_root)
}

fn workspace_database_path_at(workspace_root: &Path) -> PathBuf {
    workspace_root.join("databases").join("workspace.sqlite3")
}

fn open_database(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let connection =
        Connection::open(path).map_err(|error| format!("打开 SQLite 失败：{error}"))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(30))
        .map_err(|error| error.to_string())?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(|error| format!("初始化 SQLite 失败：{error}"))?;
    Ok(connection)
}

fn open_external_database(path: &Path) -> Result<Connection, String> {
    if !path.is_file() {
        return Err(format!("数据源文件不存在：{}", path.display()));
    }
    let connection =
        Connection::open(path).map_err(|error| format!("打开 SQLite 失败：{error}"))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(30))
        .map_err(|error| error.to_string())?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| format!("初始化 SQLite 失败：{error}"))?;
    Ok(connection)
}

fn open_database_read_only(path: &Path) -> Result<Connection, String> {
    if !path.is_file() {
        return Err(format!("数据源文件不存在：{}", path.display()));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("以只读方式打开 SQLite 失败：{error}"))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(30))
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn open_external_database_read_only(path: &Path) -> Result<Connection, String> {
    open_database_read_only(path)
}

fn list_tables_at(path: &Path) -> Result<Vec<DatabaseTable>, String> {
    let connection = open_database(path)?;
    list_tables_with_connection(&connection)
}

fn list_tables_external(path: &Path) -> Result<Vec<DatabaseTable>, String> {
    let connection = open_external_database(path)?;
    list_tables_with_connection(&connection)
}

fn list_tables_with_connection(connection: &Connection) -> Result<Vec<DatabaseTable>, String> {
    let mut statement = connection
        .prepare(
            "SELECT name, type FROM sqlite_schema \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY CASE type WHEN 'table' THEN 0 ELSE 1 END, name COLLATE NOCASE",
        )
        .map_err(|error| error.to_string())?;
    let entries = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(entries
        .into_iter()
        .map(|(name, kind)| DatabaseTable {
            name,
            kind,
            row_count: None,
        })
        .collect())
}

fn describe_table_at(path: &Path, table_name: &str) -> Result<Vec<DatabaseColumn>, String> {
    let connection = open_database(path)?;
    describe_table_with_connection(&connection, table_name)
}

fn describe_table_external(path: &Path, table_name: &str) -> Result<Vec<DatabaseColumn>, String> {
    let connection = open_external_database(path)?;
    describe_table_with_connection(&connection, table_name)
}

fn describe_table_with_connection(
    connection: &Connection,
    table_name: &str,
) -> Result<Vec<DatabaseColumn>, String> {
    if table_name.trim().is_empty() {
        return Err("请选择数据表".to_owned());
    }
    let mut statement = connection
        .prepare(
            "SELECT cid, name, type, \"notnull\", dflt_value, pk \
             FROM pragma_table_info(?1) ORDER BY cid",
        )
        .map_err(|error| error.to_string())?;
    let columns = statement
        .query_map(params![table_name], |row| {
            Ok(DatabaseColumn {
                ordinal: row.get(0)?,
                name: row.get(1)?,
                data_type: row.get::<_, String>(2)?,
                not_null: row.get::<_, i64>(3)? != 0,
                default_value: row.get(4)?,
                primary_key: row.get::<_, i64>(5)? != 0,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if columns.is_empty() {
        return Err(format!("数据表不存在：{table_name}"));
    }
    Ok(columns)
}

fn schema_context_at(path: &Path) -> Result<String, String> {
    let connection = open_database(path)?;
    schema_context_with_connection(&connection, "SQLite 工作区数据库结构")
}

fn schema_context_read_only_at(path: &Path) -> Result<String, String> {
    let connection = open_database_read_only(path)?;
    schema_context_with_connection(&connection, "SQLite 工作区数据库结构（只读）")
}

fn schema_context_external(path: &Path, title: &str) -> Result<String, String> {
    let connection = open_external_database(path)?;
    schema_context_with_connection(&connection, title)
}

fn schema_context_with_connection(connection: &Connection, title: &str) -> Result<String, String> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, sql FROM sqlite_schema \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
             AND sql IS NOT NULL \
             ORDER BY CASE type WHEN 'table' THEN 0 ELSE 1 END, name COLLATE NOCASE",
        )
        .map_err(|error| error.to_string())?;
    let definitions = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let mut output = format!("-- {title}\n");
    for definition in definitions {
        let (kind, name, sql) = definition.map_err(|error| error.to_string())?;
        let block = format!("\n-- {kind}: {name}\n{};\n", sql.trim_end_matches(';'));
        if output.len().saturating_add(block.len()) > SCHEMA_CONTEXT_BYTE_LIMIT {
            output.push_str("\n-- 结构内容过长，已截断\n");
            break;
        }
        output.push_str(&block);
    }
    if output.lines().count() == 1 {
        output.push_str("\n-- 当前没有用户数据表或视图\n");
    }
    Ok(output)
}

#[cfg(test)]
fn execute_sql_at(path: &Path, sql: &str) -> Result<DatabaseQueryResult, String> {
    execute_sql_at_window(path, sql, QueryWindow::default())
}

fn execute_sql_at_window(
    path: &Path,
    sql: &str,
    window: QueryWindow,
) -> Result<DatabaseQueryResult, String> {
    let connection = open_database(path)?;
    execute_sql_with_connection_window(&connection, sql, window)
}

#[cfg(test)]
fn execute_sql_external(path: &Path, sql: &str) -> Result<DatabaseQueryResult, String> {
    execute_sql_external_window(path, sql, QueryWindow::default())
}

fn execute_sql_external_window(
    path: &Path,
    sql: &str,
    window: QueryWindow,
) -> Result<DatabaseQueryResult, String> {
    let connection = open_external_database(path)?;
    execute_sql_with_connection_window(&connection, sql, window)
}

#[cfg(test)]
fn execute_sql_with_connection(
    connection: &Connection,
    sql: &str,
) -> Result<DatabaseQueryResult, String> {
    execute_sql_with_connection_window(connection, sql, QueryWindow::default())
}

fn execute_sql_with_connection_window(
    connection: &Connection,
    sql: &str,
    window: QueryWindow,
) -> Result<DatabaseQueryResult, String> {
    execute_sql_with_connection_mode_window(connection, sql, false, window)
}

fn execute_sql_with_connection_mode(
    connection: &Connection,
    sql: &str,
    read_only: bool,
) -> Result<DatabaseQueryResult, String> {
    execute_sql_with_connection_mode_window(connection, sql, read_only, QueryWindow::default())
}

fn execute_sql_with_connection_mode_window(
    connection: &Connection,
    sql: &str,
    read_only: bool,
    window: QueryWindow,
) -> Result<DatabaseQueryResult, String> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err("请输入要执行的 SQL".to_owned());
    }
    let started = Instant::now();
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("SQL 编译失败：{error}"))?;
    if read_only && !statement.readonly() {
        return Err("Agent 数据工具只允许只读查询，当前 SQL 可能修改数据库".to_owned());
    }
    let statement_type = first_sql_keyword(sql);

    if statement.column_count() == 0 {
        let affected_rows = statement
            .execute([])
            .map_err(|error| format!("SQL 执行失败：{error}"))?;
        return Ok(DatabaseQueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows,
            duration_ms: started.elapsed().as_millis() as u64,
            truncated: false,
            statement_type,
            offset: 0,
            limit: window.limit,
            has_more: false,
        });
    }

    let columns = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let column_count = columns.len();
    let mut cursor = statement
        .query([])
        .map_err(|error| format!("SQL 查询失败：{error}"))?;
    let mut rows = Vec::new();
    let mut truncated = false;
    let mut has_more = false;
    let mut result_bytes = 0_usize;
    let mut seen_rows = 0_usize;
    'rows: while let Some(row) = cursor
        .next()
        .map_err(|error| format!("读取查询结果失败：{error}"))?
    {
        if seen_rows < window.offset {
            seen_rows += 1;
            continue;
        }
        if rows.len() == window.limit {
            has_more = true;
            break;
        }
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let (value, cell_truncated) =
                sqlite_value_to_json(row.get_ref(index).map_err(|error| error.to_string())?);
            let value_bytes = value.to_string().len();
            if result_bytes.saturating_add(value_bytes) > RESULT_BYTE_LIMIT {
                truncated = true;
                has_more = true;
                break 'rows;
            }
            result_bytes += value_bytes;
            truncated |= cell_truncated;
            values.push(value);
        }
        rows.push(values);
    }
    Ok(DatabaseQueryResult {
        columns,
        rows,
        affected_rows: 0,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated,
        statement_type,
        offset: window.offset,
        limit: window.limit,
        has_more,
    })
}

fn open_excel_as_sqlite(path: &Path) -> Result<Connection, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("读取工作簿失败（{}）：{error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("工作簿文件不存在：{}", path.display()));
    }
    if metadata.len() > EXCEL_FILE_BYTE_LIMIT {
        return Err(format!(
            "工作簿超过 {} MB 的读取限制",
            EXCEL_FILE_BYTE_LIMIT / 1024 / 1024
        ));
    }
    let mut workbook =
        open_workbook_auto(path).map_err(|error| format!("打开工作簿失败：{error}"))?;
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.len() > EXCEL_SHEET_LIMIT {
        return Err(format!("工作表数量超过 {EXCEL_SHEET_LIMIT} 个的读取限制"));
    }
    let mut connection =
        Connection::open_in_memory().map_err(|error| format!("创建工作簿查询环境失败：{error}"))?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("初始化工作簿查询环境失败：{error}"))?;

    for sheet_name in sheet_names {
        let range = workbook
            .worksheet_range(&sheet_name)
            .map_err(|error| format!("读取工作表“{sheet_name}”失败：{error}"))?;
        let rows = range.rows().collect::<Vec<_>>();
        let column_count = rows
            .iter()
            .map(|row| row.len())
            .max()
            .unwrap_or(0)
            .min(EXCEL_COLUMN_LIMIT);
        if column_count == 0 {
            continue;
        }
        if rows.len().saturating_sub(1) > EXCEL_ROW_LIMIT {
            return Err(format!(
                "工作表“{sheet_name}”超过 {EXCEL_ROW_LIMIT} 行的读取限制"
            ));
        }
        let headers = excel_headers(rows[0], column_count);
        let types = infer_excel_column_types(&rows[1..], column_count);
        let definitions = headers
            .iter()
            .zip(types.iter())
            .map(|(name, data_type)| format!("{} {data_type}", quote_sqlite_identifier(name)))
            .collect::<Vec<_>>()
            .join(", ");
        transaction
            .execute(
                &format!(
                    "CREATE TABLE {} ({definitions})",
                    quote_sqlite_identifier(&sheet_name)
                ),
                [],
            )
            .map_err(|error| format!("映射工作表“{sheet_name}”失败：{error}"))?;
        let placeholders = (1..=column_count)
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let insert_sql = format!(
            "INSERT INTO {} VALUES ({placeholders})",
            quote_sqlite_identifier(&sheet_name)
        );
        let mut insert = transaction
            .prepare(&insert_sql)
            .map_err(|error| format!("准备导入工作表“{sheet_name}”失败：{error}"))?;
        for row in rows.iter().skip(1) {
            let values = (0..column_count)
                .map(|index| row.get(index).map_or(SqliteValue::Null, excel_value))
                .collect::<Vec<_>>();
            insert
                .execute(params_from_iter(values))
                .map_err(|error| format!("导入工作表“{sheet_name}”失败：{error}"))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| format!("提交工作簿查询环境失败：{error}"))?;
    Ok(connection)
}

fn is_tabular_file_engine(engine: &str) -> bool {
    matches!(engine, "excel" | "csv" | "json")
}

fn database_engine_name(engine: &str) -> &'static str {
    match engine {
        "postgresql" => "PostgreSQL",
        "mysql" => "MySQL",
        "mariadb" => "MariaDB",
        "sqlite" => "SQLite",
        "excel" => "Excel 工作簿",
        "csv" => "CSV / TSV",
        "json" => "JSON / JSONL",
        _ => "数据源",
    }
}

fn tabular_schema_label(engine: &str) -> &'static str {
    match engine {
        "excel" => "Excel 工作簿结构（工作表映射为只读表）",
        "csv" => "CSV / TSV 结构（文件映射为只读表）",
        "json" => "JSON / JSONL 结构（对象字段映射为只读表）",
        _ => "文件数据源结构",
    }
}

fn open_tabular_file_as_sqlite(profile: &RemoteDatabaseProfile) -> Result<Connection, String> {
    let path = Path::new(&profile.database);
    match profile.engine.as_str() {
        "excel" => open_excel_as_sqlite(path),
        "csv" => open_delimited_as_sqlite(path),
        "json" => open_json_as_sqlite(path),
        _ => Err("不是可映射为查询表的文件数据源".to_owned()),
    }
}

fn validate_tabular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("读取数据文件失败（{}）：{error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("数据文件不存在：{}", path.display()));
    }
    if metadata.len() > TABULAR_FILE_BYTE_LIMIT {
        return Err(format!(
            "数据文件超过 {} MB 的读取限制",
            TABULAR_FILE_BYTE_LIMIT / 1024 / 1024
        ));
    }
    Ok(())
}

fn open_delimited_as_sqlite(path: &Path) -> Result<Connection, String> {
    validate_tabular_file(path)?;
    let delimiter = if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("tsv"))
    {
        b'\t'
    } else {
        b','
    };
    let bytes = fs::read(path).map_err(|error| format!("读取分隔文本失败：{error}"))?;
    let source = std::str::from_utf8(&bytes)
        .map_err(|error| format!("分隔文本不是 UTF-8 编码：{error}"))?
        .trim_start_matches('\u{feff}');
    let mut records = parse_delimited_records(source, char::from(delimiter))?;
    if records.is_empty() {
        return Err("分隔文本为空".to_owned());
    }
    let headers = unique_text_headers(records.remove(0));
    if headers.is_empty() || headers.len() > EXCEL_COLUMN_LIMIT {
        return Err(format!("分隔文本字段数应为 1 到 {EXCEL_COLUMN_LIMIT}"));
    }
    if records.len() > TABULAR_ROW_LIMIT {
        return Err(format!("分隔文本超过 {TABULAR_ROW_LIMIT} 行的读取限制"));
    }
    let rows = records
        .into_iter()
        .map(|record| {
            (0..headers.len())
                .map(|index| record.get(index).cloned().unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let types = infer_text_column_types(&rows, headers.len());
    let values = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .map(|(index, value)| text_value(value, types[index]))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    create_tabular_connection(path, &headers, &types, values)
}

fn parse_delimited_records(source: &str, delimiter: char) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        if quoted {
            if character == '"' {
                if characters.peek() == Some(&'"') {
                    characters.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(character);
            }
            continue;
        }
        match character {
            '"' if field.is_empty() => quoted = true,
            value if value == delimiter => {
                record.push(std::mem::take(&mut field));
            }
            '\r' if characters.peek() == Some(&'\n') => {
                characters.next();
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            value => field.push(value),
        }
    }
    if quoted {
        return Err("分隔文本包含未闭合的引号字段".to_owned());
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    while records
        .last()
        .is_some_and(|record| record.iter().all(String::is_empty))
    {
        records.pop();
    }
    Ok(records)
}

fn open_json_as_sqlite(path: &Path) -> Result<Connection, String> {
    validate_tabular_file(path)?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let records = if matches!(extension.as_str(), "jsonl" | "ndjson") {
        let file = fs::File::open(path).map_err(|error| format!("打开 JSONL 失败：{error}"))?;
        let mut records = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line =
                line.map_err(|error| format!("读取 JSONL 第 {} 行失败：{error}", index + 1))?;
            if line.trim().is_empty() {
                continue;
            }
            if records.len() >= TABULAR_ROW_LIMIT {
                return Err(format!("JSONL 超过 {TABULAR_ROW_LIMIT} 行的读取限制"));
            }
            records.push(
                serde_json::from_str::<serde_json::Value>(&line)
                    .map_err(|error| format!("解析 JSONL 第 {} 行失败：{error}", index + 1))?,
            );
        }
        records
    } else {
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(path).map_err(|error| format!("读取 JSON 失败：{error}"))?,
        )
        .map_err(|error| format!("解析 JSON 失败：{error}"))?;
        match value {
            serde_json::Value::Array(records) => records,
            value => vec![value],
        }
    };
    if records.len() > TABULAR_ROW_LIMIT {
        return Err(format!("JSON 数据超过 {TABULAR_ROW_LIMIT} 行的读取限制"));
    }
    let mut headers = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for record in &records {
        if let serde_json::Value::Object(object) = record {
            for key in object.keys() {
                if seen.insert(key.to_ascii_lowercase()) {
                    headers.push(key.clone());
                    if headers.len() > EXCEL_COLUMN_LIMIT {
                        return Err(format!("JSON 字段数超过 {EXCEL_COLUMN_LIMIT} 个的读取限制"));
                    }
                }
            }
        } else if seen.insert("value".to_owned()) {
            headers.push("value".to_owned());
        }
    }
    if headers.is_empty() {
        return Err("JSON 数据中没有可映射的记录".to_owned());
    }
    let types = infer_json_column_types(&records, &headers);
    let values = records
        .iter()
        .map(|record| {
            headers
                .iter()
                .map(|header| match record {
                    serde_json::Value::Object(object) => {
                        object.get(header).unwrap_or(&serde_json::Value::Null)
                    }
                    value if header == "value" => value,
                    _ => &serde_json::Value::Null,
                })
                .map(json_sqlite_value)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    create_tabular_connection(path, &headers, &types, values)
}

fn create_tabular_connection(
    path: &Path,
    headers: &[String],
    types: &[&str],
    rows: Vec<Vec<SqliteValue>>,
) -> Result<Connection, String> {
    let table_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("data");
    let mut connection =
        Connection::open_in_memory().map_err(|error| format!("创建文件查询环境失败：{error}"))?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("初始化文件查询环境失败：{error}"))?;
    let definitions = headers
        .iter()
        .zip(types.iter())
        .map(|(name, data_type)| format!("{} {data_type}", quote_sqlite_identifier(name)))
        .collect::<Vec<_>>()
        .join(", ");
    transaction
        .execute(
            &format!(
                "CREATE TABLE {} ({definitions})",
                quote_sqlite_identifier(table_name)
            ),
            [],
        )
        .map_err(|error| format!("创建文件映射表失败：{error}"))?;
    let placeholders = (1..=headers.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut insert = transaction
        .prepare(&format!(
            "INSERT INTO {} VALUES ({placeholders})",
            quote_sqlite_identifier(table_name)
        ))
        .map_err(|error| format!("准备导入文件数据失败：{error}"))?;
    for row in rows {
        insert
            .execute(params_from_iter(row))
            .map_err(|error| format!("导入文件数据失败：{error}"))?;
    }
    drop(insert);
    transaction
        .commit()
        .map_err(|error| format!("提交文件查询环境失败：{error}"))?;
    Ok(connection)
}

fn unique_text_headers(headers: Vec<String>) -> Vec<String> {
    let mut used = std::collections::HashSet::new();
    headers
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let base = if value.trim().is_empty() {
                format!("column_{}", index + 1)
            } else {
                value
            };
            let mut candidate = base.clone();
            let mut suffix = 2;
            while !used.insert(candidate.to_ascii_lowercase()) {
                candidate = format!("{base}_{suffix}");
                suffix += 1;
            }
            candidate
        })
        .collect()
}

fn infer_text_column_types(rows: &[Vec<String>], column_count: usize) -> Vec<&'static str> {
    let mut types = vec!["INTEGER"; column_count];
    for row in rows.iter().take(1_000) {
        for (index, value) in row.iter().enumerate() {
            if value.trim().is_empty() {
                continue;
            }
            if value.parse::<i64>().is_ok() && types[index] == "INTEGER" {
                continue;
            }
            if value.parse::<f64>().is_ok() && matches!(types[index], "INTEGER" | "REAL") {
                types[index] = "REAL";
            } else {
                types[index] = "TEXT";
            }
        }
    }
    types
}

fn text_value(value: String, data_type: &str) -> SqliteValue {
    if value.trim().is_empty() {
        SqliteValue::Null
    } else if data_type == "INTEGER" {
        value
            .parse::<i64>()
            .map(SqliteValue::Integer)
            .unwrap_or(SqliteValue::Text(value))
    } else if data_type == "REAL" {
        value
            .parse::<f64>()
            .map(SqliteValue::Real)
            .unwrap_or(SqliteValue::Text(value))
    } else {
        SqliteValue::Text(value)
    }
}

fn infer_json_column_types(records: &[serde_json::Value], headers: &[String]) -> Vec<&'static str> {
    headers
        .iter()
        .map(|header| {
            let mut kind = "INTEGER";
            for value in records.iter().take(1_000).map(|record| match record {
                serde_json::Value::Object(object) => {
                    object.get(header).unwrap_or(&serde_json::Value::Null)
                }
                value if header == "value" => value,
                _ => &serde_json::Value::Null,
            }) {
                kind = match value {
                    serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_)
                        if kind == "INTEGER" =>
                    {
                        kind
                    }
                    serde_json::Value::Number(_) if kind == "REAL" => kind,
                    serde_json::Value::Null => kind,
                    _ => "TEXT",
                };
                if kind == "TEXT" {
                    break;
                }
            }
            kind
        })
        .collect()
}

fn json_sqlite_value(value: &serde_json::Value) -> SqliteValue {
    match value {
        serde_json::Value::Null => SqliteValue::Null,
        serde_json::Value::Bool(value) => SqliteValue::Integer(i64::from(*value)),
        serde_json::Value::Number(value) if value.is_i64() => {
            SqliteValue::Integer(value.as_i64().unwrap_or_default())
        }
        serde_json::Value::Number(value) => SqliteValue::Real(value.as_f64().unwrap_or_default()),
        serde_json::Value::String(value) => SqliteValue::Text(value.clone()),
        value => SqliteValue::Text(value.to_string()),
    }
}

fn excel_headers(row: &[Data], column_count: usize) -> Vec<String> {
    let mut used = std::collections::HashSet::new();
    (0..column_count)
        .map(|index| {
            let base = row
                .get(index)
                .map(excel_display_value)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| format!("column_{}", index + 1));
            let mut candidate = base.clone();
            let mut suffix = 2;
            while !used.insert(candidate.to_lowercase()) {
                candidate = format!("{base}_{suffix}");
                suffix += 1;
            }
            candidate
        })
        .collect()
}

fn infer_excel_column_types(rows: &[&[Data]], column_count: usize) -> Vec<&'static str> {
    let mut types = vec!["INTEGER"; column_count];
    for row in rows.iter().take(1_000) {
        for (index, cell) in row.iter().take(column_count).enumerate() {
            types[index] = match cell {
                Data::Empty => types[index],
                Data::Int(_) | Data::Bool(_) if types[index] == "INTEGER" => "INTEGER",
                Data::Int(_) | Data::Float(_) | Data::Bool(_)
                    if matches!(types[index], "INTEGER" | "REAL") =>
                {
                    "REAL"
                }
                _ => "TEXT",
            };
        }
    }
    types
}

fn excel_value(value: &Data) -> SqliteValue {
    match value {
        Data::Empty => SqliteValue::Null,
        Data::Int(value) => SqliteValue::Integer(*value),
        Data::Float(value) => SqliteValue::Real(*value),
        Data::Bool(value) => SqliteValue::Integer(i64::from(*value)),
        _ => SqliteValue::Text(excel_display_value(value)),
    }
}

fn excel_display_value(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        Data::Float(value) => value.to_string(),
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::Error(value) => format!("#{value:?}"),
        Data::DateTime(value) => value.to_string(),
        Data::DateTimeIso(value) | Data::DurationIso(value) => value.clone(),
    }
}

fn quote_sqlite_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sqlite_value_to_json(value: SqliteValueRef<'_>) -> (serde_json::Value, bool) {
    match value {
        SqliteValueRef::Null => (serde_json::Value::Null, false),
        SqliteValueRef::Integer(value) => (serde_json::Value::from(value), false),
        SqliteValueRef::Real(value) => (
            serde_json::Number::from_f64(value)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            false,
        ),
        SqliteValueRef::Text(value) => {
            let decoded = String::from_utf8_lossy(value);
            if decoded.len() <= CELL_BYTE_LIMIT {
                (serde_json::Value::String(decoded.into_owned()), false)
            } else {
                let mut end = CELL_BYTE_LIMIT;
                while !decoded.is_char_boundary(end) {
                    end -= 1;
                }
                (
                    serde_json::Value::String(format!("{}… [单元格已截断]", &decoded[..end])),
                    true,
                )
            }
        }
        SqliteValueRef::Blob(value) => {
            let truncated = value.len() > CELL_BYTE_LIMIT;
            let displayed = &value[..value.len().min(CELL_BYTE_LIMIT)];
            let suffix = if truncated {
                "… [BLOB 已截断]"
            } else {
                ""
            };
            (
                serde_json::Value::String(format!("0x{}{}", hex(displayed), suffix)),
                truncated,
            )
        }
    }
}

fn first_sql_keyword(sql: &str) -> String {
    let mut remaining = sql.trim_start();
    loop {
        if let Some(comment) = remaining.strip_prefix("--") {
            remaining = comment
                .find('\n')
                .map_or("", |index| &comment[index + 1..])
                .trim_start();
            continue;
        }
        if let Some(comment) = remaining.strip_prefix("/*") {
            remaining = comment
                .find("*/")
                .map_or("", |index| &comment[index + 2..])
                .trim_start();
            continue;
        }
        return remaining
            .split(|character: char| !character.is_ascii_alphabetic())
            .next()
            .filter(|keyword| !keyword.is_empty())
            .unwrap_or("SQL")
            .to_ascii_uppercase();
    }
}

fn ensure_read_only_sql(sql: &str) -> Result<(), String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err("请输入要执行的 SQL".to_owned());
    }
    let without_trailing = trimmed.strip_suffix(';').unwrap_or(trimmed);
    if without_trailing.contains(';') {
        return Err("Agent 数据工具一次只允许执行一条只读 SQL".to_owned());
    }
    let tokens = sql_keyword_tokens(without_trailing);
    let first = tokens.first().map(String::as_str).unwrap_or("");
    if !matches!(
        first,
        "SELECT" | "WITH" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "DESC" | "VALUES"
    ) {
        return Err(
            "Agent 数据工具仅支持 SELECT、WITH、EXPLAIN、SHOW、DESCRIBE 或 VALUES".to_owned(),
        );
    }
    const FORBIDDEN: &[&str] = &[
        "ALTER", "ANALYZE", "ATTACH", "CALL", "COPY", "CREATE", "DELETE", "DETACH", "DROP", "EXEC",
        "EXECUTE", "GRANT", "IMPORT", "INSERT", "INTO", "LOAD", "LOCK", "MERGE", "PRAGMA",
        "REINDEX", "REPLACE", "RESET", "REVOKE", "SET", "TRUNCATE", "UPDATE", "UPSERT", "VACUUM",
    ];
    if let Some(keyword) = tokens
        .iter()
        .find(|token| FORBIDDEN.contains(&token.as_str()))
    {
        return Err(format!(
            "Agent 数据工具禁止可能修改数据库的关键字：{keyword}"
        ));
    }
    Ok(())
}

fn sql_keyword_tokens(sql: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        SingleQuote,
        DoubleQuote,
        Backtick,
        LineComment,
        BlockComment,
    }
    let characters = sql.chars().collect::<Vec<_>>();
    let mut state = State::Normal;
    let mut index = 0;
    let mut word = String::new();
    let mut tokens = Vec::new();
    let flush = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word).to_ascii_uppercase());
        }
    };
    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();
        match state {
            State::Normal => {
                if character == '-' && next == Some('-') {
                    flush(&mut word, &mut tokens);
                    state = State::LineComment;
                    index += 1;
                } else if character == '/' && next == Some('*') {
                    flush(&mut word, &mut tokens);
                    state = State::BlockComment;
                    index += 1;
                } else if character == '\'' {
                    flush(&mut word, &mut tokens);
                    state = State::SingleQuote;
                } else if character == '"' {
                    flush(&mut word, &mut tokens);
                    state = State::DoubleQuote;
                } else if character == '`' {
                    flush(&mut word, &mut tokens);
                    state = State::Backtick;
                } else if character.is_ascii_alphanumeric() || character == '_' {
                    word.push(character);
                } else {
                    flush(&mut word, &mut tokens);
                }
            }
            State::SingleQuote => {
                if character == '\'' {
                    if next == Some('\'') {
                        index += 1;
                    } else {
                        state = State::Normal;
                    }
                }
            }
            State::DoubleQuote => {
                if character == '"' {
                    if next == Some('"') {
                        index += 1;
                    } else {
                        state = State::Normal;
                    }
                }
            }
            State::Backtick => {
                if character == '`' {
                    if next == Some('`') {
                        index += 1;
                    } else {
                        state = State::Normal;
                    }
                }
            }
            State::LineComment => {
                if matches!(character, '\r' | '\n') {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                if character == '*' && next == Some('/') {
                    state = State::Normal;
                    index += 1;
                }
            }
        }
        index += 1;
    }
    flush(&mut word, &mut tokens);
    tokens
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn export_query_result(
    result: &DatabaseQueryResult,
    format: &str,
    target_path: &Path,
    table_name: &str,
) -> Result<(), String> {
    if result.columns.is_empty() {
        return Err("当前结果没有可导出的列".to_owned());
    }
    let parent = target_path
        .parent()
        .ok_or_else(|| "导出路径无效".to_owned())?;
    if !parent.exists() {
        return Err(format!("导出目录不存在：{}", parent.display()));
    }
    match format.trim().to_ascii_lowercase().as_str() {
        "csv" => export_csv(result, target_path),
        "json" => export_json(result, target_path),
        "sql" => export_sql(result, target_path, table_name),
        "xlsx" => export_xlsx(result, target_path),
        "xls" => export_xls_xml(result, target_path),
        other => Err(format!("不支持的导出格式：{other}")),
    }
}

fn export_csv(result: &DatabaseQueryResult, target_path: &Path) -> Result<(), String> {
    let mut file =
        fs::File::create(target_path).map_err(|error| format!("创建 CSV 文件失败：{error}"))?;
    file.write_all(&[0xef, 0xbb, 0xbf])
        .map_err(|error| format!("写入 CSV 文件失败：{error}"))?;
    write_csv_row(&mut file, result.columns.iter().map(String::as_str))?;
    for row in &result.rows {
        write_csv_row(&mut file, row.iter().map(export_cell_text))?;
    }
    Ok(())
}

fn write_csv_row<I, S>(writer: &mut impl Write, values: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut first = true;
    for value in values {
        if !first {
            writer
                .write_all(b",")
                .map_err(|error| format!("写入 CSV 文件失败：{error}"))?;
        }
        first = false;
        let value = value.as_ref();
        if value.contains([',', '"', '\n', '\r']) {
            writer
                .write_all(b"\"")
                .and_then(|_| writer.write_all(value.replace('"', "\"\"").as_bytes()))
                .and_then(|_| writer.write_all(b"\""))
                .map_err(|error| format!("写入 CSV 文件失败：{error}"))?;
        } else {
            writer
                .write_all(value.as_bytes())
                .map_err(|error| format!("写入 CSV 文件失败：{error}"))?;
        }
    }
    writer
        .write_all(b"\r\n")
        .map_err(|error| format!("写入 CSV 文件失败：{error}"))
}

fn export_json(result: &DatabaseQueryResult, target_path: &Path) -> Result<(), String> {
    let keys = unique_export_columns(&result.columns);
    let rows = result
        .rows
        .iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for (index, key) in keys.iter().enumerate() {
                object.insert(
                    key.clone(),
                    row.get(index).cloned().unwrap_or(serde_json::Value::Null),
                );
            }
            serde_json::Value::Object(object)
        })
        .collect::<Vec<_>>();
    let payload =
        serde_json::to_vec_pretty(&rows).map_err(|error| format!("序列化 JSON 失败：{error}"))?;
    fs::write(target_path, payload).map_err(|error| format!("写入 JSON 文件失败：{error}"))
}

fn export_sql(
    result: &DatabaseQueryResult,
    target_path: &Path,
    table_name: &str,
) -> Result<(), String> {
    let table_name = if table_name.trim().is_empty() {
        "query_result"
    } else {
        table_name.trim()
    };
    let table = table_name
        .split('.')
        .map(quote_sql_export_identifier)
        .collect::<Vec<_>>()
        .join(".");
    let columns = result
        .columns
        .iter()
        .map(|column| quote_sql_export_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let mut file =
        fs::File::create(target_path).map_err(|error| format!("创建 SQL 文件失败：{error}"))?;
    writeln!(file, "-- DRPA Next 查询结果导出")
        .map_err(|error| format!("写入 SQL 文件失败：{error}"))?;
    writeln!(file, "BEGIN;").map_err(|error| format!("写入 SQL 文件失败：{error}"))?;
    for row in &result.rows {
        let values = (0..result.columns.len())
            .map(|index| sql_export_literal(row.get(index).unwrap_or(&serde_json::Value::Null)))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(file, "INSERT INTO {table} ({columns}) VALUES ({values});")
            .map_err(|error| format!("写入 SQL 文件失败：{error}"))?;
    }
    writeln!(file, "COMMIT;").map_err(|error| format!("写入 SQL 文件失败：{error}"))
}

fn export_xlsx(result: &DatabaseQueryResult, target_path: &Path) -> Result<(), String> {
    use zip::write::SimpleFileOptions;

    let file =
        fs::File::create(target_path).map_err(|error| format!("创建 XLSX 文件失败：{error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let fixed_entries = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Query Result" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
    ];
    for (name, contents) in fixed_entries {
        archive
            .start_file(name, options)
            .map_err(|error| format!("写入 XLSX 文件失败：{error}"))?;
        archive
            .write_all(contents.as_bytes())
            .map_err(|error| format!("写入 XLSX 文件失败：{error}"))?;
    }
    archive
        .start_file("xl/worksheets/sheet1.xml", options)
        .map_err(|error| format!("写入 XLSX 工作表失败：{error}"))?;
    archive
        .write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#)
        .map_err(|error| format!("写入 XLSX 工作表失败：{error}"))?;
    write_xlsx_row(
        &mut archive,
        1,
        result
            .columns
            .iter()
            .map(|value| serde_json::Value::String(value.clone())),
    )?;
    for (index, row) in result.rows.iter().enumerate() {
        write_xlsx_row(&mut archive, index + 2, row.iter().cloned())?;
    }
    archive
        .write_all(b"</sheetData></worksheet>")
        .map_err(|error| format!("写入 XLSX 工作表失败：{error}"))?;
    archive
        .finish()
        .map_err(|error| format!("完成 XLSX 文件失败：{error}"))?;
    Ok(())
}

fn write_xlsx_row<I>(writer: &mut impl Write, row_number: usize, values: I) -> Result<(), String>
where
    I: IntoIterator<Item = serde_json::Value>,
{
    write!(writer, "<row r=\"{row_number}\">")
        .map_err(|error| format!("写入 XLSX 工作表失败：{error}"))?;
    for value in values {
        match value {
            serde_json::Value::Null => writer.write_all(b"<c/>"),
            serde_json::Value::Bool(value) => {
                write!(writer, "<c t=\"b\"><v>{}</v></c>", usize::from(value))
            }
            serde_json::Value::Number(value) => write!(writer, "<c><v>{value}</v></c>"),
            other => {
                let text = excel_cell_text(&other);
                write!(
                    writer,
                    "<c t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                    escape_xml(&text)
                )
            }
        }
        .map_err(|error| format!("写入 XLSX 工作表失败：{error}"))?;
    }
    writer
        .write_all(b"</row>")
        .map_err(|error| format!("写入 XLSX 工作表失败：{error}"))
}

fn export_xls_xml(result: &DatabaseQueryResult, target_path: &Path) -> Result<(), String> {
    let mut file =
        fs::File::create(target_path).map_err(|error| format!("创建 XLS 文件失败：{error}"))?;
    file.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><?mso-application progid="Excel.Sheet"?><Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet" xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet"><Worksheet ss:Name="Query Result"><Table>"#)
        .map_err(|error| format!("写入 XLS 文件失败：{error}"))?;
    write_xls_xml_row(
        &mut file,
        result
            .columns
            .iter()
            .map(|value| serde_json::Value::String(value.clone())),
    )?;
    for row in &result.rows {
        write_xls_xml_row(&mut file, row.iter().cloned())?;
    }
    file.write_all(b"</Table></Worksheet></Workbook>")
        .map_err(|error| format!("写入 XLS 文件失败：{error}"))
}

fn write_xls_xml_row<I>(writer: &mut impl Write, values: I) -> Result<(), String>
where
    I: IntoIterator<Item = serde_json::Value>,
{
    writer
        .write_all(b"<Row>")
        .map_err(|error| format!("写入 XLS 文件失败：{error}"))?;
    for value in values {
        let (kind, text) = match value {
            serde_json::Value::Number(value) => ("Number", value.to_string()),
            serde_json::Value::Bool(value) => ("Boolean", usize::from(value).to_string()),
            serde_json::Value::Null => ("String", String::new()),
            other => ("String", excel_cell_text(&other)),
        };
        write!(
            writer,
            "<Cell><Data ss:Type=\"{kind}\">{}</Data></Cell>",
            escape_xml(&text)
        )
        .map_err(|error| format!("写入 XLS 文件失败：{error}"))?;
    }
    writer
        .write_all(b"</Row>")
        .map_err(|error| format!("写入 XLS 文件失败：{error}"))
}

fn export_cell_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

fn excel_cell_text(value: &serde_json::Value) -> String {
    export_cell_text(value).chars().take(32_767).collect()
}

fn unique_export_columns(columns: &[String]) -> Vec<String> {
    let mut counts = std::collections::HashMap::<&str, usize>::new();
    columns
        .iter()
        .map(|column| {
            let count = counts.entry(column.as_str()).or_insert(0);
            *count += 1;
            if *count == 1 {
                column.clone()
            } else {
                format!("{column}_{}", *count)
            }
        })
        .collect()
}

fn quote_sql_export_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sql_export_literal(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_owned(),
        serde_json::Value::Bool(value) => if *value { "TRUE" } else { "FALSE" }.to_owned(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => format!("'{}'", value.replace('\'', "''")),
        value => format!("'{}'", value.to_string().replace('\'', "''")),
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn agent_sql_policy_accepts_queries_and_rejects_mutations() {
        assert!(ensure_read_only_sql("SELECT id, name FROM items LIMIT 10").is_ok());
        assert!(
            ensure_read_only_sql("WITH recent AS (SELECT * FROM items) SELECT * FROM recent")
                .is_ok()
        );
        assert!(ensure_read_only_sql("SELECT 'update is text' AS note").is_ok());
        assert!(ensure_read_only_sql("UPDATE items SET name = 'changed'").is_err());
        assert!(
            ensure_read_only_sql(
                "WITH changed AS (DELETE FROM items RETURNING *) SELECT * FROM changed"
            )
            .is_err()
        );
        assert!(ensure_read_only_sql("SELECT * INTO copied_items FROM items").is_err());
        assert!(ensure_read_only_sql("SELECT 1; SELECT 2").is_err());
    }

    #[test]
    fn creates_schema_and_returns_query_rows() {
        let root = std::env::temp_dir().join(format!("drpa-data-test-{}", uuid::Uuid::new_v4()));
        let path = root.join("workspace.sqlite3");
        execute_sql_at(
            &path,
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        )
        .unwrap();
        execute_sql_at(&path, "INSERT INTO items(name) VALUES ('first')").unwrap();

        let tables = list_tables_at(&path).unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "items");
        let columns = describe_table_at(&path, "items").unwrap();
        assert_eq!(columns.len(), 2);
        let schema = schema_context_at(&path).unwrap();
        assert!(schema.contains("CREATE TABLE items"));
        assert!(schema.contains("name TEXT NOT NULL"));
        let result = execute_sql_at(&path, "SELECT id, name FROM items").unwrap();
        assert_eq!(result.columns, vec!["id", "name"]);
        assert_eq!(
            result.rows[0][1],
            serde_json::Value::String("first".to_owned())
        );
        assert_eq!(first_sql_keyword("-- note\n SELECT 1"), "SELECT");
        assert_eq!(
            first_sql_keyword("/* note */\ninsert into items values (2, 'x')"),
            "INSERT"
        );
        let oversized = vec![b'x'; CELL_BYTE_LIMIT + 1];
        let (value, truncated) = sqlite_value_to_json(SqliteValueRef::Text(&oversized));
        assert!(truncated);
        assert!(value.as_str().unwrap().contains("已截断"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pages_large_sqlite_results_without_a_fixed_row_cap() {
        let root = std::env::temp_dir().join(format!("drpa-page-test-{}", Uuid::new_v4()));
        let path = root.join("workspace.sqlite3");
        let connection = open_database(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE numbers(value INTEGER NOT NULL);\n\
                 WITH RECURSIVE values_cte(value) AS (\n\
                   SELECT 1 UNION ALL SELECT value + 1 FROM values_cte WHERE value < 2505\n\
                 ) INSERT INTO numbers SELECT value FROM values_cte;",
            )
            .unwrap();

        let first = execute_sql_with_connection_window(
            &connection,
            "SELECT value FROM numbers ORDER BY value",
            QueryWindow {
                offset: 0,
                limit: 1_500,
            },
        )
        .unwrap();
        assert_eq!(first.rows.len(), 1_500);
        assert_eq!(first.rows[0][0], 1);
        assert!(first.has_more);
        assert!(!first.truncated);

        let second = execute_sql_with_connection_window(
            &connection,
            "SELECT value FROM numbers ORDER BY value",
            QueryWindow {
                offset: 1_500,
                limit: 1_500,
            },
        )
        .unwrap();
        assert_eq!(second.rows.len(), 1_005);
        assert_eq!(second.rows[0][0], 1_501);
        assert!(!second.has_more);
        assert_eq!(second.offset, 1_500);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exports_query_results_to_common_data_formats() {
        let root = std::env::temp_dir().join(format!("drpa-export-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let result = DatabaseQueryResult {
            columns: vec!["id".to_owned(), "名称".to_owned(), "active".to_owned()],
            rows: vec![
                vec![1.into(), "含,逗号".into(), true.into()],
                vec![2.into(), serde_json::Value::Null, false.into()],
            ],
            affected_rows: 0,
            duration_ms: 2,
            truncated: false,
            statement_type: "SELECT".to_owned(),
            offset: 0,
            limit: 500,
            has_more: false,
        };
        for format in ["csv", "json", "sql", "xlsx", "xls"] {
            let path = root.join(format!("result.{format}"));
            export_query_result(&result, format, &path, "示例表").unwrap();
            assert!(
                fs::metadata(&path).unwrap().len() > 20,
                "empty {format} export"
            );
        }
        let csv = fs::read_to_string(root.join("result.csv")).unwrap();
        assert!(csv.contains("\"含,逗号\""));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
        assert_eq!(json[0]["名称"], "含,逗号");
        let sql = fs::read_to_string(root.join("result.sql")).unwrap();
        assert!(sql.contains("INSERT INTO \"示例表\""));
        let xls = fs::read_to_string(root.join("result.xls")).unwrap();
        assert!(xls.contains("Workbook"));

        let mut workbook = open_workbook_auto(root.join("result.xlsx")).unwrap();
        let range = workbook.worksheet_range("Query Result").unwrap();
        assert_eq!(range.get_value((1, 1)).unwrap().to_string(), "含,逗号");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persists_remote_profiles_without_credentials() {
        let root = std::env::temp_dir().join(format!("drpa-profile-test-{}", Uuid::new_v4()));
        let path = root.join("connections.json");
        let profile = RemoteDatabaseProfile {
            id: "database-example".to_owned(),
            name: "分析库".to_owned(),
            engine: "postgresql".to_owned(),
            host: "db.example.test".to_owned(),
            port: 5432,
            database: "analytics".to_owned(),
            username: "report user".to_owned(),
            tls_mode: "require".to_owned(),
        };

        validate_remote_profile(&profile).unwrap();
        write_remote_profiles(&path, std::slice::from_ref(&profile)).unwrap();
        let loaded = load_remote_profiles(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].host, "db.example.test");
        let persisted = fs::read_to_string(&path).unwrap();
        assert!(!persisted.contains("secret-value"));
        let url = remote_connection_url(&profile, "secret-value").unwrap();
        assert!(url.starts_with("postgresql://report%20user:secret-value@"));
        assert!(url.contains("sslmode=require"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opens_external_sqlite_and_excel_sources() {
        let root = std::env::temp_dir().join(format!("drpa-file-data-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();

        let sqlite_path = root.join("analytics.sqlite3");
        execute_sql_at(
            &sqlite_path,
            "CREATE TABLE metrics (name TEXT NOT NULL, value INTEGER)",
        )
        .unwrap();
        execute_sql_at(
            &sqlite_path,
            "INSERT INTO metrics(name, value) VALUES ('orders', 42)",
        )
        .unwrap();
        let external =
            execute_sql_external(&sqlite_path, "SELECT name, value FROM metrics").unwrap();
        assert_eq!(external.rows[0][0], "orders");
        assert_eq!(external.rows[0][1], 42);

        let workbook_path = root.join("report.xlsx");
        write_test_xlsx(&workbook_path);
        let workbook = open_excel_as_sqlite(&workbook_path).unwrap();
        let tables = list_tables_with_connection(&workbook).unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "Metrics");
        let columns = describe_table_with_connection(&workbook, "Metrics").unwrap();
        assert_eq!(
            columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            vec!["name", "value"]
        );
        let result =
            execute_sql_with_connection(&workbook, "SELECT name, value FROM \"Metrics\"").unwrap();
        assert_eq!(result.rows[0][0], "orders");
        assert_eq!(result.rows[0][1], 42.0);
        let schema = schema_context_with_connection(&workbook, "Excel 工作簿结构").unwrap();
        assert!(schema.contains("CREATE TABLE \"Metrics\""));

        let profile = RemoteDatabaseProfile {
            id: "database-excel".to_owned(),
            name: "Excel 报表".to_owned(),
            engine: "excel".to_owned(),
            host: String::new(),
            port: 0,
            database: workbook_path.to_string_lossy().into_owned(),
            username: String::new(),
            tls_mode: "prefer".to_owned(),
        };
        validate_remote_profile(&profile).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn maps_csv_tsv_and_json_files_to_read_only_query_tables() {
        let root = std::env::temp_dir().join(format!("drpa-tabular-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let csv_path = root.join("orders.csv");
        fs::write(
            &csv_path,
            "id,name,note\n1,alpha,\"line, one\"\n2,beta,\"two \"\"quotes\"\"\"\n",
        )
        .unwrap();
        let csv = open_delimited_as_sqlite(&csv_path).unwrap();
        let result =
            execute_sql_with_connection(&csv, "SELECT id, name, note FROM orders ORDER BY id")
                .unwrap();
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], 1);
        assert_eq!(result.rows[0][2], "line, one");
        assert_eq!(result.rows[1][2], "two \"quotes\"");

        let tsv_path = root.join("metrics.tsv");
        fs::write(&tsv_path, "name\tvalue\norders\t42\n").unwrap();
        let tsv = open_delimited_as_sqlite(&tsv_path).unwrap();
        let result = execute_sql_with_connection(&tsv, "SELECT value FROM metrics").unwrap();
        assert_eq!(result.rows[0][0], 42);

        let json_path = root.join("events.jsonl");
        fs::write(
            &json_path,
            "{\"id\":1,\"name\":\"created\"}\n{\"id\":2,\"name\":\"finished\",\"ok\":true}\n",
        )
        .unwrap();
        let json = open_json_as_sqlite(&json_path).unwrap();
        let result =
            execute_sql_with_connection(&json, "SELECT id, name, ok FROM events ORDER BY id")
                .unwrap();
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[1][1], "finished");
        assert_eq!(result.rows[1][2], 1);

        let _ = fs::remove_dir_all(root);
    }

    fn write_test_xlsx(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        let entries = [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Metrics" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>name</t></is></c><c r="B1" t="inlineStr"><is><t>value</t></is></c></row>
<row r="2"><c r="A2" t="inlineStr"><is><t>orders</t></is></c><c r="B2"><v>42</v></c></row>
</sheetData>
</worksheet>"#,
            ),
        ];
        for (name, content) in entries {
            archive.start_file(name, options).unwrap();
            archive.write_all(content.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    }
}
