use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError},
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::State;
use uuid::Uuid;

use crate::{AppPaths, agent_runtime::AgentRunControl};

const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
const MAX_MEMORY_CONTEXT_BYTES: usize = 25 * 1024;
const MAX_SKILL_OUTPUT_BYTES: usize = 256 * 1024;
const SKILL_IO_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const INITIALIZED_MARKER: &str = ".workspace-v2";
const LEGACY_MARKER: &str = ".workspace-v1";
static MEMORY_WRITE_LOCK: Mutex<()> = Mutex::new(());

const DEFAULT_AGENTS: &str = r#"# DRPA Agent 工作约定

## 目标

- 聚焦 RPAZ 包的创建、读取、修改、校验、构建与知识维护。
- 先读取相关文件再修改；保持改动小而可验证。
- 修改项目后运行 `rpaz_validate`，需要交付归档时运行 `rpaz_build`。

## Skills 2.0

- Skill 是由 `skill.yaml`、`instructions.md`、工作流、资源、代码和测试组成的能力包。
- Skill 可以声明 Python 或命令工具，也可以携带供工具调用的代码库。
- 只在任务匹配时加载 Skill 正文；工具由 DRPA ToolRegistry 统一发现和执行。
- `permissions` 是能力声明和调度元数据，不等于操作系统沙箱；只启用来源可信的 Skill。
"#;

const DEFAULT_MEMORY: &str = r#"# Agent Memory

> 保存跨会话仍有价值的简短事实、偏好与已验证经验。详细流程写入 Skill。
"#;

const DEFAULT_SKILL_MANIFEST: &str = r#"schema: 2
id: rpaz-development
name: RPAZ Development
version: 2.0.0
description: 创建、修改、校验或构建 RPAZ 包时使用。
activation:
  intents:
    - 创建 RPAZ 包
    - 修改 RPAZ
    - 调试脚本
  file_patterns:
    - manifest.yaml
    - '*.rpaz'
permissions:
  workspace_read: true
  workspace_write: true
  network: false
tools: []
libraries: []
"#;

const DEFAULT_SKILL_INSTRUCTIONS: &str = r#"# RPAZ Development

1. 先列出并读取项目中的 `manifest.yaml`、入口模块和相关测试。
2. 保持 manifest schema 2，参数声明与 `ctx.params` 读取一致。
3. 输出文件统一通过 `ctx.output_file()` 创建；长任务报告进度。
4. 修改后调用 `rpaz_validate`，修复全部结构错误。
5. 需要归档时调用 `rpaz_build`，报告生成路径和文件数量。
"#;

const DATA_ANALYSIS_SKILL_MANIFEST: &str = r#"schema: 2
id: data-analysis
name: 数据分析
version: 2.0.0
description: 使用数据工作台只读查询、Python 与文档能力完成可复核的数据分析。
activation:
  intents:
    - 数据分析
    - 指标诊断
    - 数据质量
    - 报告生成
  file_patterns:
    - '*.csv'
    - '*.xls'
    - '*.xlsx'
    - '*.sqlite'
    - '*.db'
permissions:
  workspace_read: true
  workspace_write: false
  network: false
tools: []
libraries: []
"#;

const DATA_ANALYSIS_SKILL_INSTRUCTIONS: &str = r#"# 数据分析

1. 先确认问题、统计口径、数据粒度和时间范围，再读取连接与表结构。
2. 数据库只使用 `data_query` 执行只读 SQL；禁止生成或尝试执行写入、DDL 与破坏性语句。
3. 需要清洗、计算、制图或验证时，在已绑定的通用项目中使用 Python，并保留可复核的代码与中间口径。
4. 主动检查缺失值、重复值、异常范围、分母和样本量；不要把相关性写成因果性。
5. 结论先行，随后给证据、口径、限制与可执行建议；生成报告或表格时使用文档工具。
"#;

const PYTHON_SKILL_RUNNER: &str = r#"
import importlib.util
import inspect
import json
import pathlib
import sys

entry_path = pathlib.Path(sys.argv[1]).resolve()
callable_name = sys.argv[2]
libraries = json.loads(sys.argv[3])
for library in reversed(libraries):
    sys.path.insert(0, str(pathlib.Path(library).resolve()))
