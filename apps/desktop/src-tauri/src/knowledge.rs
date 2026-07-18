use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use drpa_package::safe_relative_path;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::AppPaths;

const KNOWLEDGE_DIRECTORY: &str = "knowledge";
const SEED_MARKER: &str = ".drpa-default-knowledge-v4";
const MAX_MARKDOWN_BYTES: u64 = 8 * 1024 * 1024;

const DEFAULT_DOCUMENTS: &[(&str, &str)] = &[
    (
        "RPAZ 开发指南/00_阅读指南.md",
        include_str!("../knowledge_seed/00_阅读指南.md"),
    ),
    (
        "RPAZ 开发指南/01_快速开始.md",
        include_str!("../knowledge_seed/01_快速开始.md"),
    ),
    (
        "RPAZ 开发指南/02_manifest完整规范.md",
        include_str!("../knowledge_seed/02_manifest完整规范.md"),
    ),
    (
        "RPAZ 开发指南/03_ctx上下文与默认配置.md",
        include_str!("../knowledge_seed/03_ctx上下文与默认配置.md"),
    ),
    (
        "RPAZ 开发指南/04_参数输出与产物.md",
        include_str!("../knowledge_seed/04_参数输出与产物.md"),
    ),
    (
        "RPAZ 开发指南/05_DrissionPage浏览器自动化.md",
        include_str!("../knowledge_seed/05_DrissionPage浏览器自动化.md"),
    ),
    (
        "RPAZ 开发指南/06_日志事件与错误处理.md",
        include_str!("../knowledge_seed/06_日志事件与错误处理.md"),
    ),
    (
        "RPAZ 开发指南/07_Jupyter到RPAZ.md",
        include_str!("../knowledge_seed/07_Jupyter到RPAZ.md"),
    ),
    (
        "RPAZ 开发指南/08_调试测试与发布.md",
        include_str!("../knowledge_seed/08_调试测试与发布.md"),
    ),
    (
        "RPAZ 开发指南/09_完整示例_Bing每日一图.md",
        include_str!("../knowledge_seed/09_完整示例_Bing每日一图.md"),
    ),
    (
        "RPAZ 开发指南/10_AI_Agent协作约定.md",
        include_str!("../knowledge_seed/10_AI_Agent协作约定.md"),
    ),
    (
        "RPAZ 开发指南/11_SQL与数据工作台.md",
        include_str!("../knowledge_seed/11_SQL与数据工作台.md"),
    ),
    (
        "RPAZ 开发指南/12_LocalDify开发平台.md",
        include_str!("../knowledge_seed/12_LocalDify开发平台.md"),
    ),
    (
        "RPAZ 开发指南/13_LocalDify工作流设计器.md",
        include_str!("../knowledge_seed/13_LocalDify工作流设计器.md"),
    ),
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeEntry {
    path: String,
    name: String,
    kind: &'static str,
    size: u64,
    modified_at: u64,
}

pub(crate) fn seed_default_knowledge(workspace_root: &Path) -> std::io::Result<()> {
    let root = workspace_root.join(KNOWLEDGE_DIRECTORY);
    fs::create_dir_all(&root)?;
    let marker = root.join(SEED_MARKER);
    if marker.is_file() {
        return Ok(());
    }

    for (relative, content) in DEFAULT_DOCUMENTS {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if !target.exists() {
            atomic_write(&target, content.as_bytes())?;
        }
    }
    atomic_write(
        &marker,
        b"DRPA default knowledge v4. User documents are never overwritten.\n",
    )
}

pub(crate) fn list_for_agent(workspace_root: &Path) -> Result<Vec<KnowledgeEntry>, String> {
    let root = workspace_root.join(KNOWLEDGE_DIRECTORY);
    fs::create_dir_all(&root).map_err(|error| format!("创建知识库目录失败：{error}"))?;
    let mut entries = Vec::new();
    collect_entries(&root, &root, &mut entries)?;
    entries.sort_by_key(|entry| entry.path.to_lowercase());
    Ok(entries)
}

pub(crate) fn read_for_agent(workspace_root: &Path, relative_path: &str) -> Result<String, String> {
    let root = workspace_root.join(KNOWLEDGE_DIRECTORY);
    fs::create_dir_all(&root).map_err(|error| format!("创建知识库目录失败：{error}"))?;
    let relative = validate_markdown_path(relative_path)?;
    let target = resolve_existing(&root, &relative, false)?;
    let metadata = fs::metadata(&target).map_err(|error| format!("读取文档信息失败：{error}"))?;
    if metadata.len() > MAX_MARKDOWN_BYTES {
        return Err(format!(
            "Markdown 文档超过 {} MiB 限制",
            MAX_MARKDOWN_BYTES / 1024 / 1024
        ));
    }
    fs::read_to_string(target).map_err(|error| format!("读取 Markdown 文档失败：{error}"))
}

pub(crate) fn write_for_agent(
    workspace_root: &Path,
    relative_path: &str,
    content: &str,
) -> Result<(), String> {
    if content.len() as u64 > MAX_MARKDOWN_BYTES {
        return Err(format!(
            "Markdown 文档超过 {} MiB 限制",
            MAX_MARKDOWN_BYTES / 1024 / 1024
        ));
    }
    let root = workspace_root.join(KNOWLEDGE_DIRECTORY);
    fs::create_dir_all(&root).map_err(|error| format!("创建知识库目录失败：{error}"))?;
    let relative = validate_markdown_path(relative_path)?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let mut current = root.clone();
    for component in parent.components() {
        let Component::Normal(name) = component else {
            return Err("知识库路径无效".to_owned());
        };
        current.push(name);
        if !current.exists() {
            fs::create_dir(&current).map_err(|error| format!("创建知识目录失败：{error}"))?;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("知识库路径中包含无效文件夹或符号链接".to_owned());
        }
    }
    let target = resolve_writable(&root, &relative)?;
    atomic_write(&target, content.as_bytes())
        .map_err(|error| format!("保存 Markdown 文档失败：{error}"))
}

#[tauri::command]
pub(crate) fn list_knowledge_entries(
    paths: State<'_, AppPaths>,
) -> Result<Vec<KnowledgeEntry>, String> {
    list_for_agent(&paths.workspace_root)
}

#[tauri::command]
pub(crate) fn read_knowledge_file(
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    read_for_agent(&paths.workspace_root, &relative_path)
}

#[tauri::command]
pub(crate) fn write_knowledge_file(
    relative_path: String,
    content: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    write_for_agent(&paths.workspace_root, &relative_path, &content)
}

#[tauri::command]
pub(crate) fn create_knowledge_entry(
    relative_path: String,
    kind: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let root = knowledge_root(&paths)?;
    let relative = if kind == "file" {
        validate_markdown_path(&relative_path)?
    } else if kind == "directory" {
        validate_relative(&relative_path)?
    } else {
        return Err("知识条目类型必须是 file 或 directory".to_owned());
    };
    let target = resolve_writable(&root, &relative)?;
    if target.exists() {
        return Err("同名文件或文件夹已存在".to_owned());
    }
    if kind == "directory" {
        fs::create_dir(&target).map_err(|error| format!("创建文件夹失败：{error}"))
    } else {
        atomic_write(&target, "# 新文档\n\n在这里开始记录。\n".as_bytes())
            .map_err(|error| format!("创建 Markdown 文档失败：{error}"))
    }
}

#[tauri::command]
pub(crate) fn rename_knowledge_entry(
    relative_path: String,
    target_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let root = knowledge_root(&paths)?;
    let source_relative = validate_relative(&relative_path)?;
    let source = resolve_existing(&root, &source_relative, true)?;
    let source_is_file = source.is_file();
    let target_relative = if source_is_file {
        validate_markdown_path(&target_path)?
    } else {
        validate_relative(&target_path)?
    };
    if target_relative.starts_with(&source_relative) && target_relative != source_relative {
        return Err("文件夹不能移动到自身内部".to_owned());
    }
    let target = resolve_writable(&root, &target_relative)?;
    if target.exists() {
        return Err("目标名称已存在".to_owned());
    }
    fs::rename(source, target).map_err(|error| format!("重命名知识条目失败：{error}"))
}

#[tauri::command]
pub(crate) fn delete_knowledge_entry(
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    let root = knowledge_root(&paths)?;
    let relative = validate_relative(&relative_path)?;
    let target = resolve_existing(&root, &relative, true)?;
    if target.is_dir() {
        fs::remove_dir_all(target).map_err(|error| format!("删除知识目录失败：{error}"))
    } else {
        fs::remove_file(target).map_err(|error| format!("删除知识文档失败：{error}"))
    }
}

#[tauri::command]
pub(crate) fn import_knowledge_files(
    source_paths: Vec<String>,
    target_directory: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<String>, String> {
    if source_paths.is_empty() {
        return Ok(Vec::new());
    }
    let root = knowledge_root(&paths)?;
    let directory = if target_directory.trim().is_empty() {
        PathBuf::new()
    } else {
        validate_relative(&target_directory)?
    };
    let directory_path = if directory.as_os_str().is_empty() {
        root.clone()
    } else {
        resolve_existing(&root, &directory, false)?
    };
    if !directory_path.is_dir() {
        return Err("导入目标不是文件夹".to_owned());
    }

    let mut imported = Vec::new();
    for source_path in source_paths {
        let source = Path::new(&source_path);
        let metadata =
            fs::metadata(source).map_err(|error| format!("读取导入文件失败：{error}"))?;
        if !metadata.is_file() {
            return Err("只支持导入 Markdown 文件".to_owned());
        }
        if metadata.len() > MAX_MARKDOWN_BYTES {
            return Err(format!(
                "导入文件超过 {} MiB 限制",
                MAX_MARKDOWN_BYTES / 1024 / 1024
            ));
        }
        let file_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "导入文件名不是有效 Unicode".to_owned())?;
        validate_markdown_path(file_name)?;
        let target_relative = unique_import_path(&root, &directory, file_name)?;
        let target = resolve_writable(&root, &target_relative)?;
        let mut input = File::open(source).map_err(|error| format!("打开导入文件失败：{error}"))?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        input
            .read_to_end(&mut bytes)
            .map_err(|error| format!("读取导入文件失败：{error}"))?;
        let content =
            String::from_utf8(bytes).map_err(|_| "Markdown 文件必须使用 UTF-8 编码".to_owned())?;
        atomic_write(&target, content.as_bytes())
            .map_err(|error| format!("导入 Markdown 失败：{error}"))?;
        imported.push(path_string(&target_relative));
    }
    Ok(imported)
}

#[tauri::command]
pub(crate) fn export_knowledge_file(
    relative_path: String,
    target_path: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    let root = knowledge_root(&paths)?;
    let relative = validate_markdown_path(&relative_path)?;
    let source = resolve_existing(&root, &relative, false)?;
    let target = PathBuf::from(target_path);
    if target.as_os_str().is_empty() || target.is_dir() {
        return Err("导出目标必须是文件路径".to_owned());
    }
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown") {
        return Err("导出文件扩展名必须是 .md 或 .markdown".to_owned());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{error}"))?;
    }
    fs::copy(source, &target).map_err(|error| format!("导出 Markdown 失败：{error}"))?;
    Ok(target.to_string_lossy().into_owned())
}

