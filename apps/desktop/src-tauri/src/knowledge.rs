use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use drpa_package::safe_relative_path;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::AppPaths;

const KNOWLEDGE_DIRECTORY: &str = "knowledge";
const SEED_MARKER: &str = ".drpa-default-knowledge-v6";
const MANUAL_LAYOUT_MIGRATION_MARKER: &str = ".drpa-manual-layout-v6";
const LEGACY_MANUAL_DIRECTORY: &str = "DRPA使用说明";
const MANUAL_DIRECTORY: &str = "DRPA使用文档";
const MAX_MARKDOWN_BYTES: u64 = 8 * 1024 * 1024;

// The v5 defaults are hex-encoded so Git line-ending conversion cannot change the
// reference bytes. They are migration fixtures only and are never seeded as documents.
struct LegacyManual {
    relative_path: &'static str,
    target_stem: &'static str,
    default_content_hex: &'static str,
}

const LEGACY_MANUALS: &[LegacyManual] = &[
    LegacyManual {
        relative_path: "README.md",
        target_stem: "00_使用总览",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/00_使用总览.hex"),
    },
    LegacyManual {
        relative_path: "01_总览/使用说明.md",
        target_stem: "01_总览",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/01_总览.hex"),
    },
    LegacyManual {
        relative_path: "02_RPAZ包/使用说明.md",
        target_stem: "02_RPAZ包",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/02_RPAZ包.hex"),
    },
    LegacyManual {
        relative_path: "03_开发工作室/使用说明.md",
        target_stem: "03_开发工作室",
        default_content_hex: include_str!(
            "../knowledge_seed/manual_migrations/v5/03_开发工作室.hex"
        ),
    },
    LegacyManual {
        relative_path: "04_数据工作台/使用说明.md",
        target_stem: "04_数据工作台",
        default_content_hex: include_str!(
            "../knowledge_seed/manual_migrations/v5/04_数据工作台.hex"
        ),
    },
    LegacyManual {
        relative_path: "05_运行工作台/使用说明.md",
        target_stem: "05_运行工作台",
        default_content_hex: include_str!(
            "../knowledge_seed/manual_migrations/v5/05_运行工作台.hex"
        ),
    },
    LegacyManual {
        relative_path: "06_运行记录/使用说明.md",
        target_stem: "06_运行记录",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/06_运行记录.hex"),
    },
    LegacyManual {
        relative_path: "07_自动化计划/使用说明.md",
        target_stem: "07_自动化计划",
        default_content_hex: include_str!(
            "../knowledge_seed/manual_migrations/v5/07_自动化计划.hex"
        ),
    },
    LegacyManual {
        relative_path: "08_流程设计/使用说明.md",
        target_stem: "08_流程设计",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/08_流程设计.hex"),
    },
    LegacyManual {
        relative_path: "09_AI_Agent/使用说明.md",
        target_stem: "09_AI_Agent",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/09_AI_Agent.hex"),
    },
    LegacyManual {
        relative_path: "10_知识库/使用说明.md",
        target_stem: "10_知识库",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/10_知识库.hex"),
    },
    LegacyManual {
        relative_path: "11_插件/使用说明.md",
        target_stem: "11_插件",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/11_插件.hex"),
    },
    LegacyManual {
        relative_path: "12_运行环境/使用说明.md",
        target_stem: "12_运行环境",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/12_运行环境.hex"),
    },
    LegacyManual {
        relative_path: "13_凭据保险箱/使用说明.md",
        target_stem: "13_凭据保险箱",
        default_content_hex: include_str!(
            "../knowledge_seed/manual_migrations/v5/13_凭据保险箱.hex"
        ),
    },
    LegacyManual {
        relative_path: "14_设置/使用说明.md",
        target_stem: "14_设置",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/14_设置.hex"),
    },
    LegacyManual {
        relative_path: "15_知识文档/使用说明.md",
        target_stem: "15_知识文档",
        default_content_hex: include_str!("../knowledge_seed/manual_migrations/v5/15_知识文档.hex"),
    },
];

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
    (
        "DRPA使用文档/00_使用总览.md",
        include_str!("../knowledge_seed/DRPA使用文档/00_使用总览.md"),
    ),
    (
        "DRPA使用文档/01_总览.md",
        include_str!("../knowledge_seed/DRPA使用文档/01_总览.md"),
    ),
    (
        "DRPA使用文档/02_RPAZ包.md",
        include_str!("../knowledge_seed/DRPA使用文档/02_RPAZ包.md"),
    ),
    (
        "DRPA使用文档/03_开发工作室.md",
        include_str!("../knowledge_seed/DRPA使用文档/03_开发工作室.md"),
    ),
    (
        "DRPA使用文档/04_数据工作台.md",
        include_str!("../knowledge_seed/DRPA使用文档/04_数据工作台.md"),
    ),
    (
        "DRPA使用文档/05_运行工作台.md",
        include_str!("../knowledge_seed/DRPA使用文档/05_运行工作台.md"),
    ),
    (
        "DRPA使用文档/06_运行记录.md",
        include_str!("../knowledge_seed/DRPA使用文档/06_运行记录.md"),
    ),
    (
        "DRPA使用文档/07_自动化计划.md",
        include_str!("../knowledge_seed/DRPA使用文档/07_自动化计划.md"),
    ),
    (
        "DRPA使用文档/08_流程设计.md",
        include_str!("../knowledge_seed/DRPA使用文档/08_流程设计.md"),
    ),
    (
        "DRPA使用文档/09_AI_Agent.md",
        include_str!("../knowledge_seed/DRPA使用文档/09_AI_Agent.md"),
    ),
    (
        "DRPA使用文档/10_知识库.md",
        include_str!("../knowledge_seed/DRPA使用文档/10_知识库.md"),
    ),
    (
        "DRPA使用文档/11_插件.md",
        include_str!("../knowledge_seed/DRPA使用文档/11_插件.md"),
    ),
    (
        "DRPA使用文档/12_运行环境.md",
        include_str!("../knowledge_seed/DRPA使用文档/12_运行环境.md"),
    ),
    (
        "DRPA使用文档/13_凭据保险箱.md",
        include_str!("../knowledge_seed/DRPA使用文档/13_凭据保险箱.md"),
    ),
    (
        "DRPA使用文档/14_设置.md",
        include_str!("../knowledge_seed/DRPA使用文档/14_设置.md"),
    ),
    (
        "DRPA使用文档/15_知识文档.md",
        include_str!("../knowledge_seed/DRPA使用文档/15_知识文档.md"),
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
    ensure_real_directory(&root)?;
    let migration_marker = root.join(MANUAL_LAYOUT_MIGRATION_MARKER);
    if !regular_file_exists(&migration_marker)? {
        migrate_legacy_manual_layout(&root)?;
        atomic_write(
            &migration_marker,
            b"DRPA manual layout migration v6 completed without overwriting user files.\n",
        )?;
    }

    let marker = root.join(SEED_MARKER);
    if regular_file_exists(&marker)? {
        return Ok(());
    }
    ensure_real_directory(&root.join(MANUAL_DIRECTORY))?;

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
        b"DRPA default knowledge v6. User documents are never overwritten.\n",
    )
}

