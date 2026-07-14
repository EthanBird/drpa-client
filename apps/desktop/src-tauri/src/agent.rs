use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use drpa_package::{Entrypoint, PackageManifest, safe_relative_path, validate_package_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

const MAX_AGENT_ROUNDS: usize = 8;
const MAX_HISTORY_MESSAGES: usize = 40;
const MAX_MESSAGE_BYTES: usize = 100_000;
const MAX_TOOL_OUTPUT_BYTES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const PYTHON_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentTurnRequest {
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub project_id: String,
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentToolEvent {
    pub call_id: String,
    pub name: String,
    pub status: String,
    pub summary: String,
    pub output: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentTurnResult {
    pub message: String,
    pub tools: Vec<AgentToolEvent>,
    pub usage: AgentUsage,
    pub duration_ms: u64,
}

struct AgentContext {
    workspace_root: PathBuf,
    project_root: Option<PathBuf>,
    python: PathBuf,
}

struct ToolResult {
    output: Value,
    summary: String,
}

pub(crate) fn run_agent_turn(
    request: AgentTurnRequest,
    workspace_root: PathBuf,
    python: PathBuf,
) -> Result<AgentTurnResult, String> {
    validate_request(&request)?;
    let started = Instant::now();
    let endpoint = chat_completions_endpoint(&request.base_url)?;
    let project_root = if request.project_id.trim().is_empty() {
        None
    } else {
        validate_project_id(&request.project_id)?;
        let root = workspace_root.join("projects").join(&request.project_id);
        if !root.is_dir() {
            return Err(format!("Agent 项目不存在：{}", request.project_id));
        }
        Some(root)
    };
    let context = AgentContext {
        workspace_root,
        project_root,
        python,
    };

    let mut messages = Vec::new();
    messages.push(json!({
        "role": "system",
        "content": system_prompt(context.project_root.is_some()),
    }));
    let history_start = request.messages.len().saturating_sub(MAX_HISTORY_MESSAGES);
    for message in &request.messages[history_start..] {
        messages.push(json!({"role": message.role, "content": message.content}));
    }

    let tools = if context.project_root.is_some() {
        rpaz_tool_definitions()
    } else {
        Vec::new()
    };
    let mut events = Vec::new();
    let mut usage = AgentUsage::default();

    for _ in 0..MAX_AGENT_ROUNDS {
        let mut payload = json!({
            "model": request.model.trim(),
            "messages": messages,
        });
        if !tools.is_empty() {
            payload["tools"] = Value::Array(tools.clone());
            payload["tool_choice"] = Value::String("auto".to_owned());
        }
        let response = call_chat_completions(&endpoint, &request.api_key, &payload)?;
        accumulate_usage(&mut usage, response.get("usage"));
        let assistant = response
            .pointer("/choices/0/message")
            .cloned()
            .ok_or_else(|| "模型响应缺少 choices[0].message".to_owned())?;
        let tool_calls = assistant
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        messages.push(assistant.clone());

        if tool_calls.is_empty() {
            let message = message_content(assistant.get("content"));
            if message.trim().is_empty() {
                return Err("模型返回了空消息".to_owned());
            }
            return Ok(AgentTurnResult {
                message,
                tools: events,
                usage,
                duration_ms: elapsed_ms(started),
            });
        }

        for call in tool_calls {
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_owned();
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let arguments = parse_tool_arguments(call.pointer("/function/arguments"))?;
            let executed = execute_tool(&context, &name, &arguments);
            let (status, summary, output) = match executed {
                Ok(result) => ("completed".to_owned(), result.summary, result.output),
                Err(error) => (
                    "failed".to_owned(),
                    error.clone(),
                    json!({"ok": false, "error": error}),
                ),
            };
            let output_text = truncate_text(&output.to_string(), MAX_TOOL_OUTPUT_BYTES);
            events.push(AgentToolEvent {
                call_id: call_id.clone(),
                name: name.clone(),
                status,
                summary,
                output: output_text.clone(),
            });
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output_text,
            }));
        }
    }

    Err(format!(
        "Agent 工具调用超过 {MAX_AGENT_ROUNDS} 轮，请缩小任务范围后重试"
    ))
}

