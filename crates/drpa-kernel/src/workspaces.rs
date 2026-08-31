use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PERSONAL_WORKSPACE_ID: &str = "personal";
pub const PERSONAL_WORKSPACE_NAME: &str = "个人工作区";
const REGISTRY_SCHEMA: u32 = 1;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub active: bool,
    pub created_at: u64,
}

pub struct WorkspaceManager {
    data_root: PathBuf,
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

impl WorkspaceManager {
    pub fn new(data_root: impl Into<PathBuf>) -> Result<Self, String> {
        let manager = Self {
            data_root: data_root.into(),
        };
        fs::create_dir_all(&manager.data_root)
            .map_err(|error| format!("创建数据目录失败：{error}"))?;
        manager.normalize_and_save()?;
        Ok(manager)
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn active_root(&self) -> Result<PathBuf, String> {
        let mut registry = self.load()?;
        normalize_registry(&mut registry);
        let root = workspace_root(&self.data_root, &registry.active_id)?;
        fs::create_dir_all(&root).map_err(|error| format!("创建工作区目录失败：{error}"))?;
        self.write(&registry)?;
        Ok(root)
    }

    pub fn list(&self) -> Result<Vec<WorkspaceInfo>, String> {
        let mut registry = self.load()?;
        normalize_registry(&mut registry);
        self.write(&registry)?;
        registry
            .workspaces
            .iter()
            .map(|record| workspace_info(&self.data_root, &registry.active_id, record))
            .collect()
    }

    pub fn create(&self, name: &str) -> Result<WorkspaceInfo, String> {
        let name = validate_workspace_name(name)?;
        let mut registry = self.load()?;
        normalize_registry(&mut registry);
        if registry
            .workspaces
            .iter()
            .any(|item| item.name.eq_ignore_ascii_case(&name))
        {
            return Err("已存在同名工作区".to_owned());
        }
        let record = WorkspaceRecord {
            id: format!("workspace-{}", Uuid::new_v4().simple()),
            name,
            created_at: now_millis(),
        };
        let root = workspace_root(&self.data_root, &record.id)?;
        fs::create_dir_all(&root).map_err(|error| format!("创建工作区目录失败：{error}"))?;
        registry.workspaces.push(record.clone());
        self.write(&registry)?;
        workspace_info(&self.data_root, &registry.active_id, &record)
    }

    pub fn activate(&self, id: &str) -> Result<PathBuf, String> {
        validate_workspace_id(id)?;
        let mut registry = self.load()?;
        normalize_registry(&mut registry);
        if !registry.workspaces.iter().any(|item| item.id == id) {
            return Err(format!("工作区不存在：{id}"));
        }
        let root = workspace_root(&self.data_root, id)?;
        fs::create_dir_all(&root).map_err(|error| format!("创建工作区目录失败：{error}"))?;
        registry.active_id = id.to_owned();
        self.write(&registry)?;
        Ok(root)
    }

    fn normalize_and_save(&self) -> Result<(), String> {
        let mut registry = self.load()?;
        normalize_registry(&mut registry);
        self.write(&registry)
    }

    fn load(&self) -> Result<WorkspaceRegistry, String> {
        let path = self.registry_path();
        if !path.is_file() {
            return Ok(WorkspaceRegistry::default());
        }
        let source =
            fs::read_to_string(&path).map_err(|error| format!("读取工作区注册表失败：{error}"))?;
        serde_json::from_str(&source).map_err(|error| format!("工作区注册表无效：{error}"))
    }

    fn write(&self, registry: &WorkspaceRegistry) -> Result<(), String> {
        let path = self.registry_path();
        let parent = path
            .parent()
            .ok_or_else(|| "工作区注册表路径无效".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| format!("创建工作区注册表目录失败：{error}"))?;
        let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(registry).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("写入工作区注册表失败：{error}"))?;
        if path.exists() {
            fs::remove_file(&path).map_err(|error| format!("替换工作区注册表失败：{error}"))?;
        }
        fs::rename(temporary, path).map_err(|error| format!("提交工作区注册表失败：{error}"))
    }

    fn registry_path(&self) -> PathBuf {
        self.data_root.join(".drpa").join("workspaces.json")
    }
}

fn workspace_info(
    data_root: &Path,
    active_id: &str,
    record: &WorkspaceRecord,
) -> Result<WorkspaceInfo, String> {
    Ok(WorkspaceInfo {
        id: record.id.clone(),
        name: record.name.clone(),
        path: workspace_root(data_root, &record.id)?.display().to_string(),
        active: record.id == active_id,
        created_at: record.created_at,
    })
}

fn workspace_root(data_root: &Path, id: &str) -> Result<PathBuf, String> {
    validate_workspace_id(id)?;
    Ok(if id == PERSONAL_WORKSPACE_ID {
        data_root.to_path_buf()
    } else {
        data_root.join(".drpa").join("workspaces").join(id)
    })
}

fn normalize_registry(registry: &mut WorkspaceRegistry) {
    registry.schema = REGISTRY_SCHEMA;
    registry
        .workspaces
        .retain(|item| validate_workspace_id(&item.id).is_ok());
    if !registry
        .workspaces
        .iter()
        .any(|item| item.id == PERSONAL_WORKSPACE_ID)
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
        .any(|item| item.id == registry.active_id)
    {
        registry.active_id = PERSONAL_WORKSPACE_ID.to_owned();
    }
}

fn validate_workspace_id(id: &str) -> Result<(), String> {
    if id == PERSONAL_WORKSPACE_ID
        || (id.starts_with("workspace-")
            && id.len() == "workspace-".len() + 32
            && id["workspace-".len()..]
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
    if !(1..=60).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        Err("工作区名称需要 1–60 个非控制字符".to_owned())
    } else {
        Ok(name.to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn stays_compatible_with_desktop_workspace_layout() {
        let temporary = TempDir::new().unwrap();
        let manager = WorkspaceManager::new(temporary.path()).unwrap();
        assert_eq!(manager.active_root().unwrap(), temporary.path());
        let created = manager.create("CLI 工作区").unwrap();
        let root = manager.activate(&created.id).unwrap();
        assert!(root.starts_with(temporary.path().join(".drpa/workspaces")));
        assert_eq!(
            manager
                .list()
                .unwrap()
                .iter()
                .filter(|item| item.active)
                .count(),
            1
        );
    }
}