spec = importlib.util.spec_from_file_location("drpa_skill_entry", entry_path)
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSkillSummary {
    pub(crate) name: String,
    pub(crate) display_name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    pub(crate) format: String,
    pub(crate) tool_count: usize,
    pub(crate) library_count: usize,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSkillPackage {
    pub(crate) name: String,
    pub(crate) manifest_yaml: String,
    pub(crate) instructions_markdown: String,
    pub(crate) files: Vec<String>,
    pub(crate) entries: Vec<AgentSkillEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSkillEntry {
    pub(crate) path: String,
    pub(crate) kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct SkillManifest {
    pub(crate) schema: u32,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) activation: SkillActivation,
    #[serde(default)]
    pub(crate) permissions: SkillPermissions,
    #[serde(default)]
    pub(crate) tools: Vec<SkillToolManifest>,
    #[serde(default)]
    pub(crate) libraries: Vec<SkillLibraryManifest>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct SkillActivation {
    #[serde(default)]
    pub(crate) intents: Vec<String>,
    #[serde(default)]
    pub(crate) file_patterns: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct SkillPermissions {
    #[serde(default)]
    pub(crate) workspace_read: bool,
    #[serde(default)]
    pub(crate) workspace_write: bool,
    #[serde(default)]
    pub(crate) network: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct SkillToolManifest {
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
pub(crate) struct SkillLibraryManifest {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) language: String,
}

pub(crate) struct SkillToolExecution {
    pub(crate) output: Value,
    pub(crate) summary: String,
}

fn default_object_schema() -> Value {
    json!({"type":"object","properties":{},"additionalProperties":false})
}

const fn default_tool_timeout() -> u64 {
    30
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
pub(crate) fn read_agent_skill_package(
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<AgentSkillPackage, String> {
    read_skill_package(&paths.workspace_root, &name)
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
pub(crate) fn write_agent_skill_package(
    name: String,
    manifest_yaml: String,
    instructions_markdown: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    write_skill_package(
        &paths.workspace_root,
        &name,
        &manifest_yaml,
        &instructions_markdown,
    )
}

#[tauri::command]
pub(crate) fn read_agent_skill_file(
    name: String,
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    let path = resolve_skill_file(&paths.workspace_root, &name, &relative_path, true)?;
    read_bounded_text(&path, MAX_DOCUMENT_BYTES)
}

#[tauri::command]
pub(crate) fn write_agent_skill_file(
    name: String,
    relative_path: String,
    content: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let normalized_path = relative_path.replace('\\', "/");
    if normalized_path == "skill.yaml" {
        parse_skill_manifest(&name, &content)?;
    }
    if normalized_path == "instructions.md" && content.len() > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "Skill instructions.md 超过 {} KiB",
            MAX_DOCUMENT_BYTES / 1024
        ));
    }
    let path = resolve_skill_file(&paths.workspace_root, &name, &relative_path, false)?;
    write_bounded_text(&path, &content)
}

#[tauri::command]
pub(crate) fn create_agent_skill_directory(
    name: String,
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let path = resolve_skill_file(&paths.workspace_root, &name, &relative_path, false)?;
    if path.exists() {
        return Err(format!("Skill 路径已存在：{relative_path}"));
    }
    fs::create_dir_all(path).map_err(|error| format!("创建 Skill 目录失败：{error}"))
}

#[tauri::command]
pub(crate) fn rename_agent_skill_path(
    name: String,
    relative_path: String,
    new_relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    reject_protected_skill_path(&relative_path)?;
    reject_protected_skill_path(&new_relative_path)?;
    let source = resolve_skill_file(&paths.workspace_root, &name, &relative_path, true)?;
    let target = resolve_skill_file(&paths.workspace_root, &name, &new_relative_path, false)?;
    if target.exists() {
        return Err(format!("Skill 路径已存在：{new_relative_path}"));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::rename(source, target).map_err(|error| format!("重命名 Skill 路径失败：{error}"))
}

#[tauri::command]
pub(crate) fn delete_agent_skill_path(
    name: String,
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    reject_protected_skill_path(&relative_path)?;
    let target = resolve_skill_file(&paths.workspace_root, &name, &relative_path, true)?;
    let metadata = fs::symlink_metadata(&target).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Skill 路径类型无效".to_owned());
    }
    if metadata.is_dir() {
        fs::remove_dir_all(target).map_err(|error| format!("删除 Skill 目录失败：{error}"))
    } else {
        fs::remove_file(target).map_err(|error| format!("删除 Skill 文件失败：{error}"))
    }
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
    selected_skill_ids: &[String],
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
    let selected = selected_skill_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if let Some(missing) = selected
        .iter()
        .find(|name| !skills.iter().any(|skill| skill.name == **name))
    {
        return Err(format!("所选 Skill 不存在：{missing}"));
    }
    let catalog = if skills.is_empty() {
        "- 暂无 Skill".to_owned()
    } else {
        skills
            .iter()
            .filter(|skill| selected.is_empty() || selected.contains(skill.name.as_str()))
            .map(|skill| {
                format!(
                    "- {}：{}（v{}，{} 个工具，{} 个代码库）",
                    skill.name,
                    skill.description,
                    skill.version,
                    skill.tool_count,
                    skill.library_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let selected_instructions = if selected.is_empty() {
        "未固定选择 Skill；按任务从目录渐进加载。".to_owned()
    } else {
        selected_skill_ids
            .iter()
            .map(|name| {
                read_skill_for_agent(workspace_root, name).map(|content| {
                    format!("<selected_skill name=\"{name}\">\n{content}\n</selected_skill>")
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("\n\n")
    };
    Ok(format!(
        "\n\n<workspace_agents>\n{global}\n</workspace_agents>\n\
         <project_agents>\n{project}\n</project_agents>\n\
         <agent_memory>\n{memory}\n</agent_memory>\n\
         <available_skills>\n{catalog}\n</available_skills>\n\
         <selected_skills>\n{selected_instructions}\n</selected_skills>\n\
         未固定选择 Skill 时采用渐进加载：任务匹配时调用 agent_read_skill。Skill 可能包含由 ToolRegistry 暴露的可执行工具。\
         Skill 清单中的 permissions 只是能力声明和调度元数据，不代表操作系统级沙箱；只执行来源可信且经用户启用的 Skill。"
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
        let manifest_path = entry.path().join("skill.yaml");
        if manifest_path.is_file() {
            let manifest_source = read_bounded_text(&manifest_path, MAX_DOCUMENT_BYTES)?;
            let manifest = parse_skill_manifest(&name, &manifest_source)?;
            skills.push(AgentSkillSummary {
                name,
                display_name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                format: "skill-v2".to_owned(),
                tool_count: manifest.tools.len(),
                library_count: manifest.libraries.len(),
                modified_at: modified_millis(&manifest_path),
            });
            continue;
        }
        let legacy_path = entry.path().join("SKILL.md");
        if legacy_path.is_file() {
            let content = read_bounded_text(&legacy_path, MAX_DOCUMENT_BYTES)?;
            skills.push(AgentSkillSummary {
                name: name.clone(),
                display_name: name,
                version: "1.0.0".to_owned(),
                description: skill_metadata_value(&content, "description")
                    .unwrap_or_else(|| "未填写描述".to_owned()),
                format: "legacy-markdown".to_owned(),
                tool_count: 0,
                library_count: 0,
                modified_at: modified_millis(&legacy_path),
            });
        }
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

pub(crate) fn read_skill_for_agent(workspace_root: &Path, name: &str) -> Result<String, String> {
    let package = read_skill_package(workspace_root, name)?;
    Ok(format!(
        "<skill_manifest>\n{}\n</skill_manifest>\n\n<skill_instructions>\n{}\n</skill_instructions>",
        package.manifest_yaml, package.instructions_markdown
    ))
}

pub(crate) fn read_skill_package(
    workspace_root: &Path,
    name: &str,
) -> Result<AgentSkillPackage, String> {
    ensure_agent_workspace(workspace_root)?;
    validate_skill_name(name)?;
    let root = skills_root(workspace_root).join(name);
    if !root.is_dir() {
        return Err(format!("Skill 不存在：{name}"));
    }
    let manifest_path = root.join("skill.yaml");
    let instructions_path = root.join("instructions.md");
    let (manifest_yaml, instructions_markdown) = if manifest_path.is_file() {
        (
            read_bounded_text(&manifest_path, MAX_DOCUMENT_BYTES)?,
            read_bounded_text(&instructions_path, MAX_DOCUMENT_BYTES)?,
        )
    } else {
        let legacy = read_bounded_text(&root.join("SKILL.md"), MAX_DOCUMENT_BYTES)?;
        (legacy_manifest(name, &legacy), legacy)
    };
    let mut entries = Vec::new();
    collect_skill_entries(&root, &root, &mut entries)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let files = entries
        .iter()
        .filter(|entry| entry.kind == "file")
        .map(|entry| entry.path.clone())
        .collect();
    Ok(AgentSkillPackage {
        name: name.to_owned(),
        manifest_yaml,
        instructions_markdown,
        files,
        entries,
    })
}

pub(crate) fn write_skill_for_agent(
    workspace_root: &Path,
    name: &str,
    content: &str,
) -> Result<(), String> {
    validate_legacy_skill_document(name, content)?;
    let manifest = legacy_manifest(name, content);
    write_skill_package(workspace_root, name, &manifest, content)
}

pub(crate) fn write_skill_package(
    workspace_root: &Path,
    name: &str,
    manifest_yaml: &str,
    instructions_markdown: &str,
) -> Result<(), String> {
    ensure_agent_workspace(workspace_root)?;
    validate_skill_name(name)?;
    parse_skill_manifest(name, manifest_yaml)?;
    if instructions_markdown.len() > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "Skill instructions.md 超过 {} KiB",
            MAX_DOCUMENT_BYTES / 1024
        ));
    }
    let root = skills_root(workspace_root).join(name);
    if root.exists() {
        let metadata = fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("Skill 目录类型无效".to_owned());
        }
    }
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    write_bounded_text(&root.join("skill.yaml"), manifest_yaml)?;
    write_bounded_text(&root.join("instructions.md"), instructions_markdown)?;
    Ok(())
}

pub(crate) fn skill_tool_definitions(
    workspace_root: &Path,
    selected_skill_ids: &[String],
) -> Result<Vec<Value>, String> {
    ensure_agent_workspace(workspace_root)?;
    let selected = selected_skill_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut definitions = Vec::new();
    for summary in list_skills_for_agent(workspace_root)? {
        if !selected.is_empty() && !selected.contains(summary.name.as_str()) {
            continue;
        }
        if summary.format != "skill-v2" || summary.tool_count == 0 {
            continue;
        }
        let source = read_bounded_text(
            &skills_root(workspace_root)
                .join(&summary.name)
                .join("skill.yaml"),
            MAX_DOCUMENT_BYTES,
        )?;
        let manifest = parse_skill_manifest(&summary.name, &source)?;
        for tool in manifest.tools {
            definitions.push(json!({
                "type": "function",
                "function": {
                    "name": qualified_skill_tool_name(&summary.name, &tool.name),
                    "description": format!("Skill {}：{}", summary.name, tool.description),
                    "parameters": tool.parameters,
                }
            }));
        }
    }
    Ok(definitions)
}

pub(crate) fn execute_skill_tool(
    workspace_root: &Path,
    project_root: Option<&Path>,
    python: &Path,
    qualified_name: &str,
    arguments: &Value,
    control: &AgentRunControl,
) -> Option<Result<SkillToolExecution, String>> {
    let remainder = qualified_name.strip_prefix("skill_")?;
    let (skill_name, tool_name) = remainder.split_once("__")?;
    Some(execute_skill_tool_inner(
        workspace_root,
        project_root,
        python,
        skill_name,
        tool_name,
        arguments,
        control,
    ))
}

fn execute_skill_tool_inner(
    workspace_root: &Path,
    project_root: Option<&Path>,
    python: &Path,
    skill_name: &str,
    tool_name: &str,
    arguments: &Value,
    control: &AgentRunControl,
) -> Result<SkillToolExecution, String> {
    validate_skill_name(skill_name)?;
    let root = skills_root(workspace_root).join(skill_name);
    let source = read_bounded_text(&root.join("skill.yaml"), MAX_DOCUMENT_BYTES)?;
    let manifest = parse_skill_manifest(skill_name, &source)?;
    let tool = manifest
        .tools
        .iter()
        .find(|candidate| candidate.name == tool_name)
        .ok_or_else(|| {
            format!(
                "Skill 工具不存在：{qualified_name}",
                qualified_name = qualified_skill_tool_name(skill_name, tool_name)
            )
        })?;
    let entry_spec = tool.entry.split_once(':');
    let entry_relative = entry_spec.map_or(tool.entry.as_str(), |(path, _)| path);
    let entry = resolve_child_file(&root, entry_relative, true)?;
    let libraries = manifest
        .libraries
        .iter()
        .map(|library| {
            let path = resolve_child_file(&root, &library.path, true)?;
            if path.is_file() {
                path.parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| format!("Skill 代码库路径无效：{}", library.path))
            } else {
                Ok(path)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let request = json!({
        "arguments": arguments,
        "context": {
            "skillRoot": root.to_string_lossy(),
            "workspaceRoot": workspace_root.to_string_lossy(),
            "projectRoot": project_root.map(|path| path.to_string_lossy().into_owned()),
            "permissions": manifest.permissions,
            "permissionEnforcement": "declaration-only; not an operating-system sandbox",
        }
    });
    let timeout = Duration::from_secs(tool.timeout_seconds.clamp(1, 300));
    let output = match tool.runtime.as_str() {
        "python" => {
            if !python.is_file() {
                return Err("Skill Python 工具需要已初始化的封装运行环境".to_owned());
            }
            let callable = entry_spec.map_or("run", |(_, callable)| callable);
            let libraries = serde_json::to_string(&libraries).map_err(|error| error.to_string())?;
            run_json_process(
                Command::new(python),
                &[
                    "-I".to_owned(),
                    "-c".to_owned(),
                    PYTHON_SKILL_RUNNER.to_owned(),
                    entry.to_string_lossy().into_owned(),
                    callable.to_owned(),
                    libraries,
                ],
                &root,
                &request,
                timeout,
                Some(control),
            )?
        }
        "command" => run_json_process(
            Command::new(&entry),
            &tool.args,
            &root,
            &request,
            timeout,
            Some(control),
        )?,
        runtime => return Err(format!("Skill 工具 runtime 无效：{runtime}")),
    };
    Ok(SkillToolExecution {
        output,
        summary: format!("Skill {skill_name} 已执行工具 {tool_name}"),
    })
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

pub(crate) fn append_memory_entry(
    workspace_root: &Path,
    category: &str,
    key: &str,
    value: &str,
    source: &str,
) -> Result<Value, String> {
    ensure_agent_workspace(workspace_root)?;
    if !matches!(category, "preference" | "fact" | "project" | "workflow") {
        return Err("记忆 category 无效".to_owned());
    }
    let key = key.trim();
    let value = value.trim();
    let source = source.trim();
    if key.is_empty() || key.chars().count() > 120 || key.chars().any(char::is_control) {
        return Err("记忆 key 必须为 1–120 个普通字符".to_owned());
    }
    if value.is_empty() || value.chars().count() > 4_000 {
        return Err("记忆 value 必须为 1–4000 个字符".to_owned());
    }
    if source.chars().count() > 500 {
        return Err("记忆 source 不能超过 500 个字符".to_owned());
    }
    let _guard = MEMORY_WRITE_LOCK
        .lock()
        .map_err(|_| "Agent 记忆写入状态已损坏".to_owned())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let entry = json!({
        "id": format!("memory-{}", Uuid::new_v4().simple()),
        "category": category,
        "key": key,
        "value": value,
        "source": source,
        "createdAt": u64::try_from(now).unwrap_or(u64::MAX),
    });
    let journal_root = agent_root(workspace_root).join("memory");
    fs::create_dir_all(&journal_root)
        .map_err(|error| format!("创建 Agent 记忆账本目录失败：{error}"))?;
    let journal_path = journal_root.join("events.jsonl");
    if fs::metadata(&journal_path).map_or(0, |metadata| metadata.len()) > 8 * 1024 * 1024 {
        return Err("Agent 记忆事件账本已达到 8 MiB 上限，请先整理 MEMORY.md".to_owned());
    }
    let mut bytes = serde_json::to_vec(&entry).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut journal = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&journal_path)
        .map_err(|error| format!("打开 Agent 记忆账本失败：{error}"))?;
    journal
        .write_all(&bytes)
        .and_then(|_| journal.flush())
        .map_err(|error| format!("追加 Agent 记忆事件失败：{error}"))?;

    let memory_path = agent_root(workspace_root).join("MEMORY.md");
    let mut memory = read_bounded_text(&memory_path, MAX_DOCUMENT_BYTES)?;
    if !memory.contains("## Structured memory journal") {
        memory.push_str("\n\n## Structured memory journal\n");
    }
    let safe_value = value.replace(['\r', '\n'], " ");
    let safe_source = source.replace(['\r', '\n'], " ");
    memory.push_str(&format!(
        "\n- **{category}/{key}**: {safe_value}{}",
        if safe_source.is_empty() {
            String::new()
        } else {
            format!(" _(source: {safe_source})_")
        }
    ));
    write_bounded_text(&memory_path, &memory)?;
    Ok(entry)
}

fn ensure_agent_workspace(workspace_root: &Path) -> Result<(), String> {
    let root = agent_root(workspace_root);
    let skills = skills_root(workspace_root);
    fs::create_dir_all(&skills).map_err(|error| error.to_string())?;
    write_if_missing(&root.join("AGENTS.md"), DEFAULT_AGENTS)?;
    write_if_missing(&root.join("MEMORY.md"), DEFAULT_MEMORY)?;
    migrate_legacy_skills(workspace_root)?;
    let default_skill = skills.join("rpaz-development");
    fs::create_dir_all(&default_skill).map_err(|error| error.to_string())?;
    write_if_missing(&default_skill.join("skill.yaml"), DEFAULT_SKILL_MANIFEST)?;
    write_if_missing(
        &default_skill.join("instructions.md"),
        DEFAULT_SKILL_INSTRUCTIONS,
    )?;
    let data_analysis_skill = skills.join("data-analysis");
    fs::create_dir_all(&data_analysis_skill).map_err(|error| error.to_string())?;
    write_if_missing(
        &data_analysis_skill.join("skill.yaml"),
        DATA_ANALYSIS_SKILL_MANIFEST,
    )?;
    write_if_missing(
        &data_analysis_skill.join("instructions.md"),
        DATA_ANALYSIS_SKILL_INSTRUCTIONS,
    )?;
    write_if_missing(&root.join(INITIALIZED_MARKER), "2\n")?;
    Ok(())
}

fn migrate_legacy_skills(workspace_root: &Path) -> Result<(), String> {
    let root = agent_root(workspace_root);
    if !root.join(LEGACY_MARKER).is_file() && !skills_root(workspace_root).is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(skills_root(workspace_root))
        .map_err(|error| error.to_string())?
        .flatten()
    {
        if !entry.path().is_dir() || entry.path().join("skill.yaml").is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let legacy_path = entry.path().join("SKILL.md");
        if validate_skill_name(&name).is_err() || !legacy_path.is_file() {
            continue;
        }
        let legacy = read_bounded_text(&legacy_path, MAX_DOCUMENT_BYTES)?;
        write_bounded_text(
            &entry.path().join("skill.yaml"),
            &legacy_manifest(&name, &legacy),
        )?;
        write_bounded_text(&entry.path().join("instructions.md"), &legacy)?;
    }
    Ok(())
}

fn legacy_manifest(name: &str, content: &str) -> String {
    let description = skill_metadata_value(content, "description")
        .unwrap_or_else(|| "从旧版 SKILL.md 迁移的能力包".to_owned());
    let display_name = skill_metadata_value(content, "name").unwrap_or_else(|| name.to_owned());
    serde_yaml::to_string(&SkillManifest {
        schema: 2,
        id: name.to_owned(),
        name: display_name,
        version: "2.0.0".to_owned(),
        description,
        activation: SkillActivation::default(),
        permissions: SkillPermissions::default(),
        tools: Vec::new(),
        libraries: Vec::new(),
    })
    .unwrap_or_else(|_| format!("schema: 2\nid: {name}\nname: {name}\nversion: 2.0.0\ndescription: Migrated Skill\ntools: []\nlibraries: []\n"))
}

fn parse_skill_manifest(name: &str, source: &str) -> Result<SkillManifest, String> {
    if source.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("skill.yaml 超过 {} KiB", MAX_DOCUMENT_BYTES / 1024));
    }
    let manifest: SkillManifest =
        serde_yaml::from_str(source).map_err(|error| format!("skill.yaml 无效：{error}"))?;
    if manifest.schema != 2 {
        return Err("skill.yaml schema 必须为 2".to_owned());
    }
    validate_skill_name(&manifest.id)?;
    if manifest.id != name {
        return Err(format!("skill.yaml 的 id 必须与目录名一致：{name}"));
    }
    if manifest.name.trim().is_empty()
        || manifest.description.trim().is_empty()
        || manifest.version.trim().is_empty()
    {
        return Err("skill.yaml 需要 name、version 和 description".to_owned());
    }
    let mut tool_names = HashSet::new();
    for tool in &manifest.tools {
        validate_tool_name(&tool.name)?;
        if !tool_names.insert(tool.name.as_str()) {
            return Err(format!("Skill 工具名称重复：{}", tool.name));
        }
        if qualified_skill_tool_name(name, &tool.name).len() > 64 {
            return Err(format!(
                "Skill 工具 {} 的完整注册名称超过 64 个字符",
                tool.name
            ));
        }
        if tool.description.trim().is_empty() || tool.entry.trim().is_empty() {
            return Err(format!(
                "Skill 工具 {} 需要 description 和 entry",
                tool.name
            ));
        }
        validate_relative_package_path(
            tool.entry
                .split_once(':')
                .map_or(tool.entry.as_str(), |(path, _)| path),
            "Skill 工具 entry",
        )?;
        if !matches!(tool.runtime.as_str(), "python" | "command") {
            return Err(format!(
                "Skill 工具 {} 的 runtime 只支持 python 或 command",
                tool.name
            ));
        }
        if tool.timeout_seconds == 0 || tool.timeout_seconds > 300 {
            return Err(format!(
                "Skill 工具 {} 的 timeout_seconds 必须在 1-300 之间",
                tool.name
            ));
        }
        if !tool.parameters.is_object() {
            return Err(format!(
                "Skill 工具 {} 的 parameters 必须是 JSON Schema 对象",
                tool.name
            ));
        }
    }
    for library in &manifest.libraries {
        validate_relative_package_path(&library.path, "Skill 代码库 path")?;
    }
    Ok(manifest)
}

fn validate_relative_package_path(value: &str, label: &str) -> Result<(), String> {
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

fn qualified_skill_tool_name(skill: &str, tool: &str) -> String {
    format!("skill_{skill}__{tool}")
}

fn validate_tool_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 48
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || name.starts_with('_')
        || name.ends_with('_')
    {
        return Err("工具名称只允许 1-48 位小写字母、数字和下划线".to_owned());
    }
    Ok(())
}

fn run_json_process(
    mut command: Command,
    args: &[String],
    current_dir: &Path,
    request: &Value,
    timeout: Duration,
    control: Option<&AgentRunControl>,
) -> Result<Value, String> {
    command
        .args(args)
        .current_dir(current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    configure_skill_process(&mut command);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 Skill 工具失败：{error}"))?;

    let mut process_tree = SkillProcessTree::attach(&mut child);
    let Some(stdout) = child.stdout.take() else {
        process_tree.terminate(&mut child);
        return Err("无法捕获 Skill 工具标准输出".to_owned());
    };
    let Some(stderr) = child.stderr.take() else {
        process_tree.terminate(&mut child);
        return Err("无法捕获 Skill 工具错误输出".to_owned());
    };
    let Some(stdin) = child.stdin.take() else {
        process_tree.terminate(&mut child);
        return Err("无法写入 Skill 工具标准输入".to_owned());
    };
    let output_bytes = Arc::new(AtomicUsize::new(0));
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let stdout_receiver =
        capture_skill_stream(stdout, output_bytes.clone(), output_exceeded.clone());
    let stderr_receiver = capture_skill_stream(stderr, output_bytes, output_exceeded.clone());
    let stdin_receiver = write_skill_stdin(stdin, request.to_string().into_bytes());

    let mut status = None;
    let mut failure = None;
    loop {
        if let Some(control) = control
            && let Err(error) = control.check()
        {
            failure = Some(error);
            break;
        }
        if output_exceeded.load(Ordering::Relaxed) {
            failure = Some(format!(
                "Skill 工具输出超过 {} KiB 安全上限",
                MAX_SKILL_OUTPUT_BYTES / 1024
            ));
            break;
        }
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                status = Some(exit_status);
                break;
            }
            Ok(None) => {}
            Err(error) => {
                failure = Some(format!("等待 Skill 工具失败：{error}"));
                break;
            }
        }
        if started.elapsed() >= timeout {
            failure = Some(format!("Skill 工具执行超过 {} 秒", timeout.as_secs()));
            break;
        }
        thread::sleep(Duration::from_millis(30));
    }

    // Always tear down the process group/job, including after the direct child
    // exits successfully. A tool may have spawned a descendant that inherited
    // stdout/stderr; leaving it alive would keep the pipes open forever.
    process_tree.terminate(&mut child);
    let drain_deadline = Instant::now() + SKILL_IO_DRAIN_TIMEOUT;
    let stdout = receive_skill_stream(stdout_receiver, "标准输出", drain_deadline);
    let stderr = receive_skill_stream(stderr_receiver, "错误输出", drain_deadline);
    let stdin = receive_skill_stdin(stdin_receiver, drain_deadline);

    if failure.is_none() && output_exceeded.load(Ordering::Relaxed) {
        failure = Some(format!(
            "Skill 工具输出超过 {} KiB 安全上限",
            MAX_SKILL_OUTPUT_BYTES / 1024
        ));
    }
    if let Some(failure) = failure {
        return Err(failure);
    }
    let stdout = stdout?;
    let stderr = stderr?;
    stdin?;
    let status = status.ok_or_else(|| "Skill 工具没有返回退出状态".to_owned())?;
    if !status.success() {
        return Err(format!(
            "Skill 工具退出码 {}：{}",
            status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    serde_json::from_str(stdout.trim())
        .map_err(|error| format!("Skill 工具输出不是有效 JSON：{error}；输出：{stdout}"))
}

fn capture_skill_stream<R>(
    mut reader: R,
    total_bytes: Arc<AtomicUsize>,
    output_exceeded: Arc<AtomicBool>,
) -> Receiver<Result<String, String>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut retained = Vec::with_capacity(MAX_SKILL_OUTPUT_BYTES.min(16 * 1024));
        let mut buffer = [0u8; 8192];
        let result = loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break Ok(()),
                Ok(count) => count,
                Err(error) => break Err(format!("读取 Skill 工具输出失败：{error}")),
            };
            let previous = total_bytes.fetch_add(count, Ordering::Relaxed);
            let allowed = MAX_SKILL_OUTPUT_BYTES.saturating_sub(previous).min(count);
            retained.extend_from_slice(&buffer[..allowed]);
            if previous.saturating_add(count) > MAX_SKILL_OUTPUT_BYTES {
                output_exceeded.store(true, Ordering::Relaxed);
                break Ok(());
            }
        };
        let captured = result.map(|()| String::from_utf8_lossy(&retained).into_owned());
        let _ = sender.send(captured);
    });
    receiver
}

fn write_skill_stdin<W>(mut writer: W, payload: Vec<u8>) -> Receiver<Result<(), String>>
where
    W: Write + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = writer
            .write_all(&payload)
            .map_err(|error| format!("写入 Skill 工具输入失败：{error}"));
        let _ = sender.send(result);
    });
    receiver
}

fn receive_skill_stream(
    receiver: Receiver<Result<String, String>>,
    label: &str,
    deadline: Instant,
) -> Result<String, String> {
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "Skill 工具{label}管道未在进程树清理后关闭；已停止等待"
        )),
        Err(RecvTimeoutError::Disconnected) => Err(format!("Skill 工具{label}读取线程意外结束")),
    }
}

fn receive_skill_stdin(
    receiver: Receiver<Result<(), String>>,
    deadline: Instant,
) -> Result<(), String> {
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            Err("Skill 工具标准输入未在进程树清理后关闭；已停止等待".to_owned())
        }
        Err(RecvTimeoutError::Disconnected) => Err("Skill 工具输入线程意外结束".to_owned()),
    }
}

fn resolve_skill_file(
    workspace_root: &Path,
    name: &str,
    relative_path: &str,
    must_exist: bool,
) -> Result<PathBuf, String> {
    ensure_agent_workspace(workspace_root)?;
    validate_skill_name(name)?;
    let root = skills_root(workspace_root).join(name);
    resolve_child_file(&root, relative_path, must_exist)
}

fn resolve_child_file(
    root: &Path,
    relative_path: &str,
    must_exist: bool,
) -> Result<PathBuf, String> {
    if relative_path.trim().is_empty() || Path::new(relative_path).is_absolute() {
        return Err("Skill 文件路径无效".to_owned());
    }
    let mut relative = PathBuf::new();
    for component in Path::new(relative_path).components() {
        match component {
            std::path::Component::Normal(value) => relative.push(value),
            _ => return Err("Skill 文件路径包含不安全片段".to_owned()),
        }
    }
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let target = root.join(relative);
    let checked_parent = if target.exists() {
        fs::canonicalize(&target).map_err(|error| error.to_string())?
    } else {
        let mut parent = target
            .parent()
            .ok_or_else(|| "Skill 文件缺少父目录".to_owned())?;
        while !parent.exists() {
            parent = parent
                .parent()
                .ok_or_else(|| "Skill 文件路径无效".to_owned())?;
        }
        fs::canonicalize(parent).map_err(|error| error.to_string())?
    };
    if !checked_parent.starts_with(&canonical_root) {
        return Err("Skill 文件路径超出能力包目录".to_owned());
    }
    if must_exist && !target.exists() {
        return Err(format!("Skill 文件不存在：{relative_path}"));
    }
    Ok(target)
}

fn collect_skill_entries(
    root: &Path,
    current: &Path,
    output: &mut Vec<AgentSkillEntry>,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if kind.is_dir() {
            output.push(AgentSkillEntry {
                path: relative,
                kind: "directory".to_owned(),
            });
            collect_skill_entries(root, &entry.path(), output)?;
        } else if kind.is_file() {
            output.push(AgentSkillEntry {
                path: relative,
                kind: "file".to_owned(),
            });
        }
        if output.len() > 2048 {
            return Err("Skill 文件超过 2048 个".to_owned());
        }
    }
    Ok(())
}