fn regular_file_exists(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("refusing symbolic-link marker: {}", path.display()),
        )),
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn migrate_legacy_manual_layout(knowledge_root: &Path) -> std::io::Result<()> {
    let legacy_root = knowledge_root.join(LEGACY_MANUAL_DIRECTORY);
    let legacy_metadata = match fs::symlink_metadata(&legacy_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if legacy_metadata.file_type().is_symlink() || !legacy_metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "legacy manual root is not a real directory: {}",
                legacy_root.display()
            ),
        ));
    }

    let manual_root = knowledge_root.join(MANUAL_DIRECTORY);
    ensure_real_directory(&manual_root)?;

    for manual in LEGACY_MANUALS {
        let relative = Path::new(manual.relative_path);
        let source = legacy_root.join(relative);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        verify_migration_directory_chain(&legacy_root, relative.parent().unwrap_or(Path::new("")))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "legacy manual entry is not a real file: {}",
                    source.display()
                ),
            ));
        }
        let content = fs::read(&source)?;
        let default_content = decode_hex_snapshot(manual.default_content_hex)?;
        if content == default_content {
            fs::remove_file(&source)?;
            continue;
        }
        let target_name =
            preserved_file_name(manual.target_stem, "旧版用户修改", Some("md"), &content);
        preserve_then_remove(&source, &manual_root.join(target_name), &content)?;
    }

    let mut unknown_files = Vec::new();
    collect_legacy_regular_files(&legacy_root, &legacy_root, &mut unknown_files)?;
    unknown_files.sort();
    for source in unknown_files {
        let relative = source.strip_prefix(&legacy_root).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "legacy manual path escaped its root",
            )
        })?;
        let content = fs::read(&source)?;
        let (stem, extension) = flattened_legacy_name(relative);
        let target_name = preserved_file_name(
            &format!("旧版_{stem}"),
            "用户文件",
            extension.as_deref(),
            &content,
        );
        preserve_then_remove(&source, &manual_root.join(target_name), &content)?;
    }

    remove_empty_legacy_directories(&legacy_root)?;
    Ok(())
}

