use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::State;
use uuid::Uuid;
use zip::ZipArchive;

use crate::AppPaths;

const WORKER_PROTOCOL_VERSION: u32 = 1;
const SUPPORTED_FORMATS: &[&str] = &["pdf", "docx", "xlsx", "pptx"];
const MACRO_FORMATS: &[&str] = &["docm", "dotm", "xlsm", "xltm", "pptm", "potm", "ppsm"];
const MAX_DOCUMENT_BYTES: u64 = 100 * 1024 * 1024;
const MAX_WORKER_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_WORKER_ERROR_BYTES: u64 = 256 * 1024;
const MAX_ZIP_ENTRIES: usize = 20_000;
const MAX_ZIP_UNCOMPRESSED_BYTES: u64 = 250 * 1024 * 1024;
const MAX_ZIP_COMPRESSION_RATIO: u64 = 250;
const WORKER_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDocumentAttachment {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) format: String,
    pub(crate) size_bytes: u64,
    pub(crate) imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDocumentArtifact {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) format: String,
    pub(crate) size_bytes: u64,
    pub(crate) created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDocumentRead {
    pub(crate) document_id: String,
    pub(crate) format: String,
    pub(crate) content: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDocumentExport {
    pub(crate) artifact_id: String,
    pub(crate) path: String,
    pub(crate) size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDocument {
    id: String,
    name: String,
    format: String,
    size_bytes: u64,
    timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_id: Option<String>,
}

#[tauri::command]
pub(crate) fn import_agent_document(
    source_path: String,
    session_id: String,
    paths: State<'_, AppPaths>,
) -> Result<AgentDocumentAttachment, String> {
    import_agent_document_at(&paths.workspace_root, &session_id, Path::new(&source_path))
}

#[tauri::command]
pub(crate) fn list_agent_artifacts(
    session_id: String,
    paths: State<'_, AppPaths>,
) -> Result<Vec<AgentDocumentArtifact>, String> {
    list_agent_artifacts_at(&paths.workspace_root, &session_id)
}

#[tauri::command]
pub(crate) fn export_agent_artifact(
    session_id: String,
    artifact_id: String,
    destination_path: String,
    paths: State<'_, AppPaths>,
) -> Result<AgentDocumentExport, String> {
    export_agent_artifact_at(
        &paths.workspace_root,
        &session_id,
        &artifact_id,
        Path::new(&destination_path),
    )
}

/// Reads an imported attachment or generated artifact through the constrained worker.
pub(crate) fn read_document(
    workspace_root: &Path,
    python: &Path,
    session_id: &str,
    document_id: &str,
) -> Result<AgentDocumentRead, String> {
    validate_session_id(session_id)?;
    let resolved = resolve_document(workspace_root, session_id, document_id)?;
    let result = run_worker(
        python,
        json!({
            "version": WORKER_PROTOCOL_VERSION,
            "operation": "read",
            "workspace_root": workspace_root,
            "session_id": session_id,
            "input_path": resolved.path,
        }),
    )?;
    Ok(AgentDocumentRead {
        document_id: document_id.to_owned(),
        format: resolved.record.format,
        content: result,
    })
}

/// Creates a new document. The worker can only write to this session's artifact directory.
pub(crate) fn create_document(
    workspace_root: &Path,
    python: &Path,
    session_id: &str,
    format: &str,
    title: &str,
    content: &Value,
    file_name: Option<&str>,
) -> Result<AgentDocumentArtifact, String> {
    validate_session_id(session_id)?;
    let format = validate_format(format)?;
    let pending = pending_artifact(workspace_root, session_id, file_name, format, None)?;
    let request = json!({
        "version": WORKER_PROTOCOL_VERSION,
        "operation": "create",
        "workspace_root": workspace_root,
        "session_id": session_id,
        "output_path": pending.path,
        "format": format,
        "title": title,
        "content": content,
    });
    finish_artifact(pending, || run_worker(python, request))
}

/// Converts a session attachment/artifact by extracting its safe document model and recreating it.
pub(crate) fn convert_document(
    workspace_root: &Path,
    python: &Path,
    session_id: &str,
    document_id: &str,
    target_format: &str,
    title: Option<&str>,
    file_name: Option<&str>,
) -> Result<AgentDocumentArtifact, String> {
    validate_session_id(session_id)?;
    let source = resolve_document(workspace_root, session_id, document_id)?;
    let target_format = validate_format(target_format)?;
    let pending = pending_artifact(
        workspace_root,
        session_id,
        file_name,
        target_format,
        Some(document_id.to_owned()),
    )?;
    let request = json!({
        "version": WORKER_PROTOCOL_VERSION,
        "operation": "convert",
        "workspace_root": workspace_root,
        "session_id": session_id,
        "input_path": source.path,
        "output_path": pending.path,
        "format": target_format,
        "title": title.unwrap_or_default(),
    });
    finish_artifact(pending, || run_worker(python, request))
}

fn import_agent_document_at(
    workspace_root: &Path,
    session_id: &str,
    source_path: &Path,
) -> Result<AgentDocumentAttachment, String> {
    validate_session_id(session_id)?;
    let source_metadata = fs::symlink_metadata(source_path)
        .map_err(|error| format!("无法读取待导入文档：{error}"))?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
        return Err("只能导入普通文件，不能导入目录或符号链接".to_owned());
    }
    if source_metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "文档不能超过 {} MB",
            MAX_DOCUMENT_BYTES / 1024 / 1024
        ));
    }
    let source = source_path
        .canonicalize()
        .map_err(|error| format!("无法解析待导入文档：{error}"))?;
    let format = format_from_path(&source)?;
    inspect_ooxml(&source, format)?;

    let id = format!("att_{}", Uuid::new_v4().simple());
    let session_root = prepare_session_root(
        workspace_root,
        &attachment_session_root(workspace_root, session_id),
    )?;
    let files_root = session_root.join("files");
    let metadata_root = session_root.join("metadata");
    let destination = files_root.join(format!("{id}.{format}"));
    copy_new_limited(&source, &destination)?;

    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("document.{format}"));
    let record = StoredDocument {
        id: id.clone(),
        name: safe_display_name(&name, format),
        format: format.to_owned(),
        size_bytes: fs::metadata(&destination)
            .map_err(|error| format!("无法读取已导入文档：{error}"))?
            .len(),
        timestamp: Utc::now().to_rfc3339(),
        source_id: None,
    };
    if let Err(error) = write_record_new(&metadata_root, &record) {
        let _ = fs::remove_file(&destination);
        return Err(error);
    }
    Ok(attachment_from_record(record))
}