fn knowledge_root(paths: &AppPaths) -> Result<PathBuf, String> {
    let root = paths.workspace_root.join(KNOWLEDGE_DIRECTORY);
    fs::create_dir_all(&root).map_err(|error| format!("创建知识库目录失败：{error}"))?;
    Ok(root)
}

fn validate_relative(value: &str) -> Result<PathBuf, String> {
    let relative =
        safe_relative_path(value.trim_end_matches('/')).map_err(|error| error.to_string())?;
    if relative.components().any(|component| {
        matches!(component, Component::Normal(name) if name.to_string_lossy().starts_with(".drpa-"))
    }) {
        return Err("该名称由 DRPA 知识库保留".to_owned());
    }
    Ok(relative)
}

fn validate_markdown_path(value: &str) -> Result<PathBuf, String> {
    let relative = validate_relative(value)?;
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "md" | "markdown") {
        Ok(relative)
    } else {
        Err("知识文档必须使用 .md 或 .markdown 扩展名".to_owned())
    }
}

fn resolve_existing(
    root: &Path,
    relative: &Path,
    allow_directory: bool,
) -> Result<PathBuf, String> {
    verify_directory_chain(root, relative.parent().unwrap_or_else(|| Path::new("")))?;
    let target = root.join(relative);
    let metadata = fs::symlink_metadata(&target).map_err(|_| "知识条目不存在".to_owned())?;
    if metadata.file_type().is_symlink() {
        return Err("知识库不跟随符号链接".to_owned());
    }
    if metadata.is_dir() && !allow_directory {
        return Err("目标必须是 Markdown 文件".to_owned());
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err("知识条目类型不受支持".to_owned());
    }
    Ok(target)
}