fn validate_request(request: &AgentTurnRequest) -> Result<(), String> {
    if request.model.trim().is_empty() || request.model.len() > 200 {
        return Err("请填写有效的模型名称".to_owned());
    }
    if request.messages.is_empty() {
        return Err("Agent 消息不能为空".to_owned());
    }
    for message in &request.messages {
        if !matches!(message.role.as_str(), "user" | "assistant") {
            return Err("Agent 历史只接受 user/assistant 消息".to_owned());
        }
        if message.content.len() > MAX_MESSAGE_BYTES {
            return Err("单条 Agent 消息过长".to_owned());
        }
    }
    Ok(())
}

pub(crate) fn chat_completions_endpoint(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.len() > 2048
        || trimmed.contains(char::is_whitespace)
        || !(trimmed.starts_with("https://") || trimmed.starts_with("http://"))
    {
        return Err("OpenAI 兼容 URL 必须是有效的 http(s) 地址".to_owned());
    }
    if trimmed.ends_with("/chat/completions") {
        Ok(trimmed.to_owned())
    } else if trimmed.ends_with("/v1") {
        Ok(format!("{trimmed}/chat/completions"))
    } else {
        Ok(format!("{trimmed}/v1/chat/completions"))
    }
}

fn call_chat_completions(endpoint: &str, api_key: &str, payload: &Value) -> Result<Value, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let http = ureq::Agent::new_with_config(config);
    let mut request = http
        .post(endpoint)
        .header("Accept", "application/json")
        .header("User-Agent", "DRPA-Next-Agent/0.2");
    if !api_key.trim().is_empty() {
        let authorization = format!("Bearer {}", api_key.trim());
        request = request.header("Authorization", &authorization);
    }
    let mut response = request
        .send_json(payload)
        .map_err(|error| format!("模型接口请求失败：{error}"))?;
    response
        .body_mut()
        .read_json::<Value>()
        .map_err(|error| format!("模型接口返回的 JSON 无效：{error}"))
}