fn list_agent_artifacts_at(
    workspace_root: &Path,
    session_id: &str,
) -> Result<Vec<AgentDocumentArtifact>, String> {
    validate_session_id(session_id)?;
    let raw_session_root = artifact_session_root(workspace_root, session_id);
    if !raw_session_root.is_dir() {
        return Ok(Vec::new());
    }
    let session_root = existing_session_root(workspace_root, &raw_session_root)?;
    let metadata_root = session_root.join("metadata");
    let mut artifacts = Vec::new();
    for entry in
        fs::read_dir(&metadata_root).map_err(|error| format!("无法列出文档产物：{error}"))?
    {
        let entry = entry.map_err(|error| format!("无法读取文档产物：{error}"))?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let record = read_record_path(&entry.path())?;
        validate_stored_record(&record, "art")?;
        let document_path = artifact_file_path(workspace_root, session_id, &record)?;
        let size_bytes = fs::metadata(&document_path)
            .map_err(|_| format!("文档产物文件已丢失：{}", record.name))?
            .len();
        let mut artifact = artifact_from_record(record);
        artifact.size_bytes = size_bytes;
        artifacts.push(artifact);
    }
    artifacts.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(artifacts)
}

fn export_agent_artifact_at(
    workspace_root: &Path,
    session_id: &str,
    artifact_id: &str,
    destination_path: &Path,
) -> Result<AgentDocumentExport, String> {
    validate_session_id(session_id)?;
    validate_opaque_id(artifact_id, "art")?;
    let session_root = existing_session_root(
        workspace_root,
        &artifact_session_root(workspace_root, session_id),
    )?;
    let record = read_record(&session_root.join("metadata"), artifact_id)?;
    validate_stored_record(&record, "art")?;
    let source = artifact_file_path(workspace_root, session_id, &record)?;

    let destination = if destination_path.is_dir() {
        destination_path.join(&record.name)
    } else {
        destination_path.to_path_buf()
    };
    if destination.exists() {
        return Err("导出目标已存在，禁止覆盖".to_owned());
    }
    if format_from_path(&destination)? != record.format {
        return Err(format!("导出文件扩展名必须是 .{}", record.format));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "导出路径没有父目录".to_owned())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("导出目录不存在或不可访问：{error}"))?;
    let file_name = destination
        .file_name()
        .ok_or_else(|| "导出路径缺少文件名".to_owned())?;
    let final_destination = canonical_parent.join(file_name);
    copy_new_limited(&source, &final_destination)?;
    let size_bytes = fs::metadata(&final_destination)
        .map_err(|error| format!("无法读取导出文件：{error}"))?
        .len();
    Ok(AgentDocumentExport {
        artifact_id: artifact_id.to_owned(),
        path: final_destination.to_string_lossy().into_owned(),
        size_bytes,
    })
}