fn resolve_writable(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    verify_directory_chain(root, parent)?;
    let target = root.join(relative);
    if let Ok(metadata) = fs::symlink_metadata(&target)
        && metadata.file_type().is_symlink()
    {
        return Err("知识库不写入符号链接".to_owned());
    }
    Ok(target)
}

fn verify_directory_chain(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("知识库路径无效".to_owned());
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).map_err(|_| "目标文件夹不存在".to_owned())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("知识库路径中包含无效文件夹或符号链接".to_owned());
        }
    }
    Ok(())
}

fn collect_entries(
    root: &Path,
    directory: &Path,
    output: &mut Vec<KnowledgeEntry>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| format!("读取知识库失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取知识条目失败：{error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("读取知识条目信息失败：{error}"))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "知识库路径越界".to_owned())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".drpa-") {
            continue;
        }
        if metadata.is_dir() {
            output.push(KnowledgeEntry {
                path: path_string(relative),
                name,
                kind: "directory",
                size: 0,
                modified_at: modified_millis(&metadata),
            });
            collect_entries(root, &path, output)?;
        } else if metadata.is_file() && validate_markdown_path(&path_string(relative)).is_ok() {
            output.push(KnowledgeEntry {
                path: path_string(relative),
                name,
                kind: "file",
                size: metadata.len(),
                modified_at: modified_millis(&metadata),
            });
        }
    }
    Ok(())
}

