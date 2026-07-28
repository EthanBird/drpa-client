use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use drpa_host::HostState;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::AppPaths;

const REGISTRY_SCHEMA: u32 = 1;
const PERSONAL_WORKSPACE_ID: &str = "personal";
const PERSONAL_WORKSPACE_NAME: &str = "个人工作区";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceRecord {
    id: String,
    name: String,
    created_at: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceRegistry {
    schema: u32,
    active_id: String,
    workspaces: Vec<WorkspaceRecord>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceInfo {
    id: String,
    name: String,
    path: String,
    active: bool,
    created_at: u64,
}

impl Default for WorkspaceRegistry {
    fn default() -> Self {
        Self {
            schema: REGISTRY_SCHEMA,
            active_id: PERSONAL_WORKSPACE_ID.to_owned(),
            workspaces: vec![WorkspaceRecord {
                id: PERSONAL_WORKSPACE_ID.to_owned(),
                name: PERSONAL_WORKSPACE_NAME.to_owned(),
                created_at: now_millis(),
            }],
        }
    }
}

pub(crate) fn resolve_active_workspace(data_root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(data_root).map_err(|error| format!("创建数据目录失败：{error}"))?;
    let mut registry = load_registry(data_root)?;
    normalize_registry(&mut registry);
    let workspace_root = workspace_root(data_root, &registry.active_id)?;
    fs::create_dir_all(&workspace_root).map_err(|error| format!("创建工作区目录失败：{error}"))?;
    write_registry(data_root, &registry)?;
    Ok(workspace_root)
}

#[tauri::command]
pub(crate) fn list_workspaces(paths: State<'_, AppPaths>) -> Result<Vec<WorkspaceInfo>, String> {
    let mut registry = load_registry(&paths.data_root)?;
    normalize_registry(&mut registry);
    write_registry(&paths.data_root, &registry)?;
    registry
        .workspaces
        .iter()
        .map(|record| workspace_info(&paths.data_root, &registry.active_id, record))
        .collect()
}

#[tauri::command]
pub(crate) fn create_workspace(
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<WorkspaceInfo, String> {
    create_workspace_inner(&paths.data_root, &name)
}

#[tauri::command]
pub(crate) fn switch_workspace(
    workspace_id: String,
    app: tauri::AppHandle,
    paths: State<'_, AppPaths>,
    host: State<'_, HostState>,
) -> Result<(), String> {
    if host.snapshot().stats.active_runs > 0 {
        return Err("存在正在运行的任务，请等待任务结束后再切换工作区".to_owned());
    }
    validate_workspace_id(&workspace_id)?;
    let mut registry = load_registry(&paths.data_root)?;
    normalize_registry(&mut registry);
    if !registry
        .workspaces
        .iter()
        .any(|workspace| workspace.id == workspace_id)
    {
        return Err("目标工作区不存在".to_owned());
    }
    if registry.active_id == workspace_id {
        return Ok(());
    }
    let root = workspace_root(&paths.data_root, &workspace_id)?;
    fs::create_dir_all(root).map_err(|error| format!("创建工作区目录失败：{error}"))?;
    registry.active_id = workspace_id;
    write_registry(&paths.data_root, &registry)?;

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        app.request_restart();
    });
    Ok(())
}

fn create_workspace_inner(data_root: &Path, name: &str) -> Result<WorkspaceInfo, String> {
    let name = validate_workspace_name(name)?;
    let mut registry = load_registry(data_root)?;
    normalize_registry(&mut registry);
    if registry
        .workspaces
        .iter()
        .any(|workspace| workspace.name.eq_ignore_ascii_case(&name))
    {
        return Err("已存在同名工作区".to_owned());
    }
    let record = WorkspaceRecord {
        id: format!("workspace-{}", Uuid::new_v4().simple()),
        name,
        created_at: now_millis(),
    };
    let root = workspace_root(data_root, &record.id)?;
    fs::create_dir_all(&root).map_err(|error| format!("创建工作区目录失败：{error}"))?;
    registry.workspaces.push(record.clone());
    write_registry(data_root, &registry)?;
    workspace_info(data_root, &registry.active_id, &record)
}

fn workspace_info(
    data_root: &Path,
    active_id: &str,
    record: &WorkspaceRecord,
) -> Result<WorkspaceInfo, String> {
    Ok(WorkspaceInfo {
        id: record.id.clone(),
        name: record.name.clone(),
        path: workspace_root(data_root, &record.id)?
            .to_string_lossy()
            .into_owned(),
        active: record.id == active_id,
        created_at: record.created_at,
    })
}

fn load_registry(data_root: &Path) -> Result<WorkspaceRegistry, String> {
    let path = registry_path(data_root);
    if !path.is_file() {
        return Ok(WorkspaceRegistry::default());
    }
    let source =
        fs::read_to_string(&path).map_err(|error| format!("读取工作区注册表失败：{error}"))?;
    match serde_json::from_str(&source) {
        Ok(registry) => Ok(registry),
        Err(error) => {
            let backup = path.with_extension(format!("corrupt-{}.json", now_millis()));
            fs::rename(&path, &backup).map_err(|move_error| {
                format!("工作区注册表无效（{error}），备份失败：{move_error}")
            })?;
            Ok(WorkspaceRegistry::default())
        }
    }
}

fn normalize_registry(registry: &mut WorkspaceRegistry) {
    registry.schema = REGISTRY_SCHEMA;
    registry
        .workspaces
        .retain(|workspace| validate_workspace_id(&workspace.id).is_ok());
    if !registry
        .workspaces
        .iter()
        .any(|workspace| workspace.id == PERSONAL_WORKSPACE_ID)
    {
        registry.workspaces.insert(
            0,
            WorkspaceRecord {
                id: PERSONAL_WORKSPACE_ID.to_owned(),
                name: PERSONAL_WORKSPACE_NAME.to_owned(),
                created_at: now_millis(),
            },
        );
    }
    if !registry
        .workspaces
        .iter()
        .any(|workspace| workspace.id == registry.active_id)
    {
        registry.active_id = PERSONAL_WORKSPACE_ID.to_owned();
    }
}

fn write_registry(data_root: &Path, registry: &WorkspaceRegistry) -> Result<(), String> {
    let path = registry_path(data_root);
    let parent = path
        .parent()
        .ok_or_else(|| "工作区注册表路径无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建工作区注册表目录失败：{error}"))?;
    let temporary = path.with_extension("json.tmp");
    let source = serde_json::to_vec_pretty(registry)
        .map_err(|error| format!("序列化工作区注册表失败：{error}"))?;
    fs::write(&temporary, source).map_err(|error| format!("写入工作区注册表失败：{error}"))?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("替换工作区注册表失败：{error}"))?;
    }
    fs::rename(temporary, path).map_err(|error| format!("提交工作区注册表失败：{error}"))
}

