use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use calamine::{Data, Reader, open_workbook_auto};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use tauri::State;
use url::Url;
use uuid::Uuid;
use zip::ZipArchive;

use crate::AppPaths;

const INDEX_SCHEMA: i64 = 1;
const EMBEDDING_DIMENSIONS: usize = 384;
const SOURCE_FILE_LIMIT: u64 = 50 * 1024 * 1024;
const SOURCE_TEXT_LIMIT: usize = 8 * 1024 * 1024;
const MAX_SOURCES_PER_IMPORT: usize = 100;
const MAX_CHUNKS_PER_SOURCE: usize = 20_000;
const CHUNK_CHARACTERS: usize = 900;
const CHUNK_OVERLAP_CHARACTERS: usize = 140;
const MAX_SEARCH_LIMIT: usize = 30;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeBaseSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) source_count: usize,
    pub(crate) chunk_count: usize,
    pub(crate) status: String,
    pub(crate) updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeBaseSource {
    pub(crate) id: String,
    pub(crate) knowledge_base_id: String,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) status: String,
    pub(crate) chunk_count: usize,
    pub(crate) size_bytes: u64,
    pub(crate) uri: String,
    pub(crate) last_error: String,
    pub(crate) updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeBaseSearchResult {
    pub(crate) knowledge_base_id: String,
    pub(crate) knowledge_base_name: String,
    pub(crate) source_id: String,
    pub(crate) source_name: String,
    pub(crate) chunk_id: String,
    pub(crate) content: String,
    pub(crate) citation: String,
    pub(crate) score: f32,
    pub(crate) vector_score: f32,
    pub(crate) keyword_score: f32,
}

#[derive(Debug)]
struct StoredChunk {
    knowledge_base_id: String,
    knowledge_base_name: String,
    source_id: String,
    source_name: String,
    chunk_id: String,
    ordinal: usize,
    content: String,
    embedding: Vec<f32>,
}

#[tauri::command]
pub(crate) fn list_knowledge_bases(
    paths: State<'_, AppPaths>,
) -> Result<Vec<KnowledgeBaseSummary>, String> {
    list_knowledge_bases_at(&paths.workspace_root)
}

#[tauri::command]
pub(crate) fn create_knowledge_base(
    name: String,
    description: String,
    paths: State<'_, AppPaths>,
) -> Result<KnowledgeBaseSummary, String> {
    create_knowledge_base_at(&paths.workspace_root, &name, &description)
}