struct ResolvedDocument {
    record: StoredDocument,
    path: PathBuf,
}

fn resolve_document(
    workspace_root: &Path,
    session_id: &str,
    document_id: &str,
) -> Result<ResolvedDocument, String> {
    let (kind, raw_session_root) = if document_id.starts_with("att_") {
        ("att", attachment_session_root(workspace_root, session_id))
    } else if document_id.starts_with("art_") {
        ("art", artifact_session_root(workspace_root, session_id))
    } else {
        return Err("无效的文档 ID".to_owned());
    };
    let session_root = existing_session_root(workspace_root, &raw_session_root)?;
    validate_opaque_id(document_id, kind)?;
    let record = read_record(&session_root.join("metadata"), document_id)?;
    validate_stored_record(&record, kind)?;
    let path = session_root
        .join("files")
        .join(format!("{}.{}", record.id, record.format));
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| format!("文档文件已丢失：{}", record.name))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_DOCUMENT_BYTES
    {
        return Err("文档文件不安全或超过大小限制".to_owned());
    }
    Ok(ResolvedDocument { record, path })
}

struct PendingArtifact {
    record: StoredDocument,
    path: PathBuf,
    metadata_root: PathBuf,
}

fn pending_artifact(
    workspace_root: &Path,
    session_id: &str,
    file_name: Option<&str>,
    format: &str,
    source_id: Option<String>,
) -> Result<PendingArtifact, String> {
    let id = format!("art_{}", Uuid::new_v4().simple());
    let session_root = prepare_session_root(
        workspace_root,
        &artifact_session_root(workspace_root, session_id),
    )?;
    let files_root = session_root.join("files");
    let metadata_root = session_root.join("metadata");
    let default_name = format!("document-{suffix}.{format}", suffix = &id[id.len() - 8..]);
    let name = safe_display_name(file_name.unwrap_or(&default_name), format);
    Ok(PendingArtifact {
        record: StoredDocument {
            id: id.clone(),
            name,
            format: format.to_owned(),
            size_bytes: 0,
            timestamp: Utc::now().to_rfc3339(),
            source_id,
        },
        path: files_root.join(format!("{id}.{format}")),
        metadata_root,
    })
}