fn message_content(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_tool_arguments(value: Option<&Value>) -> Result<Value, String> {
    match value {
        Some(Value::String(source)) if source.trim().is_empty() => Ok(json!({})),
        Some(Value::String(source)) => {
            serde_json::from_str(source).map_err(|error| format!("工具参数 JSON 无效：{error}"))
        }
        Some(Value::Object(_)) => Ok(value.cloned().unwrap_or_else(|| json!({}))),
        None | Some(Value::Null) => Ok(json!({})),
        _ => Err("工具参数必须是 JSON 对象".to_owned()),
    }
}

fn accumulate_usage(target: &mut AgentUsage, value: Option<&Value>) {
    let Some(value) = value else { return };
    target.prompt_tokens = target.prompt_tokens.saturating_add(
        value
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    target.completion_tokens = target.completion_tokens.saturating_add(
        value
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
}

fn system_prompt(has_project: bool) -> String {
    let context = if has_project {
        "当前已绑定一个开发工作室项目，可以使用 RPAZ 工具读取、修改、校验、构建项目，也可以运行限时 Python 辅助分析。"
    } else {
        "当前未绑定开发项目。先回答问题，并提示用户在右侧选择项目后再执行文件或 Python 工具。"
    };
    format!(
        "你是 DRPA Next 内置的轻量 RPAZ 开发 Agent。{context}\n\
         RPAZ 是根目录含 manifest.yaml 的 ZIP，当前 schema 为 2；Python 入口实现 main(ctx)，\
         参数来自 ctx.params，产物使用 ctx.output_file，进度使用 ctx.progress。\n\
         只处理 RPAZ 项目开发，不假装使用未提供的终端、浏览器或网络工具。\n\
         修改文件后应调用 rpaz_validate；需要交付归档时调用 rpaz_build。\n\
         回答使用简体中文，先给结论，再列出实际完成的文件与验证结果。"
    )
}

fn rpaz_tool_definitions() -> Vec<Value> {
    vec![
        tool_definition(
            "rpaz_list_files",
            "列出当前 RPAZ 开发项目的文件。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_read_file",
            "读取当前 RPAZ 项目中的 UTF-8 文本文件。",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_write_file",
            "创建或覆盖当前 RPAZ 项目中的 UTF-8 文本文件。",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_validate",
            "按 DRPA schema 2 校验 manifest.yaml 和入口文件。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_build",
            "校验并构建当前项目的 .rpaz 归档。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool_definition(
            "rpaz_python",
            "在当前项目目录用 DRPA 内置 Python 执行最多 30 秒的辅助代码，适合检查 JSON、生成模板或验证纯 Python 逻辑。",
            json!({"type":"object","properties":{"code":{"type":"string"}},"required":["code"],"additionalProperties":false}),
        ),
    ]
}

fn tool_definition(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

fn execute_tool(
    context: &AgentContext,
    name: &str,
    arguments: &Value,
) -> Result<ToolResult, String> {
    let project_root = context
        .project_root
        .as_deref()
        .ok_or_else(|| "尚未选择 RPAZ 开发项目".to_owned())?;
    match name {
        "rpaz_list_files" => {
            let mut files = Vec::new();
            collect_project_files(project_root, project_root, &mut files)?;
            files.sort();
            let count = files.len();
            Ok(ToolResult {
                output: json!({"ok": true, "files": files}),
                summary: format!("已列出 {count} 个项目文件"),
            })
        }
        "rpaz_read_file" => {
            let relative = argument_string(arguments, "path")?;
            let path = resolve_project_file(project_root, relative, true)?;
            let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
            if metadata.len() > MAX_FILE_BYTES {
                return Err(format!(
                    "文件超过 {} MiB 读取限制",
                    MAX_FILE_BYTES / 1024 / 1024
                ));
            }
            let content = fs::read_to_string(&path)
                .map_err(|error| format!("读取 {relative} 失败：{error}"))?;
            Ok(ToolResult {
                output: json!({"ok": true, "path": relative, "content": content}),
                summary: format!("已读取 {relative}（{} 字符）", content.chars().count()),
            })
        }
        "rpaz_write_file" => {
            let relative = argument_string(arguments, "path")?;
            let content = argument_string(arguments, "content")?;
            if content.len() as u64 > MAX_FILE_BYTES {
                return Err(format!(
                    "文件超过 {} MiB 写入限制",
                    MAX_FILE_BYTES / 1024 / 1024
                ));
            }
            let path = resolve_project_file(project_root, relative, false)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(&path, content.as_bytes())
                .map_err(|error| format!("写入 {relative} 失败：{error}"))?;
            Ok(ToolResult {
                output: json!({"ok": true, "path": relative, "bytes": content.len()}),
                summary: format!("已写入 {relative}（{} 字节）", content.len()),
            })
        }
        "rpaz_validate" => validate_project(project_root),
        "rpaz_build" => build_project(context, project_root),
        "rpaz_python" => run_python(context, project_root, argument_string(arguments, "code")?),
        _ => Err(format!("未知 RPAZ 工具：{name}")),
    }
}

fn argument_string<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("工具参数缺少字符串字段 {name}"))
}

fn resolve_project_file(root: &Path, value: &str, must_exist: bool) -> Result<PathBuf, String> {
    let relative = safe_relative_path(value).map_err(|error| error.to_string())?;
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let target = root.join(relative);
    let checked = if target.exists() {
        fs::canonicalize(&target).map_err(|error| error.to_string())?
    } else {
        let parent = target
            .parent()
            .ok_or_else(|| "文件路径缺少父目录".to_owned())?;
        let existing_parent = nearest_existing_parent(parent)?;
        let canonical_parent =
            fs::canonicalize(existing_parent).map_err(|error| error.to_string())?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err("文件路径超出当前 RPAZ 项目".to_owned());
        }
        target.clone()
    };
    if !checked.starts_with(&canonical_root) {
        return Err("文件路径超出当前 RPAZ 项目".to_owned());
    }
    if must_exist && !checked.is_file() {
        return Err(format!("项目文件不存在：{value}"));
    }
    Ok(target)
}

fn nearest_existing_parent(mut path: &Path) -> Result<&Path, String> {
    while !path.exists() {
        path = path.parent().ok_or_else(|| "文件路径无效".to_owned())?;
    }
    Ok(path)
}

fn validate_project(project_root: &Path) -> Result<ToolResult, String> {
    let source = fs::read_to_string(project_root.join("manifest.yaml"))
        .map_err(|error| format!("读取 manifest.yaml 失败：{error}"))?;
    let manifest = PackageManifest::from_yaml(&source).map_err(|error| error.to_string())?;
    let entrypoint = match &manifest.entrypoint {
        Entrypoint::Python { module, callable } => {
            if !project_root.join(module).is_file() {
                return Err(format!("入口文件不存在：{module}"));
            }
            format!("{module}:{callable}")
        }
        Entrypoint::Command { executable, .. } => {
            if !project_root.join(executable).is_file() {
                return Err(format!("入口文件不存在：{executable}"));
            }
            executable.clone()
        }
    };
    Ok(ToolResult {
        output: json!({
            "ok": true,
            "id": manifest.id,
            "name": manifest.name,
            "version": manifest.version,
            "entrypoint": entrypoint,
            "parameters": manifest.parameters.len(),
        }),
        summary: format!(
            "manifest.yaml 校验通过：{} {}",
            manifest.id, manifest.version
        ),
    })
}

fn build_project(context: &AgentContext, project_root: &Path) -> Result<ToolResult, String> {
    let validation = validate_project(project_root)?;
    let source = fs::read_to_string(project_root.join("manifest.yaml"))
        .map_err(|error| error.to_string())?;
    let manifest = PackageManifest::from_yaml(&source).map_err(|error| error.to_string())?;
    let output_root = context.workspace_root.join("build");
    fs::create_dir_all(&output_root).map_err(|error| error.to_string())?;
    let output = output_root.join(format!("{}-{}.rpaz", manifest.id, manifest.version));
    let file = File::create(&output).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut files = Vec::new();
    collect_project_files(project_root, project_root, &mut files)?;
    files.sort();
    for relative in &files {
        archive
            .start_file(relative, options)
            .map_err(|error| error.to_string())?;
        let mut content = Vec::new();
        File::open(project_root.join(relative))
            .and_then(|mut file| file.read_to_end(&mut content))
            .map_err(|error| error.to_string())?;
        archive
            .write_all(&content)
            .map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    Ok(ToolResult {
        output: json!({
            "ok": true,
            "path": output.to_string_lossy(),
            "files": files.len(),
            "validation": validation.output,
        }),
        summary: format!("已构建 {}（{} 个文件）", output.display(), files.len()),
    })
}

fn collect_project_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), ".git" | "__pycache__" | ".pytest_cache") {
            continue;
        }
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_project_files(root, &path, output)?;
        } else if kind.is_file() && !name.ends_with(".pyc") {
            output.push(
                path.strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        if output.len() > 4096 {
            return Err("项目文件超过 4096 个".to_owned());
        }
    }
    Ok(())
}