fn reject_protected_skill_path(relative_path: &str) -> Result<(), String> {
    let normalized = relative_path.replace('\\', "/");
    if matches!(normalized.as_str(), "skill.yaml" | "instructions.md") {
        return Err("skill.yaml 和 instructions.md 是能力包必需文件".to_owned());
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
        || name.len() > 48
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || name.starts_with('-')
        || name.ends_with('-')
    {
        return Err("Skill 名称只允许 1-48 位小写字母、数字和中划线".to_owned());
    }
    Ok(())
}

fn validate_legacy_skill_document(name: &str, content: &str) -> Result<(), String> {
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("Skill 超过 {} KiB", MAX_DOCUMENT_BYTES / 1024));
    }
    let declared = skill_metadata_value(content, "name")
        .ok_or_else(|| "旧版 SKILL.md 需要 YAML frontmatter 和 name 字段".to_owned())?;
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

fn modified_millis(path: &Path) -> u64 {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn agent_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("agent")
}

fn skills_root(workspace_root: &Path) -> PathBuf {
    agent_root(workspace_root).join("skills")
}

struct SkillProcessTree {
    #[cfg(windows)]
    job: Option<WindowsSkillJob>,
    #[cfg(unix)]
    process_group: i32,
}

impl SkillProcessTree {
    fn attach(child: &mut Child) -> Self {
        #[cfg(windows)]
        {
            Self {
                job: WindowsSkillJob::attach(child),
            }
        }
        #[cfg(unix)]
        {
            Self {
                process_group: child.id() as i32,
            }
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = child;
            Self {}
        }
    }