fn finish_artifact<F>(
    mut pending: PendingArtifact,
    worker: F,
) -> Result<AgentDocumentArtifact, String>
where
    F: FnOnce() -> Result<Value, String>,
{
    let result = worker();
    if let Err(error) = result {
        let _ = fs::remove_file(&pending.path);
        return Err(error);
    }
    let metadata =
        fs::metadata(&pending.path).map_err(|error| format!("文档 worker 未生成产物：{error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        let _ = fs::remove_file(&pending.path);
        return Err("生成的文档不安全或超过大小限制".to_owned());
    }
    pending.record.size_bytes = metadata.len();
    if let Err(error) = write_record_new(&pending.metadata_root, &pending.record) {
        let _ = fs::remove_file(&pending.path);
        return Err(error);
    }
    Ok(artifact_from_record(pending.record))
}

fn run_worker(python: &Path, request: Value) -> Result<Value, String> {
    let mut command = Command::new(python);
    let development_worker = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../runtime/python/src/drpa_runner/document_worker.py");
    if development_worker.is_file() {
        command.arg(development_worker);
    } else {
        command.args(["-m", "drpa_runner.document_worker"]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动文档 worker：{error}"))?;

    let request_bytes =
        serde_json::to_vec(&request).map_err(|error| format!("文档请求序列化失败：{error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法连接文档 worker stdin".to_owned())?;
    stdin
        .write_all(&request_bytes)
        .map_err(|error| format!("无法发送文档请求：{error}"))?;
    drop(stdin);

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法连接文档 worker stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法连接文档 worker stderr".to_owned())?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_WORKER_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .take(MAX_WORKER_ERROR_BYTES + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("无法等待文档 worker：{error}"))?
        {
            break status;
        }
        if started.elapsed() >= WORKER_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("文档处理超时".to_owned());
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "文档 worker stdout 线程异常".to_owned())?
        .map_err(|error| format!("无法读取文档 worker stdout：{error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "文档 worker stderr 线程异常".to_owned())?
        .map_err(|error| format!("无法读取文档 worker stderr：{error}"))?;
    if stdout.len() as u64 > MAX_WORKER_RESPONSE_BYTES {
        return Err("文档 worker 返回内容过大".to_owned());
    }
    if stderr.len() as u64 > MAX_WORKER_ERROR_BYTES {
        return Err("文档 worker 错误内容过大".to_owned());
    }
    let response: WorkerResponse = serde_json::from_slice(&stdout).map_err(|error| {
        let diagnostic = String::from_utf8_lossy(&stderr);
        format!("文档 worker 返回无效 JSON：{error}；{diagnostic}")
    })?;
    if response.ok {
        return response
            .result
            .ok_or_else(|| "文档 worker 没有返回 result".to_owned());
    }
    let worker_error = response
        .error
        .map(|error| format!("{}：{}", error.code, error.message))
        .unwrap_or_else(|| "未知错误".to_owned());
    if status.success() {
        Err(format!("文档处理失败：{worker_error}"))
    } else {
        Err(worker_error)
    }
}

#[derive(Deserialize)]
struct WorkerResponse {
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<WorkerError>,
}

#[derive(Deserialize)]
struct WorkerError {
    code: String,
    message: String,
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    let valid = !session_id.is_empty()
        && session_id.len() <= 128
        && session_id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || ((byte == b'_' || byte == b'-') && index > 0)
        });
    if valid {
        Ok(())
    } else {
        Err("session_id 只能包含字母、数字、_ 和 -，并且必须以字母或数字开头".to_owned())
    }
}

fn validate_opaque_id(id: &str, kind: &str) -> Result<(), String> {
    let prefix = format!("{kind}_");
    let suffix = id
        .strip_prefix(&prefix)
        .ok_or_else(|| "无效的文档 ID".to_owned())?;
    if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("无效的文档 ID".to_owned());
    }
    Ok(())
}

fn validate_format(value: &str) -> Result<&'static str, String> {
    let normalized = value.trim().trim_start_matches('.').to_ascii_lowercase();
    if MACRO_FORMATS.contains(&normalized.as_str()) {
        return Err("不支持带宏的 Office 文档".to_owned());
    }
    SUPPORTED_FORMATS
        .iter()
        .copied()
        .find(|format| *format == normalized)
        .ok_or_else(|| "仅支持 PDF、DOCX、XLSX 和 PPTX 文档".to_owned())
}

fn format_from_path(path: &Path) -> Result<&'static str, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "文档缺少文件扩展名".to_owned())?;
    validate_format(extension)
}