fn ensure_real_directory(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "migration target is not a real directory: {}",
                    path.display()
                ),
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path),
        Err(error) => Err(error),
    }
}

fn verify_migration_directory_chain(root: &Path, relative: &Path) -> std::io::Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "legacy manual contains an invalid relative path",
            ));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "legacy manual contains a symbolic link or invalid directory: {}",
                    current.display()
                ),
            ));
        }
    }
    Ok(())
}

fn collect_legacy_regular_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.starts_with(root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "legacy manual entry escaped its root",
            ));
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("legacy manual contains a symbolic link: {}", path.display()),
            ));
        }
        if metadata.is_dir() {
            collect_legacy_regular_files(root, &path, output)?;
        } else if metadata.is_file() {
            output.push(path);
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "legacy manual contains an unsupported entry: {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

fn flattened_legacy_name(relative: &Path) -> (String, Option<String>) {
    let mut components = Vec::new();
    for component in relative.components() {
        if let Component::Normal(value) = component {
            components.push(value.to_string_lossy().into_owned());
        }
    }
    let flattened = sanitize_migration_name(&components.join("__"));
    let path = Path::new(&flattened);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("未命名")
        .chars()
        .take(96)
        .collect();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.chars().take(16).collect());
    (stem, extension)
}

fn sanitize_migration_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect()
}

fn preserved_file_name(stem: &str, label: &str, extension: Option<&str>, content: &[u8]) -> String {
    let hash = stable_content_hash(content);
    match extension.filter(|value| !value.is_empty()) {
        Some(extension) => format!("{stem}_{label}_{hash}.{extension}"),
        None => format!("{stem}_{label}_{hash}"),
    }
}

fn stable_content_hash(content: &[u8]) -> String {
    let mut first = 0xcbf2_9ce4_8422_2325_u64;
    let mut second = 0x8422_2325_cbf2_9ce4_u64;
    for byte in content {
        first ^= u64::from(*byte);
        first = first.wrapping_mul(0x0000_0100_0000_01b3);
        second ^= u64::from(*byte).wrapping_add(0x9d);
        second = second.rotate_left(7).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{first:016x}{second:016x}")
}

fn decode_hex_snapshot(value: &str) -> std::io::Result<Vec<u8>> {
    let value = value.trim();
    if value.len() % 2 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "legacy manual snapshot has an odd number of hexadecimal digits",
        ));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "legacy manual snapshot contains invalid hexadecimal data",
                )
            })
        })
        .collect()
}

fn preserve_then_remove(source: &Path, target: &Path, content: &[u8]) -> std::io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "migration target is not a regular file: {}",
                    target.display()
                ),
            ));
        }
        Ok(_) => {
            if fs::read(target)? != content {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "content-addressed migration target conflicts: {}",
                        target.display()
                    ),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_new_file(target, content)?;
        }
        Err(error) => return Err(error),
    }
    if fs::read(target)? != content {
        return Err(std::io::Error::other(format!(
            "migration verification failed: {}",
            target.display()
        )));
    }
    fs::remove_file(source)
}

fn write_new_file(target: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    if let Err(error) = output
        .write_all(content)
        .and_then(|_| output.flush())
        .and_then(|_| output.sync_all())
    {
        drop(output);
        let _ = fs::remove_file(target);
        return Err(error);
    }
    Ok(())
}