    fn terminate(&mut self, child: &mut Child) {
        #[cfg(windows)]
        {
            if let Some(job) = self.job.as_ref() {
                job.terminate();
            } else {
                terminate_windows_process_tree(child.id());
            }
        }
        #[cfg(unix)]
        {
            // SAFETY: configure_skill_process starts the tool in a dedicated
            // process group whose id is the direct child's pid.
            let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(windows)]
struct WindowsSkillJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl WindowsSkillJob {
    fn attach(child: &mut Child) -> Option<Self> {
        use std::ffi::c_void;
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use std::ptr;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // SAFETY: null attributes/name request an unnamed job owned by this
        // process; the returned handle is closed by Drop below.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return None;
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: limits has the exact layout required by the selected
        // information class and remains alive for the duration of the call.
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } != 0;
        // SAFETY: Child owns a live process handle; AssignProcessToJobObject
        // borrows it for the duration of this call.
        let assigned = configured
            && unsafe { AssignProcessToJobObject(handle, child.as_raw_handle().cast::<c_void>()) }
                != 0;
        if !assigned {
            // SAFETY: handle was returned by CreateJobObjectW and has not yet
            // been closed.
            let _ = unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Self { handle })
    }

    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        // SAFETY: handle is a live job handle owned by self.
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
    }
}