fn inspect_ooxml(path: &Path, format: &str) -> Result<(), String> {
    if !matches!(format, "docx" | "xlsx" | "pptx") {
        return Ok(());
    }
    let file = File::open(path).map_err(|error| format!("无法检查 Office 文档：{error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("Office 文档不是有效的 OOXML：{error}"))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err("Office 文档包含过多文件".to_owned());
    }
    let mut total_uncompressed = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("无法检查 Office 文档内容：{error}"))?;
        let name = entry.name().replace('\\', "/");
        let member = Path::new(&name);
        if member.is_absolute()
            || member.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err("Office 文档包含不安全路径".to_owned());
        }
        let lower_name = name.to_ascii_lowercase();
        if lower_name.contains("vbaproject.bin") || lower_name.contains("/macrosheets/") {
            return Err("Office 文档包含宏内容".to_owned());
        }
        if lower_name.ends_with(".xml") || lower_name.ends_with(".rels") {
            let mut preview = Vec::new();
            Read::take(&mut entry, 1024 * 1024)
                .read_to_end(&mut preview)
                .map_err(|error| format!("无法检查 Office XML：{error}"))?;
            let preview = String::from_utf8_lossy(&preview).to_ascii_lowercase();
            if preview.contains("<!doctype") || preview.contains("<!entity") {
                return Err("Office 文档包含不安全 XML".to_owned());
            }
        }
        total_uncompressed = total_uncompressed.saturating_add(entry.size());
        if total_uncompressed > MAX_ZIP_UNCOMPRESSED_BYTES {
            return Err("Office 文档解压后过大".to_owned());
        }
        if entry.size() > 1_000_000
            && entry.compressed_size() > 0
            && entry.size() / entry.compressed_size() > MAX_ZIP_COMPRESSION_RATIO
        {
            return Err("Office 文档压缩率异常".to_owned());
        }
    }
    Ok(())
}

fn safe_display_name(value: &str, format: &str) -> String {
    let stem = Path::new(value)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let mut safe = stem
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
        .take(120)
        .collect::<String>();
    safe = safe.trim_matches([' ', '.']).to_owned();
    if safe.is_empty() || is_windows_reserved_name(&safe) {
        safe = "document".to_owned();
    }
    format!("{safe}.{format}")
}

fn is_windows_reserved_name(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit()
            && upper.as_bytes()[3] != b'0')
}

fn copy_new_limited(source: &Path, destination: &Path) -> Result<(), String> {
    let input = File::open(source).map_err(|error| format!("无法读取文档：{error}"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("无法创建文档：{error}"))?;
    let mut limited = Read::take(input, MAX_DOCUMENT_BYTES + 1);
    let copied = match std::io::copy(&mut limited, &mut output) {
        Ok(copied) => copied,
        Err(error) => {
            drop(output);
            let _ = fs::remove_file(destination);
            return Err(format!("无法复制文档：{error}"));
        }
    };
    if copied > MAX_DOCUMENT_BYTES {
        drop(output);
        let _ = fs::remove_file(destination);
        return Err("文档超过大小限制".to_owned());
    }
    output
        .flush()
        .map_err(|error| format!("无法写入文档：{error}"))
}

fn write_record_new(metadata_root: &Path, record: &StoredDocument) -> Result<(), String> {
    let path = metadata_root.join(format!("{}.json", record.id));
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|error| format!("无法编码文档元数据：{error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("无法创建文档元数据：{error}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.flush()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("无法写入文档元数据：{error}"));
    }
    Ok(())
}

fn read_record(metadata_root: &Path, id: &str) -> Result<StoredDocument, String> {
    read_record_path(&metadata_root.join(format!("{id}.json")))
}

fn read_record_path(path: &Path) -> Result<StoredDocument, String> {
    let bytes = fs::read(path).map_err(|_| "文档 ID 不存在".to_owned())?;
    serde_json::from_slice(&bytes).map_err(|error| format!("文档元数据已损坏：{error}"))
}

fn validate_stored_record(record: &StoredDocument, kind: &str) -> Result<(), String> {
    validate_opaque_id(&record.id, kind)?;
    let format = validate_format(&record.format)?;
    if format != record.format {
        return Err("文档元数据格式无效".to_owned());
    }
    Ok(())
}

fn attachment_from_record(record: StoredDocument) -> AgentDocumentAttachment {
    AgentDocumentAttachment {
        id: record.id,
        name: record.name,
        format: record.format,
        size_bytes: record.size_bytes,
        imported_at: record.timestamp,
    }
}

fn artifact_from_record(record: StoredDocument) -> AgentDocumentArtifact {
    AgentDocumentArtifact {
        id: record.id,
        name: record.name,
        format: record.format,
        size_bytes: record.size_bytes,
        created_at: record.timestamp,
        source_id: record.source_id,
    }
}

fn attachment_session_root(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join("agent")
        .join("attachments")
        .join(session_id)
}