fn remove_empty_legacy_directories(directory: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "refusing to remove invalid legacy directory: {}",
                directory.display()
            ),
        ));
    }
    let mut child_directories = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "legacy manual contains a symbolic link: {}",
                    entry.path().display()
                ),
            ));
        }
        if metadata.is_dir() {
            child_directories.push(entry.path());
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::DirectoryNotEmpty,
                format!(
                    "legacy manual still contains a file: {}",
                    entry.path().display()
                ),
            ));
        }
    }
    child_directories.sort();
    for child in child_directories {
        remove_empty_legacy_directories(&child)?;
    }
    fs::remove_dir(directory)
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
        let manual_root = root.join("knowledge/DRPA使用文档");
        for file_name in [
            "00_使用总览.md",
            "01_总览.md",
            "02_RPAZ包.md",
            "03_开发工作室.md",
            "04_数据工作台.md",
            "05_运行工作台.md",
            "06_运行记录.md",
            "07_自动化计划.md",
            "08_流程设计.md",
            "09_AI_Agent.md",
            "10_知识库.md",
            "11_插件.md",
            "12_运行环境.md",
            "13_凭据保险箱.md",
            "14_设置.md",
            "15_知识文档.md",
        ] {
            assert!(manual_root.join(file_name).is_file(), "missing {file_name}");
        }
        assert!(
            fs::read_dir(&manual_root).unwrap().all(|entry| entry
                .unwrap()
                .file_type()
                .unwrap()
                .is_file()),
            "the v6 manual must use a flat directory"
        );
        fs::write(&guide, "用户修改").unwrap();
        let manual = manual_root.join("01_总览.md");
        fs::write(&manual, "用户修改的新手册").unwrap();
        seed_default_knowledge(&root).unwrap();
        assert_eq!(fs::read_to_string(&guide).unwrap(), "用户修改");
        assert_eq!(fs::read_to_string(&manual).unwrap(), "用户修改的新手册");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clean_legacy_manual_is_removed_before_flat_defaults_are_seeded() {
        let root = std::env::temp_dir().join(format!("drpa-knowledge-clean-v5-{}", Uuid::new_v4()));
        let knowledge_root = root.join(KNOWLEDGE_DIRECTORY);
        let legacy_root = knowledge_root.join(LEGACY_MANUAL_DIRECTORY);
        for manual in LEGACY_MANUALS {
            let target = legacy_root.join(manual.relative_path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(
                target,
                decode_hex_snapshot(manual.default_content_hex).unwrap(),
            )
            .unwrap();
        }
        fs::write(
            knowledge_root.join(".drpa-default-knowledge-v5"),
            "legacy seed marker",
        )
        .unwrap();

        seed_default_knowledge(&root).unwrap();

        assert!(!legacy_root.exists());
        assert!(
            knowledge_root
                .join(MANUAL_LAYOUT_MIGRATION_MARKER)
                .is_file()
        );
        assert!(knowledge_root.join(SEED_MARKER).is_file());
        assert!(
            knowledge_root
                .join(MANUAL_DIRECTORY)
                .join("00_使用总览.md")
                .is_file()
        );
        assert!(
            fs::read_dir(knowledge_root.join(MANUAL_DIRECTORY))
                .unwrap()
                .all(|entry| entry.unwrap().file_type().unwrap().is_file())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn modified_and_unknown_legacy_files_are_flattened_without_overwriting() {
        let root =
            std::env::temp_dir().join(format!("drpa-knowledge-modified-v5-{}", Uuid::new_v4()));
        let knowledge_root = root.join(KNOWLEDGE_DIRECTORY);
        let legacy_root = knowledge_root.join(LEGACY_MANUAL_DIRECTORY);
        let modified = b"# user-modified overview\n\nkeep this exactly\n";
        let modified_source = legacy_root.join(LEGACY_MANUALS[1].relative_path);
        fs::create_dir_all(modified_source.parent().unwrap()).unwrap();
        fs::write(&modified_source, modified).unwrap();
        let unknown = b"# private appendix\n";
        let unknown_source = legacy_root.join("我的附录/检查清单.md");
        fs::create_dir_all(unknown_source.parent().unwrap()).unwrap();
        fs::write(&unknown_source, unknown).unwrap();

        let manual_root = knowledge_root.join(MANUAL_DIRECTORY);
        fs::create_dir_all(&manual_root).unwrap();
        fs::write(manual_root.join("01_总览.md"), "用户已创建的新版同名文档").unwrap();

        seed_default_knowledge(&root).unwrap();

        let modified_name = preserved_file_name("01_总览", "旧版用户修改", Some("md"), modified);
        assert_eq!(fs::read(manual_root.join(modified_name)).unwrap(), modified);
        let (unknown_stem, unknown_extension) =
            flattened_legacy_name(Path::new("我的附录/检查清单.md"));
        let unknown_name = preserved_file_name(
            &format!("旧版_{unknown_stem}"),
            "用户文件",
            unknown_extension.as_deref(),
            unknown,
        );
        assert_eq!(fs::read(manual_root.join(unknown_name)).unwrap(), unknown);
        assert_eq!(
            fs::read_to_string(manual_root.join("01_总览.md")).unwrap(),
            "用户已创建的新版同名文档"
        );
        assert!(!legacy_root.exists());

        let before = fs::read_dir(&manual_root).unwrap().count();
        seed_default_knowledge(&root).unwrap();
        assert_eq!(fs::read_dir(&manual_root).unwrap().count(), before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migration_retry_accepts_an_identical_companion_without_duplication() {
        let root = std::env::temp_dir().join(format!("drpa-knowledge-retry-v5-{}", Uuid::new_v4()));
        let knowledge_root = root.join(KNOWLEDGE_DIRECTORY);
        let legacy_root = knowledge_root.join(LEGACY_MANUAL_DIRECTORY);
        let source = legacy_root.join(LEGACY_MANUALS[2].relative_path);
        let content = b"modified package guide";
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, content).unwrap();
        let manual_root = knowledge_root.join(MANUAL_DIRECTORY);
        fs::create_dir_all(&manual_root).unwrap();
        let target_name = preserved_file_name("02_RPAZ包", "旧版用户修改", Some("md"), content);
        fs::write(manual_root.join(&target_name), content).unwrap();

        seed_default_knowledge(&root).unwrap();

        assert!(!legacy_root.exists());
        assert_eq!(fs::read(manual_root.join(target_name)).unwrap(), content);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migration_conflict_never_overwrites_or_deletes_the_source() {
        let root =
            std::env::temp_dir().join(format!("drpa-knowledge-conflict-v5-{}", Uuid::new_v4()));
        let knowledge_root = root.join(KNOWLEDGE_DIRECTORY);
        let legacy_root = knowledge_root.join(LEGACY_MANUAL_DIRECTORY);
        let source = legacy_root.join(LEGACY_MANUALS[3].relative_path);
        let content = b"modified studio guide";
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, content).unwrap();
        let manual_root = knowledge_root.join(MANUAL_DIRECTORY);
        fs::create_dir_all(&manual_root).unwrap();
        let target_name = preserved_file_name("03_开发工作室", "旧版用户修改", Some("md"), content);
        let target = manual_root.join(target_name);
        fs::write(&target, "conflicting existing content").unwrap();

        assert!(seed_default_knowledge(&root).is_err());
        assert_eq!(fs::read(&source).unwrap(), content);
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "conflicting existing content"
        );
        assert!(!knowledge_root.join(MANUAL_LAYOUT_MIGRATION_MARKER).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn layout_migration_runs_even_when_the_default_seed_marker_exists() {
        let root =
            std::env::temp_dir().join(format!("drpa-knowledge-marker-v6-{}", Uuid::new_v4()));
        let knowledge_root = root.join(KNOWLEDGE_DIRECTORY);
        let legacy_root = knowledge_root.join(LEGACY_MANUAL_DIRECTORY);
        let source = legacy_root.join(LEGACY_MANUALS[0].relative_path);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(
            &source,
            decode_hex_snapshot(LEGACY_MANUALS[0].default_content_hex).unwrap(),
        )
        .unwrap();
        fs::write(knowledge_root.join(SEED_MARKER), "seed completed first").unwrap();

        seed_default_knowledge(&root).unwrap();

        assert!(!legacy_root.exists());
        assert!(
            knowledge_root
                .join(MANUAL_LAYOUT_MIGRATION_MARKER)
                .is_file()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn layout_migration_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("drpa-knowledge-symlink-v5-{}", Uuid::new_v4()));
        let knowledge_root = root.join(KNOWLEDGE_DIRECTORY);
        let legacy_root = knowledge_root.join(LEGACY_MANUAL_DIRECTORY);
        fs::create_dir_all(&legacy_root).unwrap();
        let outside = root.join("outside.md");
        fs::write(&outside, "outside").unwrap();
        symlink(&outside, legacy_root.join("linked.md")).unwrap();

        assert!(seed_default_knowledge(&root).is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside");
        assert!(!knowledge_root.join(MANUAL_LAYOUT_MIGRATION_MARKER).exists());
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