fn unique_import_path(root: &Path, directory: &Path, file_name: &str) -> Result<PathBuf, String> {
    let original = Path::new(file_name);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("文档");
    let extension = original
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("md");
    for index in 0..10_000 {
        let candidate_name = if index == 0 {
            file_name.to_owned()
        } else {
            format!("{stem} ({index}).{extension}")
        };
        let relative = directory.join(candidate_name);
        if !root.join(&relative).exists() {
            return Ok(relative);
        }
    }
    Err("同名导入文件过多".to_owned())
}

fn atomic_write(target: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::other("target has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".drpa-write-{}.tmp", Uuid::new_v4()));
    let mut output = File::create(&temporary)?;
    output.write_all(content)?;
    output.flush()?;
    output.sync_all()?;

    if target.exists() {
        let backup = parent.join(format!(".drpa-write-{}.bak", Uuid::new_v4()));
        fs::rename(target, &backup)?;
        if let Err(error) = fs::rename(&temporary, target) {
            let _ = fs::rename(&backup, target);
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let _ = fs::remove_file(backup);
    } else {
        fs::rename(temporary, target)?;
    }
    Ok(())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn modified_millis(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |value| value.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_markdown_paths_and_rejects_traversal() {
        assert_eq!(
            validate_markdown_path("目录/说明.md").unwrap(),
            PathBuf::from("目录/说明.md")
        );
        assert!(validate_markdown_path("../说明.md").is_err());
        assert!(validate_markdown_path("目录/脚本.py").is_err());
        assert!(validate_relative(".drpa-default-knowledge-v1").is_err());
    }

    #[test]
    fn seed_is_idempotent_and_keeps_user_changes() {
        let root = std::env::temp_dir().join(format!("drpa-knowledge-test-{}", Uuid::new_v4()));
        seed_default_knowledge(&root).unwrap();
        let guide = root.join("knowledge/RPAZ 开发指南/00_阅读指南.md");
        assert!(guide.is_file());
        fs::write(&guide, "用户修改").unwrap();
        seed_default_knowledge(&root).unwrap();
        assert_eq!(fs::read_to_string(&guide).unwrap(), "用户修改");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_name_is_deduplicated() {
        let root =
            std::env::temp_dir().join(format!("drpa-knowledge-name-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("guide.md"), "one").unwrap();
        assert_eq!(
            unique_import_path(&root, Path::new(""), "guide.md").unwrap(),
            PathBuf::from("guide (1).md")
        );
        let _ = fs::remove_dir_all(root);
    }
}