fn artifact_session_root(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join("agent")
        .join("artifacts")
        .join(session_id)
}

fn artifact_file_path(
    workspace_root: &Path,
    session_id: &str,
    record: &StoredDocument,
) -> Result<PathBuf, String> {
    let session_root = existing_session_root(
        workspace_root,
        &artifact_session_root(workspace_root, session_id),
    )?;
    let path = session_root
        .join("files")
        .join(format!("{}.{}", record.id, record.format));
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| format!("文档产物文件已丢失：{}", record.name))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("文档产物不是普通文件".to_owned());
    }
    Ok(path)
}

fn prepare_session_root(workspace_root: &Path, session_root: &Path) -> Result<PathBuf, String> {
    let workspace = workspace_root
        .canonicalize()
        .map_err(|error| format!("无法解析工作区：{error}"))?;
    let relative = session_root
        .strip_prefix(workspace_root)
        .map_err(|_| "Agent 文档目录不在工作区内".to_owned())?;
    let mut current = workspace.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Agent 文档目录包含不安全路径".to_owned());
        };
        let candidate = current.join(name);
        match fs::create_dir(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("无法创建 Agent 文档目录：{error}")),
        }
        current = candidate
            .canonicalize()
            .map_err(|error| format!("无法解析 Agent 文档目录：{error}"))?;
        if !current.starts_with(&workspace) || !current.is_dir() {
            return Err("Agent 文档目录不能通过链接指向工作区外".to_owned());
        }
    }
    for child in ["files", "metadata"] {
        let candidate = current.join(child);
        match fs::create_dir(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("无法创建 Agent 文档目录：{error}")),
        }
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("无法解析 Agent 文档目录：{error}"))?;
        if !canonical.starts_with(&workspace) || !canonical.is_dir() {
            return Err("Agent 文档目录不能通过链接指向工作区外".to_owned());
        }
    }
    Ok(current)
}

fn existing_session_root(workspace_root: &Path, session_root: &Path) -> Result<PathBuf, String> {
    let workspace = workspace_root
        .canonicalize()
        .map_err(|error| format!("无法解析工作区：{error}"))?;
    let session = session_root
        .canonicalize()
        .map_err(|error| format!("Agent 文档会话不存在：{error}"))?;
    if !session.starts_with(&workspace) || !session.is_dir() {
        return Err("Agent 文档目录不能通过链接指向工作区外".to_owned());
    }
    for child in ["files", "metadata"] {
        let child_path = session
            .join(child)
            .canonicalize()
            .map_err(|error| format!("Agent 文档目录已损坏：{error}"))?;
        if !child_path.starts_with(&workspace) || !child_path.is_dir() {
            return Err("Agent 文档目录不能通过链接指向工作区外".to_owned());
        }
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("drpa-agent-documents-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn session_and_document_ids_cannot_escape_the_workspace() {
        assert!(validate_session_id("../escape").is_err());
        assert!(validate_session_id("safe-session_1").is_ok());
        assert!(validate_opaque_id("art_deadbeef", "art").is_err());
        assert!(validate_opaque_id("art_1234567890abcdef1234567890abcdef", "art").is_ok());
    }

    #[test]
    fn imports_with_an_opaque_id_and_does_not_overwrite_the_source() {
        let root = temporary_root();
        let source = root.join("input.pdf");
        fs::write(&source, b"%PDF-1.4\n").unwrap();

        let imported = import_agent_document_at(&root, "session-1", &source).unwrap();

        assert!(imported.id.starts_with("att_"));
        assert_eq!(imported.name, "input.pdf");
        assert_eq!(fs::read(&source).unwrap(), b"%PDF-1.4\n");
        let stored = attachment_session_root(&root, "session-1")
            .join("files")
            .join(format!("{}.pdf", imported.id));
        assert_eq!(fs::read(stored).unwrap(), b"%PDF-1.4\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn display_names_are_flat_and_macro_formats_are_denied() {
        assert_eq!(safe_display_name("../../report.docx", "pdf"), "report.pdf");
        assert_eq!(safe_display_name("CON.xlsx", "xlsx"), "document.xlsx");
        assert!(validate_format("xlsm").is_err());
        assert!(validate_format("pptx").is_ok());
    }
}
