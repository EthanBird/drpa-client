use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::AppPaths;

const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
const MAX_MEMORY_CONTEXT_BYTES: usize = 25 * 1024;
const INITIALIZED_MARKER: &str = ".workspace-v1";

const DEFAULT_AGENTS: &str = r#"# DRPA Agent 工作约定

## 目标

- 聚焦 RPAZ 脚本包的创建、读取、修改、校验、构建与知识维护。
- 先读取相关文件再修改；保持改动小而可验证。
- 修改项目后运行 `rpaz_validate`，需要交付归档时运行 `rpaz_build`。

## RPAZ 约定

- 包根目录必须包含 `manifest.yaml`，当前 schema 为 2。
- Python 入口使用 `def main(ctx)`。
- 参数从 `ctx.params` 读取，产物通过 `ctx.output_file()` 登记。
- 长任务使用 `ctx.progress()`；需要展示结果目录时使用 `ctx.open_output_directory()`。

## 上下文管理

- AGENTS.md 保存每次会话都应遵循的短规则。
- MEMORY.md 保存稳定事实、偏好和已验证经验，不保存密钥。
- Skills 保存可复用的多步骤流程；只在任务匹配时按需读取正文。
"#;

const DEFAULT_MEMORY: &str = r#"# Agent Memory

> 这里保存跨会话仍有价值的简短事实、偏好与已验证经验。详细流程请写入 Skill。
"#;

const DEFAULT_SKILL: &str = r#"---
name: rpaz-development
description: 创建、修改、校验或构建 RPAZ 脚本包时使用，覆盖 manifest、ctx API 和交付检查。
---

# RPAZ Development

1. 先列出并读取项目中的 `manifest.yaml`、入口模块和相关测试。
2. 保持 manifest schema 2，参数声明与 `ctx.params` 读取一致。
3. 输出文件统一通过 `ctx.output_file()` 创建；长任务报告进度。
4. 修改后调用 `rpaz_validate`，修复全部结构错误。
5. 需要归档时调用 `rpaz_build`，报告生成路径和文件数量。
"#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSkillSummary {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) modified_at: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentWorkspaceConfig {
    agents_markdown: String,
    memory_markdown: String,
    skills: Vec<AgentSkillSummary>,
    root_directory: String,
}

#[tauri::command]
pub(crate) fn get_agent_workspace_config(
    paths: State<'_, AppPaths>,
) -> Result<AgentWorkspaceConfig, String> {
    load_workspace_config(&paths.workspace_root)
}

#[tauri::command]
pub(crate) fn write_agent_workspace_document(
    document: String,
    content: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    ensure_agent_workspace(&paths.workspace_root)?;
    let target = match document.as_str() {
        "agents" => agent_root(&paths.workspace_root).join("AGENTS.md"),
        "memory" => agent_root(&paths.workspace_root).join("MEMORY.md"),
        _ => return Err("未知的 Agent 文档类型".to_owned()),
    };
    write_bounded_text(&target, &content)
}

#[tauri::command]
pub(crate) fn read_agent_skill(name: String, paths: State<'_, AppPaths>) -> Result<String, String> {
    read_skill_for_agent(&paths.workspace_root, &name)
}

#[tauri::command]
pub(crate) fn write_agent_skill(
    name: String,
    content: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    write_skill_for_agent(&paths.workspace_root, &name, &content)
}

#[tauri::command]
pub(crate) fn delete_agent_skill(name: String, paths: State<'_, AppPaths>) -> Result<(), String> {
    ensure_agent_workspace(&paths.workspace_root)?;
    validate_skill_name(&name)?;
    let target = skills_root(&paths.workspace_root).join(&name);
    if !target.exists() {
        return Err(format!("Skill 不存在：{name}"));
    }
    let metadata = fs::symlink_metadata(&target).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Skill 目录类型无效".to_owned());
    }
    fs::remove_dir_all(target).map_err(|error| format!("删除 Skill 失败：{error}"))
}

