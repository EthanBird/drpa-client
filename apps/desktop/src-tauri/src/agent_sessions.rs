use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use drpa_package::validate_package_id;
use rusqlite::{
    Connection, DatabaseName, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::{AppPaths, agent_browser, agent_documents, jcode};

const SESSION_SCHEMA_VERSION: i64 = 2;
const DEFAULT_SESSION_TITLE: &str = "新对话";
const MAX_PROJECT_NAME_CHARS: usize = 120;
const MAX_SESSION_TITLE_CHARS: usize = 200;
const MAX_IDENTIFIER_CHARS: usize = 128;
const MAX_SKILL_ID_CHARS: usize = 256;
static SESSION_DATABASE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentProjectSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
    pub(crate) session_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSessionSummary {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) project_id: Option<String>,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
    pub(crate) revision: u64,
    pub(crate) message_count: usize,
    pub(crate) selected_skill_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSessionRecord {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) project_id: Option<String>,
    #[serde(default)]
    pub(crate) created_at: u64,
    #[serde(default)]
    pub(crate) updated_at: u64,
    #[serde(default)]
    pub(crate) revision: u64,
    #[serde(default)]
    pub(crate) messages: Vec<Value>,
    #[serde(default)]
    pub(crate) selected_skill_ids: Vec<String>,
}

#[tauri::command]
pub(crate) fn list_agent_projects(
    paths: State<'_, AppPaths>,
) -> Result<Vec<AgentProjectSummary>, String> {
    let _guard = lock_session_database()?;
    list_agent_projects_at(&paths.workspace_root)
}

