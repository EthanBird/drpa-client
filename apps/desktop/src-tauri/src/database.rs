use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use calamine::{Data, Reader, open_workbook_auto};
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
const RESULT_BYTE_LIMIT: usize = 8 * 1024 * 1024;
const CELL_BYTE_LIMIT: usize = 256 * 1024;
const SCHEMA_CONTEXT_BYTE_LIMIT: usize = 100 * 1024;
const REMOTE_CONNECTION_LIMIT: usize = 50;
const REMOTE_TABLE_LIMIT: usize = 2_000;
const SCHEMA_TABLE_LIMIT: usize = 200;
const EXCEL_FILE_BYTE_LIMIT: u64 = 100 * 1024 * 1024;
const EXCEL_SHEET_LIMIT: usize = 100;
const EXCEL_ROW_LIMIT: usize = 100_000;
const EXCEL_COLUMN_LIMIT: usize = 512;

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseQueryResult {
    columns: Vec<String>,
    pub(crate) rows: Vec<Vec<serde_json::Value>>,
    affected_rows: usize,
    duration_ms: u64,
    truncated: bool,
    statement_type: String,
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
    paths: State<'_, AppPaths>,
) -> Result<DatabaseQueryResult, String> {
    execute_sql_at(&workspace_database_path(&paths), &sql)
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
    if profile.engine == "excel" {
        let connection = open_excel_as_sqlite(Path::new(&profile.database))?;
        return schema_context_with_connection(
            &connection,
            "Excel 工作簿结构（工作表映射为只读表）",
        );
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
    if profile.engine == "excel" {
        let connection = open_excel_as_sqlite(Path::new(&profile.database))?;
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
    if profile.engine == "excel" {
        let connection = open_excel_as_sqlite(Path::new(&profile.database))?;
        let sheet_count = list_tables_with_connection(&connection)?.len();
        return Ok(RemoteConnectionTest {
            server_version: format!("Excel 工作簿 · {sheet_count} 个工作表（只读）"),
            latency_ms: started.elapsed().as_millis() as u64,
        });
    }
    let pool = connect_remote_database(&profile, &password).await?;
    let version_sql = match profile.engine.as_str() {
        "postgresql" => "SELECT version()",
        "mysql" => "SELECT VERSION()",
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
    if profile.engine == "excel" {
        let connection = open_excel_as_sqlite(Path::new(&profile.database))?;
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
    if profile.engine == "excel" {
        let connection = open_excel_as_sqlite(Path::new(&profile.database))?;
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
    paths: State<'_, AppPaths>,
) -> Result<DatabaseQueryResult, String> {
    let profile = load_remote_profile(&remote_profiles_path(&paths), &profile_id)?;
    if profile.engine == "sqlite" {
        return execute_sql_external(Path::new(&profile.database), &sql);
    }
    if profile.engine == "excel" {
        let statement_type = first_sql_keyword(&sql);
        if !matches!(
            statement_type.as_str(),
            "SELECT" | "WITH" | "EXPLAIN" | "PRAGMA"
        ) {
            return Err(
                "Excel 工作簿是只读数据源，仅支持 SELECT、WITH、EXPLAIN 或 PRAGMA 查询".to_owned(),
            );
        }
        let connection = open_excel_as_sqlite(Path::new(&profile.database))?;
        return execute_sql_with_connection(&connection, &sql);
    }
    let pool = connect_remote_database(&profile, &password).await?;
    let result = execute_remote_sql_with_pool(&pool, &sql).await;
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
    if profile.engine == "excel" {
        let connection = open_excel_as_sqlite(Path::new(&profile.database))?;
        return schema_context_with_connection(
            &connection,
            "Excel 工作簿结构（工作表映射为只读表）",
        );
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
        "postgresql" | "mysql" | "sqlite" | "excel"
    ) {
        return Err("仅支持 PostgreSQL、MySQL、SQLite 或 Excel 数据源".to_owned());
    }
    if matches!(profile.engine.as_str(), "sqlite" | "excel") {
        let source = profile.database.trim();
        if source.is_empty() || source.len() > 32_767 {
            return Err("数据源文件路径无效".to_owned());
        }
        let extension = Path::new(source)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let valid_extension = if profile.engine == "sqlite" {
            matches!(extension.as_str(), "db" | "sqlite" | "sqlite3")
        } else {
            matches!(extension.as_str(), "xls" | "xlsx" | "xlsb" | "ods")
        };
        if !valid_extension {
            return Err(if profile.engine == "sqlite" {
                "SQLite 文件应使用 .db、.sqlite 或 .sqlite3 扩展名".to_owned()
            } else {
                "工作簿应使用 .xls、.xlsx、.xlsb 或 .ods 扩展名".to_owned()
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
        "mysql" => "mysql",
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
        "mysql" => (
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
        "mysql" => {
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
        "mysql" => {
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
        if profile.engine == "postgresql" {
            "PostgreSQL"
        } else {
            "MySQL"
        },
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
    if profile.engine == "mysql" {
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
        });
    }

    let remote_rows = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("SQL 查询失败：{error}"))?;
    Ok(remote_rows_to_result(
        remote_rows,
        statement_type,
        started.elapsed().as_millis() as u64,
    ))
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
        "mysql" => "START TRANSACTION READ ONLY",
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
    for row in remote_rows {
        if rows.len() >= RESULT_ROW_LIMIT {
            truncated = true;
            break;
        }
        let mut values = Vec::with_capacity(row.len());
        for index in 0..row.len() {
            let value = remote_value_to_json(&row, index);
            let value_bytes = value.to_string().len();
            if result_bytes.saturating_add(value_bytes) > RESULT_BYTE_LIMIT {
                truncated = true;
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

fn execute_sql_at(path: &Path, sql: &str) -> Result<DatabaseQueryResult, String> {
    let connection = open_database(path)?;
    execute_sql_with_connection(&connection, sql)
}

fn execute_sql_external(path: &Path, sql: &str) -> Result<DatabaseQueryResult, String> {
    let connection = open_external_database(path)?;
    execute_sql_with_connection(&connection, sql)
}

fn execute_sql_with_connection(
    connection: &Connection,
    sql: &str,
) -> Result<DatabaseQueryResult, String> {
    execute_sql_with_connection_mode(connection, sql, false)
}

fn execute_sql_with_connection_mode(
    connection: &Connection,
    sql: &str,
    read_only: bool,
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
    let mut result_bytes = 0_usize;
    'rows: while let Some(row) = cursor
        .next()
        .map_err(|error| format!("读取查询结果失败：{error}"))?
    {
        if rows.len() == RESULT_ROW_LIMIT {
            truncated = true;
            break;
        }
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let (value, cell_truncated) =
                sqlite_value_to_json(row.get_ref(index).map_err(|error| error.to_string())?);
            let value_bytes = value.to_string().len();
            if result_bytes.saturating_add(value_bytes) > RESULT_BYTE_LIMIT {
                truncated = true;
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