pub(crate) fn load_workspace_config(workspace_root: &Path) -> Result<AgentWorkspaceConfig, String> {
    ensure_agent_workspace(workspace_root)?;
    let root = agent_root(workspace_root);
    Ok(AgentWorkspaceConfig {
        agents_markdown: read_bounded_text(&root.join("AGENTS.md"), MAX_DOCUMENT_BYTES)?,
        memory_markdown: read_bounded_text(&root.join("MEMORY.md"), MAX_DOCUMENT_BYTES)?,
        skills: list_skills_for_agent(workspace_root)?,
        root_directory: root.to_string_lossy().into_owned(),
    })
}

pub(crate) fn render_agent_context(
    workspace_root: &Path,
    project_root: Option<&Path>,
) -> Result<String, String> {
    ensure_agent_workspace(workspace_root)?;
    let root = agent_root(workspace_root);
    let global = read_bounded_text(&root.join("AGENTS.md"), MAX_DOCUMENT_BYTES)?;
    let memory = read_bounded_text(&root.join("MEMORY.md"), MAX_MEMORY_CONTEXT_BYTES)?;
    let project = project_root
        .map(|path| path.join("AGENTS.md"))
        .filter(|path| path.is_file())
        .map(|path| read_bounded_text(&path, MAX_DOCUMENT_BYTES))
        .transpose()?
        .unwrap_or_default();
    let skills = list_skills_for_agent(workspace_root)?;
    let catalog = if skills.is_empty() {
        "- 暂无 Skill".to_owned()
    } else {
        skills
            .iter()
            .map(|skill| format!("- {}：{}", skill.name, skill.description))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(format!(
        "\n\n<workspace_agents>\n{global}\n</workspace_agents>\n\
         <project_agents>\n{project}\n</project_agents>\n\
         <agent_memory>\n{memory}\n</agent_memory>\n\
         <available_skills>\n{catalog}\n</available_skills>\n\
         Skill 正文采用渐进加载：仅在任务匹配时调用 agent_read_skill；不要把全部 Skill 一次性读入上下文。"
    ))
}

pub(crate) fn list_skills_for_agent(
    workspace_root: &Path,
) -> Result<Vec<AgentSkillSummary>, String> {
    ensure_agent_workspace(workspace_root)?;
    let root = skills_root(workspace_root);
    let mut skills = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|error| error.to_string())?
        .flatten()
    {
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if validate_skill_name(&name).is_err() {
            continue;
        }
        let path = entry.path().join("SKILL.md");
        if !path.is_file() {
            continue;
        }
        let content = read_bounded_text(&path, MAX_DOCUMENT_BYTES)?;
        let description = skill_metadata_value(&content, "description")
            .unwrap_or_else(|| "未填写描述".to_owned());
        let modified_at = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        skills.push(AgentSkillSummary {
            name,
            description,
            modified_at,
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

pub(crate) fn read_skill_for_agent(workspace_root: &Path, name: &str) -> Result<String, String> {
    ensure_agent_workspace(workspace_root)?;
    validate_skill_name(name)?;
    let path = skills_root(workspace_root).join(name).join("SKILL.md");
    if !path.is_file() {
        return Err(format!("Skill 不存在：{name}"));
    }
    read_bounded_text(&path, MAX_DOCUMENT_BYTES)
}

pub(crate) fn write_skill_for_agent(
    workspace_root: &Path,
    name: &str,
    content: &str,
) -> Result<(), String> {
    ensure_agent_workspace(workspace_root)?;
    validate_skill_name(name)?;
    validate_skill_document(name, content)?;
    let root = skills_root(workspace_root).join(name);
    if root.exists() {
        let metadata = fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("Skill 目录类型无效".to_owned());
        }
    }
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    write_bounded_text(&root.join("SKILL.md"), content)
}

pub(crate) fn read_memory_for_agent(workspace_root: &Path) -> Result<String, String> {
    ensure_agent_workspace(workspace_root)?;
    read_bounded_text(
        &agent_root(workspace_root).join("MEMORY.md"),
        MAX_DOCUMENT_BYTES,
    )
}

pub(crate) fn write_memory_for_agent(workspace_root: &Path, content: &str) -> Result<(), String> {
    ensure_agent_workspace(workspace_root)?;
    write_bounded_text(&agent_root(workspace_root).join("MEMORY.md"), content)
}

fn ensure_agent_workspace(workspace_root: &Path) -> Result<(), String> {
    let root = agent_root(workspace_root);
    let skills = skills_root(workspace_root);
    fs::create_dir_all(&skills).map_err(|error| error.to_string())?;
    let marker = root.join(INITIALIZED_MARKER);
    if !marker.is_file() {
        write_if_missing(&root.join("AGENTS.md"), DEFAULT_AGENTS)?;
        write_if_missing(&root.join("MEMORY.md"), DEFAULT_MEMORY)?;
        let default_skill = skills.join("rpaz-development");
        fs::create_dir_all(&default_skill).map_err(|error| error.to_string())?;
        write_if_missing(&default_skill.join("SKILL.md"), DEFAULT_SKILL)?;
        fs::write(marker, b"1\n").map_err(|error| error.to_string())?;
    } else {
        write_if_missing(&root.join("AGENTS.md"), DEFAULT_AGENTS)?;
        write_if_missing(&root.join("MEMORY.md"), DEFAULT_MEMORY)?;
    }
    Ok(())
}

fn write_if_missing(path: &Path, content: &str) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    write_bounded_text(path, content)
}

fn write_bounded_text(path: &Path, content: &str) -> Result<(), String> {
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("Agent 文档超过 {} KiB", MAX_DOCUMENT_BYTES / 1024));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
    fs::write(&temporary, content.as_bytes()).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            fs::remove_file(path).map_err(|remove_error| remove_error.to_string())?;
            fs::rename(&temporary, path).map_err(|rename_error| rename_error.to_string())?;
        } else {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
    }
    Ok(())
}