#[tauri::command]
pub(crate) fn create_agent_project(
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<AgentProjectSummary, String> {
    let _guard = lock_session_database()?;
    create_agent_project_at(&paths.workspace_root, &name)
}

#[tauri::command]
pub(crate) fn rename_agent_project(
    project_id: String,
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<AgentProjectSummary, String> {
    let _guard = lock_session_database()?;
    rename_agent_project_at(&paths.workspace_root, &project_id, &name)
}

/// `project_id` has three useful forms:
/// - omitted (`None`): all sessions, newest activity first (the "recent" view);
/// - empty string: ordinary sessions that are not inside a project;
/// - a project id: only sessions inside that project.
#[tauri::command]
pub(crate) fn list_agent_sessions(
    project_id: Option<String>,
    paths: State<'_, AppPaths>,
) -> Result<Vec<AgentSessionSummary>, String> {
    let _guard = lock_session_database()?;
    list_agent_sessions_at(&paths.workspace_root, project_id.as_deref())
}

#[tauri::command]
pub(crate) fn create_agent_session(
    project_id: Option<String>,
    title: Option<String>,
    selected_skill_ids: Option<Vec<String>>,
    paths: State<'_, AppPaths>,
) -> Result<AgentSessionRecord, String> {
    let _guard = lock_session_database()?;
    create_agent_session_at(
        &paths.workspace_root,
        project_id.as_deref(),
        title.as_deref(),
        selected_skill_ids.unwrap_or_default(),
    )
}

#[tauri::command]
pub(crate) fn get_agent_session(
    session_id: String,
    paths: State<'_, AppPaths>,
) -> Result<AgentSessionRecord, String> {
    let _guard = lock_session_database()?;
    get_agent_session_at(&paths.workspace_root, &session_id)
}

/// Saves the complete message JSON without pruning or truncating it. Existing
/// browser-local sessions may use this command as an upsert during migration.
#[tauri::command]
pub(crate) fn save_agent_session(
    session: AgentSessionRecord,
    paths: State<'_, AppPaths>,
) -> Result<AgentSessionRecord, String> {
    let _guard = lock_session_database()?;
    save_agent_session_at(&paths.workspace_root, session)
}

#[tauri::command]
pub(crate) fn rename_agent_session(
    session_id: String,
    title: String,
    paths: State<'_, AppPaths>,
) -> Result<AgentSessionRecord, String> {
    let _guard = lock_session_database()?;
    rename_agent_session_at(&paths.workspace_root, &session_id, &title)
}

#[tauri::command]
pub(crate) fn move_agent_session(
    session_id: String,
    project_id: Option<String>,
    paths: State<'_, AppPaths>,
) -> Result<AgentSessionRecord, String> {
    let _guard = lock_session_database()?;
    move_agent_session_at(&paths.workspace_root, &session_id, project_id.as_deref())
}

#[tauri::command]
pub(crate) fn delete_agent_session(
    session_id: String,
    paths: State<'_, AppPaths>,
    browsers: State<'_, agent_browser::AgentBrowserManager>,
) -> Result<(), String> {
    let _guard = lock_session_database()?;
    let staged = agent_documents::stage_session_documents(&paths.workspace_root, &session_id)?;
    if let Err(error) = delete_agent_session_at(&paths.workspace_root, &session_id) {
        staged.rollback();
        return Err(error);
    }
    staged.commit()?;
    jcode::delete_session_data(&paths.workspace_root, &session_id)?;
    browsers.release(&session_id)
}

fn list_agent_projects_at(workspace_root: &Path) -> Result<Vec<AgentProjectSummary>, String> {
    let connection = open_database(workspace_root)?;
    let mut statement = connection
        .prepare(
            "SELECT p.id, p.name, p.created_at, p.updated_at, COUNT(s.id)
             FROM projects p
             LEFT JOIN sessions s ON s.project_id = p.id
             GROUP BY p.id, p.name, p.created_at, p.updated_at
             ORDER BY p.name COLLATE NOCASE ASC, p.created_at ASC",
        )
        .map_err(database_error("读取 Agent 项目失败"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(database_error("读取 Agent 项目失败"))?;

    rows.map(|row| {
        let (id, name, created_at, updated_at, session_count) =
            row.map_err(database_error("解析 Agent 项目失败"))?;
        Ok(AgentProjectSummary {
            path: project_path(workspace_root, &id)
                .to_string_lossy()
                .into_owned(),
            id,
            name,
            created_at: nonnegative_u64(created_at),
            updated_at: nonnegative_u64(updated_at),
            session_count: nonnegative_usize(session_count),
        })
    })
    .collect()
}

fn create_agent_project_at(
    workspace_root: &Path,
    name: &str,
) -> Result<AgentProjectSummary, String> {
    let name = validate_project_name(name)?;
    let id = generated_project_id();
    let now = now_millis();
    let project_root = project_path(workspace_root, &id);
    let projects_root = workspace_root.join("projects");
    ensure_real_directory(&projects_root, "项目根目录")?;
    if fs::symlink_metadata(&project_root).is_ok() {
        return Err("项目目录发生冲突，请重试".to_owned());
    }

    let mut connection = open_database(workspace_root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error("创建 Agent 项目事务失败"))?;
    transaction
        .execute(
            "INSERT INTO projects (id, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![id, name, millis_i64(now)],
        )
        .map_err(database_error("创建 Agent 项目失败"))?;

    fs::create_dir(&project_root).map_err(|error| format!("创建项目目录失败：{error}"))?;
    if let Err(error) = harden_directory_permissions(&project_root) {
        let _ = fs::remove_dir(&project_root);
        return Err(error);
    }
    if let Err(error) = transaction.commit() {
        let _ = fs::remove_dir(&project_root);
        return Err(format!("提交 Agent 项目失败：{error}"));
    }

    Ok(AgentProjectSummary {
        id,
        name,
        path: project_root.to_string_lossy().into_owned(),
        created_at: now,
        updated_at: now,
        session_count: 0,
    })
}

fn rename_agent_project_at(
    workspace_root: &Path,
    project_id: &str,
    name: &str,
) -> Result<AgentProjectSummary, String> {
    validate_project_identifier(project_id)?;
    let name = validate_project_name(name)?;
    let now = now_millis();
    let connection = open_database(workspace_root)?;
    let changed = connection
        .execute(
            "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, millis_i64(now), project_id],
        )
        .map_err(database_error("重命名 Agent 项目失败"))?;
    if changed == 0 {
        return Err("Agent 项目不存在".to_owned());
    }
    get_agent_project(&connection, workspace_root, project_id)
}

#[cfg(test)]
fn delete_agent_project_at(workspace_root: &Path, project_id: &str) -> Result<(), String> {
    validate_project_identifier(project_id)?;
    let project_root = project_path(workspace_root, project_id);
    let projects_root = workspace_root.join("projects");
    ensure_real_directory(&projects_root, "项目根目录")?;

    let mut connection = open_database(workspace_root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error("删除 Agent 项目事务失败"))?;
    ensure_project_exists_in(&transaction, project_id)?;

    let staged_path = projects_root.join(format!(
        ".drpa-deleting-{project_id}-{}",
        Uuid::new_v4().simple()
    ));
    let staged = match fs::symlink_metadata(&project_root) {
        Ok(_) => {
            fs::rename(&project_root, &staged_path)
                .map_err(|error| format!("暂存待删除项目目录失败：{error}"))?;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("检查项目目录失败：{error}")),
    };

    if let Err(error) = transaction.execute("DELETE FROM projects WHERE id = ?1", [project_id]) {
        if staged {
            let _ = fs::rename(&staged_path, &project_root);
        }
        return Err(format!("删除 Agent 项目失败：{error}"));
    }
    if let Err(error) = transaction.commit() {
        if staged {
            let _ = fs::rename(&staged_path, &project_root);
        }
        return Err(format!("提交 Agent 项目删除失败：{error}"));
    }

    if staged {
        remove_path(&staged_path)
            .map_err(|error| format!("项目记录已删除，但清理项目目录失败：{error}"))?;
    }
    Ok(())
}

fn list_agent_sessions_at(
    workspace_root: &Path,
    project_filter: Option<&str>,
) -> Result<Vec<AgentSessionSummary>, String> {
    let connection = open_database(workspace_root)?;
    let normalized_filter = project_filter.map(str::trim);
    if let Some(project_id) = normalized_filter.filter(|value| !value.is_empty()) {
        validate_project_identifier(project_id)?;
        ensure_project_exists(&connection, project_id)?;
    }

    let (mode, project_id) = match normalized_filter {
        None => (0_i64, None),
        Some("") => (1_i64, None),
        Some(value) => (2_i64, Some(value)),
    };
    let mut statement = connection
        .prepare(
            "SELECT id, title, project_id, created_at, updated_at, message_count,
                    selected_skills_json, revision
             FROM sessions
             WHERE ?1 = 0
                OR (?1 = 1 AND project_id IS NULL)
                OR (?1 = 2 AND project_id = ?2)
             ORDER BY updated_at DESC, created_at DESC, id ASC",
        )
        .map_err(database_error("读取 Agent 会话失败"))?;
    let rows = statement
        .query_map(params![mode, project_id], session_summary_from_row)
        .map_err(database_error("读取 Agent 会话失败"))?;
    rows.map(|row| row.map_err(database_error("解析 Agent 会话失败")))
        .collect()
}

fn create_agent_session_at(
    workspace_root: &Path,
    project_id: Option<&str>,
    title: Option<&str>,
    selected_skill_ids: Vec<String>,
) -> Result<AgentSessionRecord, String> {
    let project_id = normalize_project_id(project_id)?;
    let title = validate_session_title(title.unwrap_or(DEFAULT_SESSION_TITLE))?;
    let selected_skill_ids = normalize_skill_ids(selected_skill_ids)?;
    let now = now_millis();
    let session = AgentSessionRecord {
        id: format!("agent-{}", Uuid::new_v4().simple()),
        title,
        project_id,
        created_at: now,
        updated_at: now,
        revision: 1,
        messages: Vec::new(),
        selected_skill_ids,
    };
    insert_session(workspace_root, &session)?;
    Ok(session)
}

fn get_agent_session_at(
    workspace_root: &Path,
    session_id: &str,
) -> Result<AgentSessionRecord, String> {
    validate_identifier(session_id, "会话 ID")?;
    let connection = open_database(workspace_root)?;
    connection
        .query_row(
            "SELECT id, title, project_id, created_at, updated_at, messages_json,
                    selected_skills_json, revision
             FROM sessions WHERE id = ?1",
            [session_id],
            session_record_from_row,
        )
        .optional()
        .map_err(database_error("读取 Agent 会话失败"))?
        .ok_or_else(|| "Agent 会话不存在".to_owned())
}

fn save_agent_session_at(
    workspace_root: &Path,
    mut session: AgentSessionRecord,
) -> Result<AgentSessionRecord, String> {
    validate_identifier(&session.id, "会话 ID")?;
    session.title = validate_session_title(&session.title)?;
    session.project_id = normalize_project_id(session.project_id.as_deref())?;
    session.selected_skill_ids = normalize_skill_ids(session.selected_skill_ids)?;
    let now = now_millis();
    if session.created_at == 0 {
        session.created_at = now;
    }
    if session.updated_at == 0 {
        session.updated_at = now;
    }
    session.updated_at = session.updated_at.max(session.created_at);
    let messages_json = serde_json::to_string(&session.messages)
        .map_err(|error| format!("序列化 Agent 会话消息失败：{error}"))?;
    let skills_json = serde_json::to_string(&session.selected_skill_ids)
        .map_err(|error| format!("序列化会话技能失败：{error}"))?;

    let mut connection = open_database(workspace_root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error("保存 Agent 会话事务失败"))?;
    if let Some(project_id) = session.project_id.as_deref() {
        ensure_or_register_project_in(&transaction, workspace_root, project_id)?;
    }
    let current_revision = transaction
        .query_row(
            "SELECT revision FROM sessions WHERE id = ?1",
            [&session.id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(database_error("读取 Agent 会话版本失败"))?
        .map(nonnegative_u64);
    match current_revision {
        Some(current) if session.revision != current => {
            return Err(format!(
                "Agent 会话版本冲突：客户端 revision={}，数据库 revision={current}；请重新载入会话",
                session.revision
            ));
        }
        None if session.revision > 0 => {
            return Err("Agent 会话已删除，已阻止旧快照重新创建会话".to_owned());
        }
        _ => {}
    }
    let next_revision = current_revision.unwrap_or(0).saturating_add(1);
    if current_revision.is_some() {
        transaction
            .execute(
                "UPDATE sessions SET
                    project_id = ?1,
                    title = ?2,
                    messages_json = ?3,
                    selected_skills_json = ?4,
                    message_count = ?5,
                    updated_at = ?6,
                    revision = ?7
                 WHERE id = ?8",
                params![
                    session.project_id,
                    session.title,
                    messages_json,
                    skills_json,
                    usize_i64(session.messages.len())?,
                    millis_i64(session.updated_at),
                    millis_i64(next_revision),
                    session.id,
                ],
            )
            .map_err(database_error("保存 Agent 会话失败"))?;
    } else {
        transaction
            .execute(
                "INSERT INTO sessions (
                    id, project_id, title, messages_json, selected_skills_json,
                    message_count, created_at, updated_at, revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    session.id,
                    session.project_id,
                    session.title,
                    messages_json,
                    skills_json,
                    usize_i64(session.messages.len())?,
                    millis_i64(session.created_at),
                    millis_i64(session.updated_at),
                    millis_i64(next_revision),
                ],
            )
            .map_err(database_error("保存 Agent 会话失败"))?;
    }
    session.revision = next_revision;
    transaction
        .commit()
        .map_err(database_error("提交 Agent 会话失败"))?;
    Ok(session)
}

fn rename_agent_session_at(
    workspace_root: &Path,
    session_id: &str,
    title: &str,
) -> Result<AgentSessionRecord, String> {
    validate_identifier(session_id, "会话 ID")?;
    let title = validate_session_title(title)?;
    let connection = open_database(workspace_root)?;
    let changed = connection
        .execute(
            "UPDATE sessions SET title = ?1, updated_at = ?2, revision = revision + 1 WHERE id = ?3",
            params![title, millis_i64(now_millis()), session_id],
        )
        .map_err(database_error("重命名 Agent 会话失败"))?;
    if changed == 0 {
        return Err("Agent 会话不存在".to_owned());
    }
    get_agent_session_with(&connection, session_id)
}

fn move_agent_session_at(
    workspace_root: &Path,
    session_id: &str,
    project_id: Option<&str>,
) -> Result<AgentSessionRecord, String> {
    validate_identifier(session_id, "会话 ID")?;
    let project_id = normalize_project_id(project_id)?;
    let mut connection = open_database(workspace_root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error("移动 Agent 会话事务失败"))?;
    if let Some(project_id) = project_id.as_deref() {
        ensure_or_register_project_in(&transaction, workspace_root, project_id)?;
    }
    let changed = transaction
        .execute(
            "UPDATE sessions SET project_id = ?1, updated_at = ?2, revision = revision + 1 WHERE id = ?3",
            params![project_id, millis_i64(now_millis()), session_id],
        )
        .map_err(database_error("移动 Agent 会话失败"))?;
    if changed == 0 {
        return Err("Agent 会话不存在".to_owned());
    }
    transaction
        .commit()
        .map_err(database_error("提交 Agent 会话移动失败"))?;
    get_agent_session_at(workspace_root, session_id)
}

fn delete_agent_session_at(workspace_root: &Path, session_id: &str) -> Result<(), String> {
    validate_identifier(session_id, "会话 ID")?;
    let connection = open_database(workspace_root)?;
    let changed = connection
        .execute("DELETE FROM sessions WHERE id = ?1", [session_id])
        .map_err(database_error("删除 Agent 会话失败"))?;
    if changed == 0 {
        return Err("Agent 会话不存在".to_owned());
    }
    Ok(())
}

fn insert_session(workspace_root: &Path, session: &AgentSessionRecord) -> Result<(), String> {
    let messages_json = serde_json::to_string(&session.messages)
        .map_err(|error| format!("序列化 Agent 会话消息失败：{error}"))?;
    let skills_json = serde_json::to_string(&session.selected_skill_ids)
        .map_err(|error| format!("序列化会话技能失败：{error}"))?;
    let mut connection = open_database(workspace_root)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error("创建 Agent 会话事务失败"))?;
    if let Some(project_id) = session.project_id.as_deref() {
        ensure_or_register_project_in(&transaction, workspace_root, project_id)?;
    }
    transaction
        .execute(
            "INSERT INTO sessions (
                id, project_id, title, messages_json, selected_skills_json,
                message_count, created_at, updated_at, revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session.id,
                session.project_id,
                session.title,
                messages_json,
                skills_json,
                usize_i64(session.messages.len())?,
                millis_i64(session.created_at),
                millis_i64(session.updated_at),
                millis_i64(session.revision.max(1)),
            ],
        )
        .map_err(database_error("创建 Agent 会话失败"))?;
    transaction
        .commit()
        .map_err(database_error("提交 Agent 会话失败"))
}

fn get_agent_project(
    connection: &Connection,
    workspace_root: &Path,
    project_id: &str,
) -> Result<AgentProjectSummary, String> {
    connection
        .query_row(
            "SELECT p.id, p.name, p.created_at, p.updated_at, COUNT(s.id)
             FROM projects p
             LEFT JOIN sessions s ON s.project_id = p.id
             WHERE p.id = ?1
             GROUP BY p.id, p.name, p.created_at, p.updated_at",
            [project_id],
            |row| {
                let id = row.get::<_, String>(0)?;
                Ok(AgentProjectSummary {
                    path: project_path(workspace_root, &id)
                        .to_string_lossy()
                        .into_owned(),
                    id,
                    name: row.get(1)?,
                    created_at: nonnegative_u64(row.get(2)?),
                    updated_at: nonnegative_u64(row.get(3)?),
                    session_count: nonnegative_usize(row.get(4)?),
                })
            },
        )
        .optional()
        .map_err(database_error("读取 Agent 项目失败"))?
        .ok_or_else(|| "Agent 项目不存在".to_owned())
}

fn get_agent_session_with(
    connection: &Connection,
    session_id: &str,
) -> Result<AgentSessionRecord, String> {
    connection
        .query_row(
            "SELECT id, title, project_id, created_at, updated_at, messages_json,
                    selected_skills_json, revision
             FROM sessions WHERE id = ?1",
            [session_id],
            session_record_from_row,
        )
        .optional()
        .map_err(database_error("读取 Agent 会话失败"))?
        .ok_or_else(|| "Agent 会话不存在".to_owned())
}

fn session_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSessionRecord> {
    let messages_json = row.get::<_, String>(5)?;
    let skills_json = row.get::<_, String>(6)?;
    Ok(AgentSessionRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        project_id: row.get(2)?,
        created_at: nonnegative_u64(row.get(3)?),
        updated_at: nonnegative_u64(row.get(4)?),
        revision: nonnegative_u64(row.get(7)?),
        messages: parse_json_column(&messages_json, 5)?,
        selected_skill_ids: parse_json_column(&skills_json, 6)?,
    })
}

fn session_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSessionSummary> {
    let skills_json = row.get::<_, String>(6)?;
    Ok(AgentSessionSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        project_id: row.get(2)?,
        created_at: nonnegative_u64(row.get(3)?),
        updated_at: nonnegative_u64(row.get(4)?),
        revision: nonnegative_u64(row.get(7)?),
        message_count: nonnegative_usize(row.get(5)?),
        selected_skill_ids: parse_json_column(&skills_json, 6)?,
    })
}

fn parse_json_column<T>(source: &str, column: usize) -> rusqlite::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(source).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

pub(crate) fn lock_session_database() -> Result<MutexGuard<'static, ()>, String> {
    SESSION_DATABASE_LOCK
        .lock()
        .map_err(|_| "Agent 会话数据库锁不可用".to_owned())
}