fn run_python(
    context: &AgentContext,
    project_root: &Path,
    code: &str,
) -> Result<ToolResult, String> {
    if code.len() > 50_000 {
        return Err("Python 辅助代码超过 50000 字符".to_owned());
    }
    let temporary_root = context.workspace_root.join("agent").join("tmp");
    fs::create_dir_all(&temporary_root).map_err(|error| error.to_string())?;
    let id = Uuid::new_v4().simple().to_string();
    let stdout_path = temporary_root.join(format!("{id}.stdout"));
    let stderr_path = temporary_root.join(format!("{id}.stderr"));
    let stdout = File::create(&stdout_path).map_err(|error| error.to_string())?;
    let stderr = File::create(&stderr_path).map_err(|error| error.to_string())?;
    let mut command = Command::new(&context.python);
    command
        .args(["-I", "-c", code])
        .current_dir(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    hide_child_window(&mut command);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动内置 Python 失败：{error}"))?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= PYTHON_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            cleanup_file(&stdout_path);
            cleanup_file(&stderr_path);
            return Err("Python 辅助代码执行超过 30 秒".to_owned());
        }
        thread::sleep(Duration::from_millis(40));
    };
    let stdout = read_limited(&stdout_path, MAX_TOOL_OUTPUT_BYTES);
    let stderr = read_limited(&stderr_path, MAX_TOOL_OUTPUT_BYTES);
    cleanup_file(&stdout_path);
    cleanup_file(&stderr_path);
    let exit_code = status.code().unwrap_or(-1);
    let output = json!({
        "ok": status.success(),
        "exitCode": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "durationMs": elapsed_ms(started),
    });
    if status.success() {
        Ok(ToolResult {
            output,
            summary: format!("Python 执行完成，退出码 {exit_code}"),
        })
    } else {
        Err(format!("Python 执行失败，退出码 {exit_code}：{stderr}"))
    }
}

