use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, params};
use serde::Serialize;
use tauri::State;

use crate::AppPaths;

const RESULT_ROW_LIMIT: usize = 1_000;
const RESULT_BYTE_LIMIT: usize = 8 * 1024 * 1024;
const CELL_BYTE_LIMIT: usize = 256 * 1024;

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
    rows: Vec<Vec<serde_json::Value>>,
    affected_rows: usize,
    duration_ms: u64,
    truncated: bool,
    statement_type: String,
}

#[tauri::command]
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

#[tauri::command]
pub(crate) fn list_database_tables(
    paths: State<'_, AppPaths>,
) -> Result<Vec<DatabaseTable>, String> {
    list_tables_at(&workspace_database_path(&paths))
}

#[tauri::command]
pub(crate) fn describe_database_table(
    table_name: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<DatabaseColumn>, String> {
    describe_table_at(&workspace_database_path(&paths), &table_name)
}

#[tauri::command]
pub(crate) fn execute_database_sql(
    sql: String,
    paths: State<'_, AppPaths>,
) -> Result<DatabaseQueryResult, String> {
    execute_sql_at(&workspace_database_path(&paths), &sql)
}

#[tauri::command]
pub(crate) fn open_workspace_database_directory(paths: State<'_, AppPaths>) -> Result<(), String> {
    let directory = workspace_database_path(&paths)
        .parent()
        .ok_or_else(|| "数据库目录无效".to_owned())?
        .to_path_buf();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    crate::open_directory_in_file_explorer(&directory)
}

fn workspace_database_path(paths: &AppPaths) -> PathBuf {
    paths
        .workspace_root
        .join("databases")
        .join("workspace.sqlite3")
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

fn list_tables_at(path: &Path) -> Result<Vec<DatabaseTable>, String> {
    let connection = open_database(path)?;
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
    if table_name.trim().is_empty() {
        return Err("请选择数据表".to_owned());
    }
    let connection = open_database(path)?;
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

fn execute_sql_at(path: &Path, sql: &str) -> Result<DatabaseQueryResult, String> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err("请输入要执行的 SQL".to_owned());
    }
    let started = Instant::now();
    let connection = open_database(path)?;
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("SQL 编译失败：{error}"))?;
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

fn sqlite_value_to_json(value: ValueRef<'_>) -> (serde_json::Value, bool) {
    match value {
        ValueRef::Null => (serde_json::Value::Null, false),
        ValueRef::Integer(value) => (serde_json::Value::from(value), false),
        ValueRef::Real(value) => (
            serde_json::Number::from_f64(value)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            false,
        ),
        ValueRef::Text(value) => {
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
        ValueRef::Blob(value) => {
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
        let (value, truncated) = sqlite_value_to_json(ValueRef::Text(&oversized));
        assert!(truncated);
        assert!(value.as_str().unwrap().contains("已截断"));

        let _ = fs::remove_dir_all(root);
    }
}