pub(crate) fn create_export_snapshot(
    workspace_root: &Path,
    destination: &Path,
) -> Result<bool, String> {
    let _guard = lock_session_database()?;
    if !workspace_root.join("agent").join("session.db").is_file() {
        return Ok(false);
    }
    let connection = open_database(workspace_root)?;
    connection
        .backup(DatabaseName::Main, destination, None)
        .map_err(database_error("创建 Agent 会话导出快照失败"))?;
    Ok(true)
}

fn open_database(workspace_root: &Path) -> Result<Connection, String> {
    let agent_root = workspace_root.join("agent");
    ensure_real_directory(&agent_root, "Agent 数据目录")?;
    harden_directory_permissions(&agent_root)?;
    let path = agent_root.join("session.db");
    let mut connection =
        Connection::open(&path).map_err(database_error("打开 Agent 会话数据库失败"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(database_error("配置 Agent 会话数据库失败"))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(database_error("初始化 Agent 会话数据库失败"))?;
    migrate_database(&mut connection)?;
    harden_file_permissions(&path)?;
    Ok(connection)
}

fn migrate_database(connection: &mut Connection) -> Result<(), String> {
    let version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(database_error("读取 Agent 会话数据库版本失败"))?;
    if version > SESSION_SCHEMA_VERSION {
        return Err(format!(
            "Agent 会话数据库版本 {version} 高于当前支持的 {SESSION_SCHEMA_VERSION}，请升级应用"
        ));
    }
    if version == SESSION_SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error("迁移 Agent 会话数据库失败"))?;
    if version < 1 {
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS projects (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY NOT NULL,
                    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
                    title TEXT NOT NULL,
                    messages_json TEXT NOT NULL DEFAULT '[]'
                        CHECK (json_valid(messages_json) AND json_type(messages_json) = 'array'),
                    selected_skills_json TEXT NOT NULL DEFAULT '[]'
                        CHECK (
                            json_valid(selected_skills_json)
                            AND json_type(selected_skills_json) = 'array'
                        ),
                    message_count INTEGER NOT NULL DEFAULT 0 CHECK (message_count >= 0),
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1)
                 );
                 CREATE INDEX IF NOT EXISTS idx_agent_sessions_updated
                    ON sessions(updated_at DESC, created_at DESC);
                 CREATE INDEX IF NOT EXISTS idx_agent_sessions_project_updated
                    ON sessions(project_id, updated_at DESC, created_at DESC);
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_projects_name_nocase
                    ON projects(name COLLATE NOCASE);",
            )
            .map_err(database_error("创建 Agent 会话数据库结构失败"))?;
    }
    if version < 2 && version >= 1 {
        transaction
            .execute_batch(
                "ALTER TABLE sessions ADD COLUMN revision INTEGER NOT NULL DEFAULT 1
                    CHECK (revision >= 1);",
            )
            .map_err(database_error("增加 Agent 会话版本列失败"))?;
    }
    transaction
        .pragma_update(None, "user_version", SESSION_SCHEMA_VERSION)
        .map_err(database_error("更新 Agent 会话数据库版本失败"))?;
    transaction
        .commit()
        .map_err(database_error("提交 Agent 会话数据库迁移失败"))
}