fn read_bounded_text(path: &Path, limit: usize) -> Result<String, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    if metadata.len() > limit as u64 {
        return Err(format!("Agent 文档超过 {} KiB", limit / 1024));
    }
    fs::read_to_string(path).map_err(|error| format!("读取 {} 失败：{error}", path.display()))
}

fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || name.starts_with('-')
        || name.ends_with('-')
    {
        return Err("Skill 名称仅允许 1-64 位小写字母、数字和中划线".to_owned());
    }
    Ok(())
}

fn validate_skill_document(name: &str, content: &str) -> Result<(), String> {
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("Skill 超过 {} KiB", MAX_DOCUMENT_BYTES / 1024));
    }
    let declared = skill_metadata_value(content, "name")
        .ok_or_else(|| "SKILL.md 需要 YAML frontmatter 和 name 字段".to_owned())?;
    if declared != name {
        return Err(format!("SKILL.md 的 name 必须与目录名一致：{name}"));
    }
    if skill_metadata_value(content, "description").is_none() {
        return Err("SKILL.md 需要 description 字段".to_owned());
    }
    Ok(())
}

fn skill_metadata_value(content: &str, key: &str) -> Option<String> {
    let normalized = content.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    let (frontmatter, _) = rest.split_once("\n---\n")?;
    frontmatter.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        if candidate.trim() != key {
            return None;
        }
        let value = value.trim().trim_matches(['\'', '"']);
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn agent_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("agent")
}

fn skills_root(workspace_root: &Path) -> PathBuf {
    agent_root(workspace_root).join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_and_manages_agent_skills() {
        let workspace = std::env::temp_dir().join(format!("drpa-agent-config-{}", Uuid::new_v4()));
        let config = load_workspace_config(&workspace).unwrap();
        assert!(config.agents_markdown.contains("RPAZ"));
        assert_eq!(config.skills[0].name, "rpaz-development");

        let content = "---\nname: test-skill\ndescription: 测试流程\n---\n\n# Test\n";
        write_skill_for_agent(&workspace, "test-skill", content).unwrap();
        assert_eq!(
            read_skill_for_agent(&workspace, "test-skill").unwrap(),
            content
        );
        assert!(write_skill_for_agent(&workspace, "../bad", content).is_err());
        let project = workspace.join("projects/test");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "# 项目规则\n").unwrap();
        write_memory_for_agent(&workspace, "# Memory\n\n使用简体中文。\n").unwrap();
        let injected = render_agent_context(&workspace, Some(&project)).unwrap();
        assert!(injected.contains("# 项目规则"));
        assert!(injected.contains("使用简体中文"));
        assert!(injected.contains("test-skill：测试流程"));
        let _ = fs::remove_dir_all(workspace);
    }
}