#[cfg(windows)]
impl Drop for WindowsSkillJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        // SAFETY: handle is a live job handle owned by self. Kill-on-close is
        // configured, and the explicit termination also covers normal child
        // exit with still-running descendants.
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
fn terminate_windows_process_tree(process_id: u32) {
    let mut command = Command::new("taskkill");
    command
        .args(["/PID", &process_id.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_skill_process(&mut command);
    let Ok(mut killer) = command.spawn() else {
        return;
    };
    let started = Instant::now();
    loop {
        if killer.try_wait().ok().flatten().is_some() {
            break;
        }
        if started.elapsed() >= Duration::from_secs(1) {
            let _ = killer.kill();
            let _ = killer.wait();
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
fn configure_skill_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn configure_skill_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(any(windows, unix)))]
fn configure_skill_process(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_and_manages_skill_v2_packages() {
        let workspace = std::env::temp_dir().join(format!("drpa-agent-config-{}", Uuid::new_v4()));
        let config = load_workspace_config(&workspace).unwrap();
        assert!(config.agents_markdown.contains("Skills 2.0"));
        assert!(config.agents_markdown.contains("不等于操作系统沙箱"));
        assert!(
            config
                .skills
                .iter()
                .any(|skill| skill.name == "rpaz-development" && skill.format == "skill-v2")
        );
        assert!(
            config
                .skills
                .iter()
                .any(|skill| skill.name == "data-analysis" && skill.format == "skill-v2")
        );

        let manifest = r#"schema: 2
id: test-skill
name: Test Skill
version: 1.0.0
description: 测试可执行能力包
tools:
  - name: echo
    description: 回显参数
    runtime: python
    entry: tools/echo.py:run
    parameters:
      type: object
      properties:
        text: { type: string }
libraries:
  - path: lib
    language: python
"#;
        write_skill_package(&workspace, "test-skill", manifest, "# Test\n").unwrap();
        let tool_path =
            resolve_skill_file(&workspace, "test-skill", "tools/echo.py", false).unwrap();
        write_bounded_text(
            &tool_path,
            "from helper import decorate\n\ndef run(arguments, context):\n    return {'echo': decorate(arguments['text']), 'skillRoot': context['skillRoot']}\n",
        )
        .unwrap();
        let library_path =
            resolve_skill_file(&workspace, "test-skill", "lib/helper.py", false).unwrap();
        write_bounded_text(
            &library_path,
            "def decorate(value):\n    return f'library:{value}'\n",
        )
        .unwrap();
        let package = read_skill_package(&workspace, "test-skill").unwrap();
        assert!(package.manifest_yaml.contains("runtime: python"));
        assert!(
            package
                .entries
                .iter()
                .any(|entry| entry.path == "tools" && entry.kind == "directory")
        );
        assert!(
            package
                .entries
                .iter()
                .any(|entry| entry.path == "lib/helper.py" && entry.kind == "file")
        );
        let definitions = skill_tool_definitions(&workspace, &[]).unwrap();
        assert_eq!(definitions[0]["function"]["name"], "skill_test-skill__echo");
        if let Some(python) = find_test_python() {
            let executed = execute_skill_tool(
                &workspace,
                None,
                &python,
                "skill_test-skill__echo",
                &json!({"text": "hello"}),
                &AgentRunControl::for_tests(),
            )
            .expect("skill tool dispatch")
            .expect("skill tool execution");
            assert_eq!(executed.output["result"]["echo"], "library:hello");
            assert!(
                executed.output["result"]["skillRoot"]
                    .as_str()
                    .is_some_and(|value| value.ends_with("test-skill"))
            );
        }
        assert!(resolve_skill_file(&workspace, "test-skill", "../bad.py", false).is_err());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn skill_process_cleans_descendants_that_inherit_output_pipes() {
        let Some(python) = find_test_python() else {
            return;
        };
        let workspace =
            std::env::temp_dir().join(format!("drpa-skill-descendant-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let script = concat!(
            "import json, subprocess, sys\n",
            "subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n",
            "print(json.dumps({'ok': True}))\n",
        );
        let args = vec!["-c".to_owned(), script.to_owned()];
        let started = Instant::now();
        let output = run_json_process(
            Command::new(python),
            &args,
            &workspace,
            &json!({}),
            Duration::from_secs(5),
            None,
        )
        .unwrap();
        assert_eq!(output["ok"], true);
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "inherited output pipe kept the request alive"
        );
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn skill_process_stops_when_combined_output_exceeds_limit() {
        let Some(python) = find_test_python() else {
            return;
        };
        let workspace =
            std::env::temp_dir().join(format!("drpa-skill-output-cap-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let script = format!(
            "import sys\nsys.stdout.write('x' * {})\nsys.stdout.flush()\n",
            MAX_SKILL_OUTPUT_BYTES + 8192
        );
        let args = vec!["-c".to_owned(), script];
        let started = Instant::now();
        let error = run_json_process(
            Command::new(python),
            &args,
            &workspace,
            &json!({}),
            Duration::from_secs(5),
            None,
        )
        .unwrap_err();
        assert!(error.contains("输出超过"), "unexpected error: {error}");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "output limit did not stop the request promptly"
        );
        let _ = fs::remove_dir_all(workspace);
    }

    fn find_test_python() -> Option<PathBuf> {
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

    #[test]
    fn migrates_legacy_markdown_skill_to_v2() {
        let workspace = std::env::temp_dir().join(format!("drpa-agent-legacy-{}", Uuid::new_v4()));
        let root = skills_root(&workspace).join("legacy-skill");
        fs::create_dir_all(&root).unwrap();
        fs::write(agent_root(&workspace).join(LEGACY_MARKER), b"1\n").unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: legacy-skill\ndescription: 旧版流程\n---\n\n# Legacy\n",
        )
        .unwrap();
        let config = load_workspace_config(&workspace).unwrap();
        assert!(
            config
                .skills
                .iter()
                .any(|skill| skill.name == "legacy-skill" && skill.format == "skill-v2")
        );
        assert!(root.join("skill.yaml").is_file());
        assert!(root.join("instructions.md").is_file());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn structured_memory_appends_events_without_replacing_existing_memory() {
        let workspace = std::env::temp_dir().join(format!("drpa-agent-memory-{}", Uuid::new_v4()));
        ensure_agent_workspace(&workspace).unwrap();
        write_memory_for_agent(&workspace, "# Existing\n\nKeep this line.\n").unwrap();
        let entry =
            append_memory_entry(&workspace, "preference", "language", "简体中文", "user").unwrap();
        assert_eq!(entry["key"], "language");
        let memory = read_memory_for_agent(&workspace).unwrap();
        assert!(memory.contains("Keep this line."));
        assert!(memory.contains("preference/language"));
        let journal =
            fs::read_to_string(agent_root(&workspace).join("memory").join("events.jsonl")).unwrap();
        assert!(journal.contains("简体中文"));
        let _ = fs::remove_dir_all(workspace);
    }
}