fn ensure_project_exists(connection: &Connection, project_id: &str) -> Result<(), String> {
    let exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            [project_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(database_error("检查 Agent 项目失败"))?;
    if exists {
        Ok(())
    } else {
        Err("Agent 项目不存在".to_owned())
    }
}

fn ensure_project_exists_in(transaction: &Transaction<'_>, project_id: &str) -> Result<(), String> {
    let exists = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            [project_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(database_error("检查 Agent 项目失败"))?;
    if exists {
        Ok(())
    } else {
        Err("Agent 项目不存在".to_owned())
    }
}

fn ensure_or_register_project_in(
    transaction: &Transaction<'_>,
    workspace_root: &Path,
    project_id: &str,
) -> Result<(), String> {
    match ensure_project_exists_in(transaction, project_id) {
        Ok(()) => return Ok(()),
        Err(error) if error == "Agent 项目不存在" => {}
        Err(error) => return Err(error),
    }
    validate_project_identifier(project_id)?;
    let root = project_path(workspace_root, project_id);
    let metadata = fs::symlink_metadata(&root)
        .map_err(|_| "Agent 项目不存在，且未找到可关联的开发项目目录".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("可关联的开发项目目录类型无效".to_owned());
    }
    let now = now_millis();
    transaction
        .execute(
            "INSERT INTO projects (id, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![
                project_id,
                format!("已关联项目 {}", project_id),
                millis_i64(now)
            ],
        )
        .map_err(database_error("关联现有开发项目失败"))?;
    Ok(())
}

fn validate_project_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名称不能为空".to_owned());
    }
    if name.chars().count() > MAX_PROJECT_NAME_CHARS {
        return Err(format!("项目名称不能超过 {MAX_PROJECT_NAME_CHARS} 个字符"));
    }
    if name.chars().any(char::is_control) {
        return Err("项目名称不能包含控制字符".to_owned());
    }
    Ok(name.to_owned())
}