fn registry_path(data_root: &Path) -> PathBuf {
    data_root.join(".drpa").join("workspaces.json")
}

fn workspace_root(data_root: &Path, workspace_id: &str) -> Result<PathBuf, String> {
    validate_workspace_id(workspace_id)?;
    if workspace_id == PERSONAL_WORKSPACE_ID {
        Ok(data_root.to_path_buf())
    } else {
        Ok(data_root
            .join(".drpa")
            .join("workspaces")
            .join(workspace_id))
    }
}

fn validate_workspace_id(workspace_id: &str) -> Result<(), String> {
    if workspace_id == PERSONAL_WORKSPACE_ID
        || (workspace_id.starts_with("workspace-")
            && workspace_id.len() == "workspace-".len() + 32
            && workspace_id["workspace-".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err("工作区标识无效".to_owned())
    }
}

fn validate_workspace_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let length = name.chars().count();
    if !(1..=60).contains(&length) {
        return Err("工作区名称需要 1–60 个字符".to_owned());
    }
    if name.chars().any(char::is_control) {
        return Err("工作区名称不能包含控制字符".to_owned());
    }
    Ok(name.to_owned())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root() -> PathBuf {
        std::env::temp_dir().join(format!("drpa-workspace-test-{}", Uuid::new_v4()))
    }

    #[test]
    fn personal_workspace_preserves_the_legacy_data_root() {
        let root = temporary_root();
        fs::create_dir_all(root.join("projects/legacy-project")).unwrap();

        let active = resolve_active_workspace(&root).unwrap();

        assert_eq!(active, root);
        assert!(active.join("projects/legacy-project").is_dir());
        let _ = fs::remove_dir_all(active);
    }

    #[test]
    fn created_workspaces_have_separate_data_roots() {
        let root = temporary_root();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("personal-only.txt"), b"personal").unwrap();

        let created = create_workspace_inner(&root, "客户 A").unwrap();
        let created_root = PathBuf::from(created.path);

        assert_ne!(created_root, root);
        assert!(created_root.starts_with(root.join(".drpa/workspaces")));
        assert!(!created_root.join("personal-only.txt").exists());
        assert!(create_workspace_inner(&root, "客户 A").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