#[tauri::command]
pub(crate) fn delete_knowledge_base(
    knowledge_base_id: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_id(&knowledge_base_id)?;
    let connection = open_index(&paths.workspace_root)?;
    let removed = connection
        .execute(
            "DELETE FROM knowledge_bases WHERE id = ?1",
            params![knowledge_base_id],
        )
        .map_err(|error| format!("删除知识库失败：{error}"))?;
    if removed == 0 {
        return Err("知识库不存在".to_owned());
    }
    let source_root = sources_root(&paths.workspace_root).join(&knowledge_base_id);
    if source_root.is_dir() {
        fs::remove_dir_all(source_root)
            .map_err(|error| format!("删除知识库源文件失败：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn list_knowledge_base_sources(
    knowledge_base_id: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<KnowledgeBaseSource>, String> {
    list_sources_at(&paths.workspace_root, &knowledge_base_id)
}

#[tauri::command]
pub(crate) fn import_knowledge_base_files(
    knowledge_base_id: String,
    source_paths: Vec<String>,
    paths: State<'_, AppPaths>,
) -> Result<Vec<KnowledgeBaseSource>, String> {
    if source_paths.len() > MAX_SOURCES_PER_IMPORT {
        return Err(format!(
            "一次最多导入 {MAX_SOURCES_PER_IMPORT} 个知识库文件"
        ));
    }
    ensure_knowledge_base_exists(&paths.workspace_root, &knowledge_base_id)?;
    source_paths
        .iter()
        .map(|source| ingest_file_at(&paths.workspace_root, &knowledge_base_id, Path::new(source)))
        .collect()
}

#[tauri::command]
pub(crate) fn import_knowledge_base_directory(
    knowledge_base_id: String,
    directory_path: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<KnowledgeBaseSource>, String> {
    ensure_knowledge_base_exists(&paths.workspace_root, &knowledge_base_id)?;
    let directory = PathBuf::from(directory_path);
    let metadata =
        fs::symlink_metadata(&directory).map_err(|error| format!("读取知识库目录失败：{error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("知识库目录必须是真实的本地文件夹，不能是符号链接".to_owned());
    }
    let mut files = Vec::new();
    collect_supported_files(&directory, &mut files)?;
    if files.len() > MAX_SOURCES_PER_IMPORT {
        return Err(format!(
            "目录中有 {} 个受支持文件，一次最多导入 {MAX_SOURCES_PER_IMPORT} 个",
            files.len()
        ));
    }
    files
        .iter()
        .map(|source| ingest_file_at(&paths.workspace_root, &knowledge_base_id, source))
        .collect()
}

#[tauri::command]
pub(crate) fn add_knowledge_base_text(
    knowledge_base_id: String,
    title: String,
    content: String,
    paths: State<'_, AppPaths>,
) -> Result<KnowledgeBaseSource, String> {
    ingest_text_at(
        &paths.workspace_root,
        &knowledge_base_id,
        &title,
        &content,
        "text",
        "",
    )
}

#[tauri::command]
pub(crate) fn add_knowledge_base_url(
    knowledge_base_id: String,
    url: String,
    paths: State<'_, AppPaths>,
) -> Result<KnowledgeBaseSource, String> {
    ensure_knowledge_base_exists(&paths.workspace_root, &knowledge_base_id)?;
    let parsed = validate_remote_url(&url)?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(45)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent
        .get(parsed.as_str())
        .header("User-Agent", "DRPA-Knowledge-Indexer/2.0")
        .call()
        .map_err(|error| format!("读取知识库 URL 失败：{error}"))?;
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|value| value > SOURCE_TEXT_LIMIT as u64)
    {
        return Err("URL 内容超过 8 MiB 限制".to_owned());
    }
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("读取知识库 URL 响应失败：{error}"))?;
    if body.len() > SOURCE_TEXT_LIMIT {
        return Err("URL 内容超过 8 MiB 限制".to_owned());
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let text = if content_type.contains("html")
        || body
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("<!doctype")
        || body.trim_start().to_ascii_lowercase().starts_with("<html")
    {
        strip_markup(&body)
    } else {
        body
    };
    let title = parsed
        .host_str()
        .map_or_else(|| "网页资料".to_owned(), |host| host.to_owned());
    ingest_text_at(
        &paths.workspace_root,
        &knowledge_base_id,
        &title,
        &text,
        "url",
        parsed.as_str(),
    )
}

#[tauri::command]
pub(crate) fn delete_knowledge_base_source(
    knowledge_base_id: String,
    source_id: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_id(&knowledge_base_id)?;
    validate_id(&source_id)?;
    let connection = open_index(&paths.workspace_root)?;
    let stored_path = connection
        .query_row(
            "SELECT stored_path FROM knowledge_sources WHERE id = ?1 AND knowledge_base_id = ?2",
            params![source_id, knowledge_base_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("读取知识源失败：{error}"))?
        .ok_or_else(|| "知识源不存在".to_owned())?;
    connection
        .execute(
            "DELETE FROM knowledge_sources WHERE id = ?1 AND knowledge_base_id = ?2",
            params![source_id, knowledge_base_id],
        )
        .map_err(|error| format!("删除知识源失败：{error}"))?;
    if !stored_path.is_empty() {
        let target = sources_root(&paths.workspace_root).join(stored_path);
        if target.is_file() {
            fs::remove_file(target).map_err(|error| format!("删除知识源文件失败：{error}"))?;
        }
    }
    touch_knowledge_base(&connection, &knowledge_base_id)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn search_knowledge_base(
    knowledge_base_ids: Vec<String>,
    query: String,
    limit: usize,
    paths: State<'_, AppPaths>,
) -> Result<Vec<KnowledgeBaseSearchResult>, String> {
    search_at(&paths.workspace_root, &knowledge_base_ids, &query, limit)
}

pub(crate) fn list_for_agent(workspace_root: &Path) -> Result<Vec<KnowledgeBaseSummary>, String> {
    list_knowledge_bases_at(workspace_root)
}

pub(crate) fn search_for_agent(
    workspace_root: &Path,
    knowledge_base_ids: &[String],
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeBaseSearchResult>, String> {
    search_at(workspace_root, knowledge_base_ids, query, limit)
}

fn list_knowledge_bases_at(workspace_root: &Path) -> Result<Vec<KnowledgeBaseSummary>, String> {
    let connection = open_index(workspace_root)?;
    let mut statement = connection
        .prepare(
            "SELECT kb.id, kb.name, kb.description, kb.status, kb.updated_at, \
                    COUNT(DISTINCT source.id), COUNT(chunk.id) \
             FROM knowledge_bases kb \
             LEFT JOIN knowledge_sources source ON source.knowledge_base_id = kb.id \
             LEFT JOIN knowledge_chunks chunk ON chunk.source_id = source.id \
             GROUP BY kb.id ORDER BY kb.updated_at DESC, kb.name COLLATE NOCASE",
        )
        .map_err(|error| format!("读取知识库列表失败：{error}"))?;
    statement
        .query_map([], |row| {
            Ok(KnowledgeBaseSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                status: row.get(3)?,
                updated_at: row.get::<_, i64>(4)?.max(0) as u64,
                source_count: row.get::<_, i64>(5)?.max(0) as usize,
                chunk_count: row.get::<_, i64>(6)?.max(0) as usize,
            })
        })
        .map_err(|error| format!("读取知识库列表失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("解析知识库列表失败：{error}"))
}

fn create_knowledge_base_at(
    workspace_root: &Path,
    name: &str,
    description: &str,
) -> Result<KnowledgeBaseSummary, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("知识库名称应为 1 到 80 个字符".to_owned());
    }
    if description.chars().count() > 500 {
        return Err("知识库说明不能超过 500 个字符".to_owned());
    }
    let id = format!("kb-{}", Uuid::new_v4().simple());
    let now = now_millis();
    let connection = open_index(workspace_root)?;
    connection
        .execute(
            "INSERT INTO knowledge_bases(id, name, description, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'ready', ?4, ?4)",
            params![id, name, description.trim(), now as i64],
        )
        .map_err(|error| format!("创建知识库失败：{error}"))?;
    fs::create_dir_all(sources_root(workspace_root).join(&id))
        .map_err(|error| format!("创建知识库源文件目录失败：{error}"))?;
    Ok(KnowledgeBaseSummary {
        id,
        name: name.to_owned(),
        description: description.trim().to_owned(),
        source_count: 0,
        chunk_count: 0,
        status: "ready".to_owned(),
        updated_at: now,
    })
}

fn list_sources_at(
    workspace_root: &Path,
    knowledge_base_id: &str,
) -> Result<Vec<KnowledgeBaseSource>, String> {
    ensure_knowledge_base_exists(workspace_root, knowledge_base_id)?;
    let connection = open_index(workspace_root)?;
    let mut statement = connection
        .prepare(
            "SELECT source.id, source.knowledge_base_id, source.name, source.kind, source.status, \
                    source.size_bytes, source.uri, source.last_error, source.updated_at, COUNT(chunk.id) \
             FROM knowledge_sources source \
             LEFT JOIN knowledge_chunks chunk ON chunk.source_id = source.id \
             WHERE source.knowledge_base_id = ?1 \
             GROUP BY source.id ORDER BY source.updated_at DESC, source.name COLLATE NOCASE",
        )
        .map_err(|error| format!("读取知识源列表失败：{error}"))?;
    statement
        .query_map(params![knowledge_base_id], |row| {
            Ok(KnowledgeBaseSource {
                id: row.get(0)?,
                knowledge_base_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                status: row.get(4)?,
                size_bytes: row.get::<_, i64>(5)?.max(0) as u64,
                uri: row.get(6)?,
                last_error: row.get(7)?,
                updated_at: row.get::<_, i64>(8)?.max(0) as u64,
                chunk_count: row.get::<_, i64>(9)?.max(0) as usize,
            })
        })
        .map_err(|error| format!("读取知识源列表失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("解析知识源列表失败：{error}"))
}

fn ingest_file_at(
    workspace_root: &Path,
    knowledge_base_id: &str,
    source: &Path,
) -> Result<KnowledgeBaseSource, String> {
    let metadata =
        fs::symlink_metadata(source).map_err(|error| format!("读取导入文件失败：{error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("知识源必须是真实的本地文件，不能是符号链接".to_owned());
    }
    if metadata.len() > SOURCE_FILE_LIMIT {
        return Err(format!(
            "知识源 {} 超过 50 MiB 限制",
            source.to_string_lossy()
        ));
    }
    ensure_supported_extension(source)?;
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "知识源文件名不是有效 Unicode".to_owned())?
        .to_owned();
    let source_id = format!("source-{}", Uuid::new_v4().simple());
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let relative = PathBuf::from(knowledge_base_id).join(format!("{source_id}.{extension}"));
    let target = sources_root(workspace_root).join(&relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建知识源目录失败：{error}"))?;
    }
    fs::copy(source, &target).map_err(|error| format!("复制知识源失败：{error}"))?;
    let content = match extract_plain_text(&target) {
        Ok(content) => content,
        Err(error) => {
            let _ = fs::remove_file(&target);
            return Err(format!("解析知识源“{name}”失败：{error}"));
        }
    };
    insert_source(
        workspace_root,
        knowledge_base_id,
        &source_id,
        &name,
        "file",
        &source.to_string_lossy(),
        &relative.to_string_lossy().replace('\\', "/"),
        metadata.len(),
        &content,
    )
}

fn ingest_text_at(
    workspace_root: &Path,
    knowledge_base_id: &str,
    title: &str,
    content: &str,
    kind: &str,
    uri: &str,
) -> Result<KnowledgeBaseSource, String> {
    ensure_knowledge_base_exists(workspace_root, knowledge_base_id)?;
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 160 {
        return Err("知识源标题应为 1 到 160 个字符".to_owned());
    }
    if content.trim().is_empty() {
        return Err("知识源内容不能为空".to_owned());
    }
    if content.len() > SOURCE_TEXT_LIMIT {
        return Err("知识源文本超过 8 MiB 限制".to_owned());
    }
    let source_id = format!("source-{}", Uuid::new_v4().simple());
    let relative = PathBuf::from(knowledge_base_id).join(format!("{source_id}.txt"));
    let target = sources_root(workspace_root).join(&relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建知识源目录失败：{error}"))?;
    }
    fs::write(&target, content).map_err(|error| format!("保存知识源文本失败：{error}"))?;
    insert_source(
        workspace_root,
        knowledge_base_id,
        &source_id,
        title,
        kind,
        uri,
        &relative.to_string_lossy().replace('\\', "/"),
        content.len() as u64,
        content,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_source(
    workspace_root: &Path,
    knowledge_base_id: &str,
    source_id: &str,
    name: &str,
    kind: &str,
    uri: &str,
    stored_path: &str,
    size_bytes: u64,
    content: &str,
) -> Result<KnowledgeBaseSource, String> {
    ensure_knowledge_base_exists(workspace_root, knowledge_base_id)?;
    let chunks = split_chunks(content);
    if chunks.is_empty() {
        return Err("知识源没有可索引的文本".to_owned());
    }
    if chunks.len() > MAX_CHUNKS_PER_SOURCE {
        return Err(format!("知识源切分后超过 {MAX_CHUNKS_PER_SOURCE} 个片段"));
    }
    let now = now_millis();
    let mut connection = open_index(workspace_root)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("开始知识源索引事务失败：{error}"))?;
    transaction
        .execute(
            "INSERT INTO knowledge_sources(\
                id, knowledge_base_id, name, kind, uri, stored_path, content_hash, \
                size_bytes, status, last_error, created_at, updated_at\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'ready', '', ?9, ?9)",
            params![
                source_id,
                knowledge_base_id,
                name,
                kind,
                uri,
                stored_path,
                content_fingerprint(content.as_bytes()),
                size_bytes as i64,
                now as i64
            ],
        )
        .map_err(|error| format!("保存知识源失败：{error}"))?;
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO knowledge_chunks(\
                    id, source_id, ordinal, content, token_count, embedding, created_at\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|error| format!("准备知识片段索引失败：{error}"))?;
        for (ordinal, chunk) in chunks.iter().enumerate() {
            let tokens = tokenize(chunk);
            let embedding = encode_embedding(&embed_tokens(&tokens));
            statement
                .execute(params![
                    format!("chunk-{}", Uuid::new_v4().simple()),
                    source_id,
                    ordinal as i64,
                    chunk,
                    tokens.len() as i64,
                    embedding,
                    now as i64,
                ])
                .map_err(|error| format!("保存知识片段失败：{error}"))?;
        }
    }
    transaction
        .execute(
            "UPDATE knowledge_bases SET updated_at = ?2, status = 'ready' WHERE id = ?1",
            params![knowledge_base_id, now as i64],
        )
        .map_err(|error| format!("更新知识库状态失败：{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("提交知识源索引失败：{error}"))?;
    Ok(KnowledgeBaseSource {
        id: source_id.to_owned(),
        knowledge_base_id: knowledge_base_id.to_owned(),
        name: name.to_owned(),
        kind: kind.to_owned(),
        status: "ready".to_owned(),
        chunk_count: chunks.len(),
        size_bytes,
        uri: uri.to_owned(),
        last_error: String::new(),
        updated_at: now,
    })
}

fn search_at(
    workspace_root: &Path,
    knowledge_base_ids: &[String],
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeBaseSearchResult>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("知识库查询不能为空".to_owned());
    }
    if query.chars().count() > 2_000 {
        return Err("知识库查询不能超过 2000 个字符".to_owned());
    }
    for id in knowledge_base_ids {
        validate_id(id)?;
    }
    let selected = knowledge_base_ids.iter().collect::<HashSet<_>>();
    let connection = open_index(workspace_root)?;
    let mut statement = connection
        .prepare(
            "SELECT kb.id, kb.name, source.id, source.name, chunk.id, chunk.ordinal, \
                    chunk.content, chunk.embedding \
             FROM knowledge_chunks chunk \
             JOIN knowledge_sources source ON source.id = chunk.source_id \
             JOIN knowledge_bases kb ON kb.id = source.knowledge_base_id \
             WHERE source.status = 'ready' AND kb.status = 'ready'",
        )
        .map_err(|error| format!("准备知识库查询失败：{error}"))?;
    let chunks = statement
        .query_map([], |row| {
            let embedding = row.get::<_, Vec<u8>>(7)?;
            Ok(StoredChunk {
                knowledge_base_id: row.get(0)?,
                knowledge_base_name: row.get(1)?,
                source_id: row.get(2)?,
                source_name: row.get(3)?,
                chunk_id: row.get(4)?,
                ordinal: row.get::<_, i64>(5)?.max(0) as usize,
                content: row.get(6)?,
                embedding: decode_embedding(&embedding).unwrap_or_default(),
            })
        })
        .map_err(|error| format!("执行知识库查询失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("解析知识片段失败：{error}"))?;
    let query_tokens = tokenize(query);
    let query_embedding = embed_tokens(&query_tokens);
    let query_counts = token_counts(&query_tokens);
    let mut scored = chunks
        .into_iter()
        .filter(|chunk| selected.is_empty() || selected.contains(&chunk.knowledge_base_id))
        .filter_map(|chunk| {
            if chunk.embedding.len() != EMBEDDING_DIMENSIONS {
                return None;
            }
            let vector_score = cosine_similarity(&query_embedding, &chunk.embedding).max(0.0);
            let keyword_score = lexical_similarity(&query_counts, &tokenize(&chunk.content));
            if vector_score <= 0.0001 && keyword_score <= 0.0001 {
                return None;
            }
            let score = vector_score * 0.7 + keyword_score * 0.3;
            Some((score, vector_score, keyword_score, chunk))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| right.2.total_cmp(&left.2))
    });
    Ok(scored
        .into_iter()
        .take(limit.clamp(1, MAX_SEARCH_LIMIT))
        .map(
            |(score, vector_score, keyword_score, chunk)| KnowledgeBaseSearchResult {
                citation: format!("{} · 片段 {}", chunk.source_name, chunk.ordinal + 1),
                knowledge_base_id: chunk.knowledge_base_id,
                knowledge_base_name: chunk.knowledge_base_name,
                source_id: chunk.source_id,
                source_name: chunk.source_name,
                chunk_id: chunk.chunk_id,
                content: chunk.content,
                score,
                vector_score,
                keyword_score,
            },
        )
        .collect())
}

pub(crate) fn extract_plain_text(path: &Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "txt" | "md" | "markdown" | "csv" | "tsv" | "json" | "jsonl" | "yaml" | "yml" | "xml"
        | "html" | "htm" => {
            let source =
                fs::read_to_string(path).map_err(|error| format!("读取文本文件失败：{error}"))?;
            if matches!(extension.as_str(), "html" | "htm" | "xml") {
                Ok(strip_markup(&source))
            } else {
                Ok(source)
            }
        }
        "pdf" => {
            pdf_extract::extract_text(path).map_err(|error| format!("提取 PDF 文本失败：{error}"))
        }
        "docx" => extract_open_xml(path, "word/document.xml"),
        "pptx" => extract_presentation(path),
        "xls" | "xlsx" | "xlsb" | "ods" => extract_spreadsheet(path),
        _ => Err(format!("不支持的知识源格式：.{extension}")),
    }
}

fn extract_open_xml(path: &Path, entry_name: &str) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("打开 Office 文档失败：{error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("Office 文档压缩结构无效：{error}"))?;
    if archive.len() > 20_000 {
        return Err("Office 文档包含过多压缩条目".to_owned());
    }
    let mut entry = archive
        .by_name(entry_name)
        .map_err(|_| format!("Office 文档缺少 {entry_name}"))?;
    if entry.size() > SOURCE_TEXT_LIMIT as u64 {
        return Err("Office 文档正文超过 8 MiB 限制".to_owned());
    }
    let mut xml = String::new();
    entry
        .read_to_string(&mut xml)
        .map_err(|error| format!("读取 Office 文档正文失败：{error}"))?;
    Ok(strip_markup(&xml))
}

fn extract_presentation(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("打开 PowerPoint 失败：{error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("PowerPoint 压缩结构无效：{error}"))?;
    if archive.len() > 20_000 {
        return Err("PowerPoint 包含过多压缩条目".to_owned());
    }
    let mut names = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index(index)
                .ok()
                .map(|entry| entry.name().to_owned())
        })
        .filter(|name| {
            name.starts_with("ppt/slides/slide")
                && name.ends_with(".xml")
                && !name.contains("_rels")
        })
        .collect::<Vec<_>>();
    names.sort_by_key(|name| slide_number(name));
    let mut output = String::new();
    for (index, name) in names.iter().enumerate() {
        let mut entry = archive
            .by_name(name)
            .map_err(|error| format!("读取幻灯片失败：{error}"))?;
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|error| format!("读取幻灯片正文失败：{error}"))?;
        output.push_str(&format!("\n\n## 幻灯片 {}\n", index + 1));
        output.push_str(&strip_markup(&xml));
        if output.len() > SOURCE_TEXT_LIMIT {
            return Err("PowerPoint 提取文本超过 8 MiB 限制".to_owned());
        }
    }
    Ok(output.trim().to_owned())
}

fn extract_spreadsheet(path: &Path) -> Result<String, String> {
    let mut workbook =
        open_workbook_auto(path).map_err(|error| format!("打开 Excel 工作簿失败：{error}"))?;
    let names = workbook.sheet_names().to_owned();
    if names.len() > 100 {
        return Err("Excel 工作表数量超过 100 个限制".to_owned());
    }
    let mut output = String::new();
    for name in names {
        let range = workbook
            .worksheet_range(&name)
            .map_err(|error| format!("读取工作表“{name}”失败：{error}"))?;
        output.push_str(&format!("\n\n## 工作表：{name}\n"));
        for row in range.rows().take(100_000) {
            output.push_str(
                &row.iter()
                    .take(512)
                    .map(excel_display_value)
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
            output.push('\n');
            if output.len() > SOURCE_TEXT_LIMIT {
                return Err("Excel 提取文本超过 8 MiB 限制".to_owned());
            }
        }
    }
    Ok(output.trim().to_owned())
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

fn strip_markup(source: &str) -> String {
    let source = source
        .replace("</w:p>", "\n")
        .replace("</a:p>", "\n")
        .replace("</p>", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<w:tab/>", "\t")
        .replace("<a:br/>", "\n");
    let mut output = String::with_capacity(source.len());
    let mut in_tag = false;
    for character in source.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    let output = output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'");
    normalize_extracted_text(&output)
}

fn normalize_extracted_text(source: &str) -> String {
    let mut output = String::new();
    let mut empty_lines = 0;
    for line in source.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line = line.trim();
        if line.is_empty() {
            empty_lines += 1;
            if empty_lines <= 1 && !output.is_empty() {
                output.push('\n');
            }
            continue;
        }
        empty_lines = 0;
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(line);
    }
    output.trim().to_owned()
}

fn split_chunks(source: &str) -> Vec<String> {
    let normalized = normalize_extracted_text(source);
    if normalized.is_empty() {
        return Vec::new();
    }
    let characters = normalized.chars().collect::<Vec<_>>();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < characters.len() && chunks.len() < MAX_CHUNKS_PER_SOURCE {
        let mut end = (start + CHUNK_CHARACTERS).min(characters.len());
        if end < characters.len() {
            let lower = start + CHUNK_CHARACTERS / 2;
            if let Some(position) = (lower..end).rev().find(|index| {
                matches!(
                    characters[*index],
                    '\n' | '。' | '！' | '？' | '.' | '!' | '?'
                )
            }) {
                end = position + 1;
            }
        }
        let chunk = characters[start..end]
            .iter()
            .collect::<String>()
            .trim()
            .to_owned();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        if end == characters.len() {
            break;
        }
        start = end.saturating_sub(CHUNK_OVERLAP_CHARACTERS).max(start + 1);
    }
    chunks
}

fn tokenize(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut cjk_run = Vec::new();
    let flush_word = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word));
        }
    };
    let flush_cjk = |run: &mut Vec<char>, tokens: &mut Vec<String>| {
        if run.is_empty() {
            return;
        }
        for character in run.iter() {
            tokens.push(character.to_string());
        }
        for pair in run.windows(2) {
            tokens.push(pair.iter().collect());
        }
        run.clear();
    };
    for character in source.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            flush_cjk(&mut cjk_run, &mut tokens);
            word.push(character);
        } else if is_cjk(character) {
            flush_word(&mut word, &mut tokens);
            cjk_run.push(character);
        } else {
            flush_word(&mut word, &mut tokens);
            flush_cjk(&mut cjk_run, &mut tokens);
        }
    }
    flush_word(&mut word, &mut tokens);
    flush_cjk(&mut cjk_run, &mut tokens);
    tokens.retain(|token| token.chars().count() > 1 || token.chars().any(is_cjk));
    tokens
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

fn embed_tokens(tokens: &[String]) -> Vec<f32> {
    let counts = token_counts(tokens);
    let mut vector = vec![0.0_f32; EMBEDDING_DIMENSIONS];
    for (token, count) in counts {
        let hash = fnv1a(token.as_bytes(), 0xcbf29ce484222325);
        let index = (hash as usize) % EMBEDDING_DIMENSIONS;
        let sign = if hash & (1 << 63) == 0 { 1.0 } else { -1.0 };
        vector[index] += sign * (1.0 + (count as f32).ln());
    }
    normalize_vector(&mut vector);
    vector
}

fn token_counts(tokens: &[String]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for token in tokens {
        *counts.entry(token.clone()).or_insert(0) += 1;
    }
    counts
}

fn lexical_similarity(query: &HashMap<String, usize>, document: &[String]) -> f32 {
    if query.is_empty() || document.is_empty() {
        return 0.0;
    }
    let document = token_counts(document);
    let overlap = query
        .iter()
        .map(|(token, count)| {
            document
                .get(token)
                .map_or(0.0, |value| (*count).min(*value) as f32)
        })
        .sum::<f32>();
    let denominator =
        (query.values().sum::<usize>() as f32 * document.values().sum::<usize>() as f32).sqrt();
    if denominator == 0.0 {
        0.0
    } else {
        (overlap / denominator).min(1.0)
    }
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum()
}

fn normalize_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector {
            *value /= norm;
        }
    }
}

fn encode_embedding(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for value in embedding {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_embedding(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() != EMBEDDING_DIMENSIONS * 4 {
        return Err("知识向量维度无效".to_owned());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn open_index(workspace_root: &Path) -> Result<Connection, String> {
    let root = knowledge_base_root(workspace_root);
    fs::create_dir_all(&root).map_err(|error| format!("创建知识库目录失败：{error}"))?;
    let connection = Connection::open(root.join("index.sqlite3"))
        .map_err(|error| format!("打开知识库索引失败：{error}"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| format!("配置知识库索引失败：{error}"))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS knowledge_meta(
               key TEXT PRIMARY KEY,
               value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS knowledge_bases(
               id TEXT PRIMARY KEY,
               name TEXT NOT NULL,
               description TEXT NOT NULL DEFAULT '',
               status TEXT NOT NULL DEFAULT 'ready',
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS knowledge_sources(
               id TEXT PRIMARY KEY,
               knowledge_base_id TEXT NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
               name TEXT NOT NULL,
               kind TEXT NOT NULL,
               uri TEXT NOT NULL DEFAULT '',
               stored_path TEXT NOT NULL DEFAULT '',
               content_hash TEXT NOT NULL,
               size_bytes INTEGER NOT NULL DEFAULT 0,
               status TEXT NOT NULL DEFAULT 'ready',
               last_error TEXT NOT NULL DEFAULT '',
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_knowledge_sources_base
               ON knowledge_sources(knowledge_base_id);
             CREATE TABLE IF NOT EXISTS knowledge_chunks(
               id TEXT PRIMARY KEY,
               source_id TEXT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
               ordinal INTEGER NOT NULL,
               content TEXT NOT NULL,
               token_count INTEGER NOT NULL,
               embedding BLOB NOT NULL,
               created_at INTEGER NOT NULL,
               UNIQUE(source_id, ordinal)
             );
             CREATE INDEX IF NOT EXISTS idx_knowledge_chunks_source
               ON knowledge_chunks(source_id);
             INSERT INTO knowledge_meta(key, value)
               VALUES ('schema', '1') ON CONFLICT(key) DO NOTHING;",
        )
        .map_err(|error| format!("初始化知识库索引失败：{error}"))?;
    let schema = connection
        .query_row(
            "SELECT value FROM knowledge_meta WHERE key = 'schema'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("读取知识库索引版本失败：{error}"))?;
    if schema.parse::<i64>().unwrap_or_default() != INDEX_SCHEMA {
        return Err(format!("不支持的知识库索引版本：{schema}"));
    }
    Ok(connection)
}

fn ensure_knowledge_base_exists(
    workspace_root: &Path,
    knowledge_base_id: &str,
) -> Result<(), String> {
    validate_id(knowledge_base_id)?;
    let exists = open_index(workspace_root)?
        .query_row(
            "SELECT 1 FROM knowledge_bases WHERE id = ?1",
            params![knowledge_base_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| format!("读取知识库失败：{error}"))?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err("知识库不存在".to_owned())
    }
}

fn touch_knowledge_base(connection: &Connection, knowledge_base_id: &str) -> Result<(), String> {
    connection
        .execute(
            "UPDATE knowledge_bases SET updated_at = ?2 WHERE id = ?1",
            params![knowledge_base_id, now_millis() as i64],
        )
        .map_err(|error| format!("更新知识库时间失败：{error}"))?;
    Ok(())
}

fn collect_supported_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    if output.len() > MAX_SOURCES_PER_IMPORT {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|error| format!("读取知识库目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取知识库目录项失败：{error}"))?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| format!("读取文件信息失败：{error}"))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_supported_files(&path, output)?;
        } else if metadata.is_file() && ensure_supported_extension(&path).is_ok() {
            output.push(path);
        }
    }
    Ok(())
}

fn ensure_supported_extension(path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        extension.as_str(),
        "txt"
            | "md"
            | "markdown"
            | "csv"
            | "tsv"
            | "json"
            | "jsonl"
            | "yaml"
            | "yml"
            | "xml"
            | "html"
            | "htm"
            | "pdf"
            | "docx"
            | "xls"
            | "xlsx"
            | "xlsb"
            | "ods"
            | "pptx"
    ) {
        Ok(())
    } else {
        Err(format!("不支持的知识源格式：.{extension}"))
    }
}

fn validate_remote_url(value: &str) -> Result<Url, String> {
    let parsed = Url::parse(value.trim()).map_err(|error| format!("URL 无效：{error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("知识库 URL 只支持 http/https".to_owned());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("知识库 URL 不能包含用户名或密码".to_owned());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "知识库 URL 缺少主机名".to_owned())?;
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| !is_public_address(address))
    {
        return Err("知识库 URL 不能访问本机或私有网络地址".to_owned());
    }
    Ok(parsed)
}

fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            !(address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_documentation()
                || address.is_unspecified())
        }
        IpAddr::V6(address) => {
            !(address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local())
        }
    }
}

fn validate_id(value: &str) -> Result<(), String> {
    if !(3..=80).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("知识库标识无效".to_owned());
    }
    Ok(())
}

fn content_fingerprint(bytes: &[u8]) -> String {
    format!(
        "{:016x}{:016x}",
        fnv1a(bytes, 0xcbf29ce484222325),
        fnv1a(bytes, 0x84222325cbf29ce4)
    )
}

fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn slide_number(name: &str) -> usize {
    name.trim_end_matches(".xml")
        .rsplit("slide")
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(usize::MAX)
}

fn knowledge_base_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("knowledge-bases")
}

fn sources_root(workspace_root: &Path) -> PathBuf {
    knowledge_base_root(workspace_root).join("sources")
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

    fn temporary_workspace() -> PathBuf {
        std::env::temp_dir().join(format!("drpa-kb-test-{}", Uuid::new_v4().simple()))
    }

    #[test]
    fn local_hybrid_index_keeps_knowledge_documents_separate() {
        let workspace = temporary_workspace();
        fs::create_dir_all(workspace.join("knowledge")).unwrap();
        fs::write(
            workspace.join("knowledge/not-indexed.md"),
            "# 机密术语\nblue-orchid-only",
        )
        .unwrap();
        let base = create_knowledge_base_at(&workspace, "产品资料", "测试知识库").unwrap();
        ingest_text_at(
            &workspace,
            &base.id,
            "运行手册",
            "DRPA 使用 RPAZ 包执行自动化流程。运行工作台可以查看日志与产物。",
            "text",
            "",
        )
        .unwrap();

        let results = search_at(&workspace, &[], "RPAZ 自动化运行", 5).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("RPAZ"));
        assert!(
            search_at(&workspace, &[], "blue-orchid-only", 5)
                .unwrap()
                .is_empty()
        );
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn vector_round_trip_and_chinese_tokenization_are_stable() {
        let tokens = tokenize("数据工作台 data runtime 数据查询");
        assert!(tokens.iter().any(|token| token == "数据"));
        assert!(tokens.iter().any(|token| token == "runtime"));
        let vector = embed_tokens(&tokens);
        let decoded = decode_embedding(&encode_embedding(&vector)).unwrap();
        assert_eq!(decoded.len(), EMBEDDING_DIMENSIONS);
        assert!(cosine_similarity(&vector, &decoded) > 0.999);
    }

    #[test]
    fn url_import_rejects_local_network_targets() {
        assert!(validate_remote_url("file:///etc/passwd").is_err());
        assert!(validate_remote_url("http://localhost:8080/private").is_err());
        assert!(validate_remote_url("http://127.0.0.1/private").is_err());
        assert!(validate_remote_url("https://example.com/docs").is_ok());
    }

    #[test]
    fn chunking_has_overlap_without_stalling() {
        let source = "数据与流程。".repeat(1_000);
        let chunks = split_chunks(&source);
        assert!(chunks.len() > 2);
        assert!(chunks.iter().all(|chunk| !chunk.is_empty()));
        assert!(chunks.len() < MAX_CHUNKS_PER_SOURCE);
    }
}