fn validate_session_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(DEFAULT_SESSION_TITLE.to_owned());
    }
    if title.chars().count() > MAX_SESSION_TITLE_CHARS {
        return Err(format!("会话标题不能超过 {MAX_SESSION_TITLE_CHARS} 个字符"));
    }
    if title.chars().any(char::is_control) {
        return Err("会话标题不能包含控制字符".to_owned());
    }
    Ok(title.to_owned())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(format!("{label} 无效"))
    }
}

fn validate_project_identifier(value: &str) -> Result<(), String> {
    let generated = value
        .strip_prefix("project-")
        .is_some_and(|hash| hash.len() == 24 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let valid = !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_CHARS
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\'])
        && (generated || validate_package_id(value).is_ok());
    if valid {
        Ok(())
    } else {
        Err("项目 ID 无效".to_owned())
    }
}

fn normalize_project_id(project_id: Option<&str>) -> Result<Option<String>, String> {
    let project_id = project_id.map(str::trim).filter(|value| !value.is_empty());
    if let Some(project_id) = project_id {
        validate_project_identifier(project_id)?;
        Ok(Some(project_id.to_owned()))
    } else {
        Ok(None)
    }
}

fn normalize_skill_ids(skill_ids: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(skill_ids.len());
    for skill_id in skill_ids {
        let skill_id = skill_id.trim();
        if skill_id.is_empty() {
            continue;
        }
        if skill_id.chars().count() > MAX_SKILL_ID_CHARS || skill_id.chars().any(char::is_control) {
            return Err("技能 ID 无效".to_owned());
        }
        if seen.insert(skill_id.to_owned()) {
            normalized.push(skill_id.to_owned());
        }
    }
    Ok(normalized)
}

fn generated_project_id() -> String {
    let simple = Uuid::new_v4().simple().to_string();
    format!("project-{}", &simple[..24])
}

fn project_path(workspace_root: &Path, project_id: &str) -> PathBuf {
    workspace_root.join("projects").join(project_id)
}

fn ensure_real_directory(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!("{label}不能是符号链接")),
        Ok(metadata) if !metadata.is_dir() => Err(format!("{label}不是目录")),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| format!("创建{label}失败：{error}"))
        }
        Err(error) => Err(format!("检查{label}失败：{error}")),
    }
}