fn read_limited(path: &Path, limit: usize) -> String {
    match fs::read(path) {
        Ok(content) => truncate_text(&String::from_utf8_lossy(&content), limit),
        Err(error) => format!("读取输出失败：{error}"),
    }
}

fn cleanup_file(path: &Path) {
    let _ = fs::remove_file(path);
}

fn truncate_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…输出已截断…", &value[..end])
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn validate_project_id(value: &str) -> Result<(), String> {
    let generated = value
        .strip_prefix("project-")
        .is_some_and(|hash| hash.len() == 24 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if generated || validate_package_id(value).is_ok() {
        Ok(())
    } else {
        Err("开发项目 ID 无效".to_owned())
    }
}

#[cfg(windows)]
fn hide_child_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_child_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_openai_compatible_endpoints() {
        assert_eq!(
            chat_completions_endpoint("http://localhost:11434/v1").unwrap(),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("https://gateway.example/api").unwrap(),
            "https://gateway.example/api/v1/chat/completions"
        );
        assert!(chat_completions_endpoint("file:///tmp/model").is_err());
    }

    #[test]
    fn rpaz_tools_reject_parent_traversal_and_validate_a_project() {
        let workspace = std::env::temp_dir().join(format!("drpa-agent-{}", Uuid::new_v4()));
        let project = workspace
            .join("projects")
            .join("project-000000000000000000000001");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("manifest.yaml"),
            "schema: 2\nid: com.example.agent\nname: Agent Test\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\nruntime:\n  python: '3.11.*'\n",
        )
        .unwrap();
        fs::write(project.join("main.py"), "def main(ctx):\n    pass\n").unwrap();
        let context = AgentContext {
            workspace_root: workspace.clone(),
            project_root: Some(project.clone()),
            python: PathBuf::from("python"),
        };

        assert!(execute_tool(&context, "rpaz_validate", &json!({})).is_ok());
        assert!(
            execute_tool(
                &context,
                "rpaz_read_file",
                &json!({"path": "../outside.txt"})
            )
            .is_err()
        );
        fs::remove_dir_all(workspace).unwrap();
    }
}