#[cfg(test)]
fn remove_path(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn millis_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn nonnegative_u64(value: i64) -> u64 {
    value.max(0) as u64
}

fn nonnegative_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

fn usize_i64(value: usize) -> Result<i64, String> {
    i64::try_from(value).map_err(|_| "会话消息数量超出 SQLite 支持范围".to_owned())
}

fn database_error(prefix: &'static str) -> impl FnOnce(rusqlite::Error) -> String {
    move |error| format!("{prefix}：{error}")
}

#[cfg(unix)]
fn harden_directory_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("设置 Agent 数据目录权限失败：{error}"))
}

#[cfg(not(unix))]
fn harden_directory_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn harden_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("设置 Agent 会话数据库权限失败：{error}"))
}

#[cfg(not(unix))]
fn harden_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "drpa-agent-sessions-{label}-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn persists_complete_messages_and_selected_skills() {
        let workspace = TestWorkspace::new("roundtrip");
        let project = create_agent_project_at(&workspace.root, "数据分析").unwrap();
        assert!(Path::new(&project.path).is_dir());
        assert!(!Path::new(&project.path).join("manifest.yaml").exists());

        let mut session = create_agent_session_at(
            &workspace.root,
            Some(&project.id),
            Some("季度分析"),
            vec!["data-analysis".to_owned(), "python".to_owned()],
        )
        .unwrap();
        session.messages = vec![
            json!({"id":"m1","role":"user","content":"分析数据"}),
            json!({
                "id":"m2",
                "role":"assistant",
                "content":[{"type":"text","text":"完成"}],
                "tools":[{"name":"python","output":{"rows":[1,2,3]}}]
            }),
        ];
        session.updated_at += 10;
        save_agent_session_at(&workspace.root, session.clone()).unwrap();

        let loaded = get_agent_session_at(&workspace.root, &session.id).unwrap();
        assert_eq!(loaded.messages, session.messages);
        assert_eq!(loaded.selected_skill_ids, session.selected_skill_ids);
        let summaries = list_agent_sessions_at(&workspace.root, Some(&project.id)).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].message_count, 2);
        assert_eq!(summaries[0].selected_skill_ids, session.selected_skill_ids);
    }

    #[test]
    fn supports_unlimited_retention_and_session_filters() {
        let workspace = TestWorkspace::new("unlimited");
        let project = create_agent_project_at(&workspace.root, "项目会话").unwrap();
        for index in 0..75 {
            let project_id = (index % 2 == 0).then_some(project.id.as_str());
            create_agent_session_at(
                &workspace.root,
                project_id,
                Some(&format!("会话 {index}")),
                Vec::new(),
            )
            .unwrap();
        }

        assert_eq!(
            list_agent_sessions_at(&workspace.root, None).unwrap().len(),
            75
        );
        assert_eq!(
            list_agent_sessions_at(&workspace.root, Some(""))
                .unwrap()
                .len(),
            37
        );
        assert_eq!(
            list_agent_sessions_at(&workspace.root, Some(&project.id))
                .unwrap()
                .len(),
            38
        );
    }

    #[test]
    fn projects_and_databases_are_workspace_isolated() {
        let first = TestWorkspace::new("isolated-first");
        let second = TestWorkspace::new("isolated-second");
        let project = create_agent_project_at(&first.root, "只在工作区一").unwrap();
        create_agent_session_at(&first.root, Some(&project.id), Some("私有会话"), Vec::new())
            .unwrap();

        assert_eq!(list_agent_projects_at(&first.root).unwrap().len(), 1);
        assert_eq!(list_agent_projects_at(&second.root).unwrap().len(), 0);
        assert_eq!(list_agent_sessions_at(&first.root, None).unwrap().len(), 1);
        assert_eq!(list_agent_sessions_at(&second.root, None).unwrap().len(), 0);
        assert!(first.root.join("agent/session.db").is_file());
        assert!(second.root.join("agent/session.db").is_file());
    }

    #[test]
    fn rejects_dot_segments_and_creates_consistent_export_snapshots() {
        assert!(normalize_project_id(Some(".")).is_err());
        assert!(normalize_project_id(Some("..")).is_err());
        assert_eq!(
            normalize_project_id(Some("com.example.agent")).unwrap(),
            Some("com.example.agent".to_owned())
        );

        let workspace = TestWorkspace::new("snapshot");
        create_agent_session_at(&workspace.root, None, Some("快照会话"), Vec::new()).unwrap();
        let snapshot = workspace.root.join("export-session.db");
        assert!(create_export_snapshot(&workspace.root, &snapshot).unwrap());
        let connection = Connection::open(snapshot).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn deleting_a_project_cascades_sessions_and_removes_its_directory() {
        let workspace = TestWorkspace::new("delete");
        let project = create_agent_project_at(&workspace.root, "临时项目").unwrap();
        let project_path = PathBuf::from(&project.path);
        fs::write(project_path.join("analysis.py"), "print('ok')").unwrap();
        create_agent_session_at(
            &workspace.root,
            Some(&project.id),
            Some("会被删除"),
            Vec::new(),
        )
        .unwrap();

        delete_agent_project_at(&workspace.root, &project.id).unwrap();

        assert!(!project_path.exists());
        assert!(list_agent_projects_at(&workspace.root).unwrap().is_empty());
        assert!(
            list_agent_sessions_at(&workspace.root, None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn save_upserts_browser_local_sessions_and_moves_them() {
        let workspace = TestWorkspace::new("upsert");
        let project = create_agent_project_at(&workspace.root, "迁移目标").unwrap();
        let imported = AgentSessionRecord {
            id: "agent-1700000000-deadbeef".to_owned(),
            title: "浏览器旧会话".to_owned(),
            project_id: None,
            created_at: 1_700_000_000,
            updated_at: 1_700_000_100,
            revision: 0,
            messages: vec![json!({"id":"old","role":"user","content":"保留我"})],
            selected_skill_ids: vec!["data-analysis".to_owned()],
        };
        save_agent_session_at(&workspace.root, imported.clone()).unwrap();
        let moved =
            move_agent_session_at(&workspace.root, &imported.id, Some(&project.id)).unwrap();
        assert_eq!(moved.project_id.as_deref(), Some(project.id.as_str()));

        let renamed = rename_agent_session_at(&workspace.root, &imported.id, "已迁移").unwrap();
        assert_eq!(renamed.title, "已迁移");
        assert_eq!(renamed.messages, imported.messages);
    }

    #[test]
    fn rejects_stale_saves_and_does_not_resurrect_deleted_sessions() {
        let workspace = TestWorkspace::new("optimistic-concurrency");
        let created =
            create_agent_session_at(&workspace.root, None, Some("版本化会话"), Vec::new()).unwrap();
        let mut first_writer = get_agent_session_at(&workspace.root, &created.id).unwrap();
        let mut stale_writer = first_writer.clone();

        first_writer.messages = vec![json!({"id":"m1","role":"user","content":"第一位写入者"})];
        first_writer.updated_at += 1;
        let saved = save_agent_session_at(&workspace.root, first_writer).unwrap();
        assert_eq!(saved.revision, created.revision + 1);

        stale_writer.messages = vec![json!({"id":"m2","role":"user","content":"旧快照"})];
        stale_writer.updated_at += 2;
        let conflict = save_agent_session_at(&workspace.root, stale_writer.clone()).unwrap_err();
        assert!(conflict.contains("版本冲突"));
        assert_eq!(
            get_agent_session_at(&workspace.root, &created.id)
                .unwrap()
                .messages,
            saved.messages
        );

        delete_agent_session_at(&workspace.root, &created.id).unwrap();
        let resurrection = save_agent_session_at(&workspace.root, stale_writer).unwrap_err();
        assert!(resurrection.contains("已删除"));
        assert!(get_agent_session_at(&workspace.root, &created.id).is_err());
    }

    #[test]
    fn rejects_a_database_created_by_a_newer_application() {
        let workspace = TestWorkspace::new("future-schema");
        let agent_root = workspace.root.join("agent");
        fs::create_dir_all(&agent_root).unwrap();
        let connection = Connection::open(agent_root.join("session.db")).unwrap();
        connection
            .pragma_update(None, "user_version", SESSION_SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);

        let error = open_database(&workspace.root).unwrap_err();
        assert!(error.contains("高于当前支持"));
    }
}
