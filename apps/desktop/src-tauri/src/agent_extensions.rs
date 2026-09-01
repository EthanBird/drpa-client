use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use drpa_package::safe_relative_path;
use flate2::read::GzDecoder;
use rquickjs::{Context, Promise, Runtime, Value as JsValue, function::Func};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tar::Archive;
use uuid::Uuid;

const EXTENSION_SCHEMA: u16 = 1;
const MANIFEST_FILE: &str = "extension.json";
const MAX_EXTENSION_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EXTENSION_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_EXTENSION_FILES: usize = 1_024;
const QUICKJS_MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const QUICKJS_STACK_LIMIT: usize = 512 * 1024;
const QUICKJS_TIMEOUT: Duration = Duration::from_secs(5);

const QUICKJS_BOOTSTRAP: &str = r#"
globalThis.__drpa_tool_registry = new Map();
globalThis.pi = Object.freeze({
  registerTool(spec) {
    if (!spec || typeof spec !== "object") throw new Error("registerTool requires a tool object");
    const name = String(spec.name || "").trim();
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(name)) throw new Error("invalid extension tool name: " + name);
    if (typeof spec.execute !== "function") throw new Error("tool.execute must be a function");
    if (globalThis.__drpa_tool_registry.has(name)) throw new Error("duplicate extension tool: " + name);
    globalThis.__drpa_tool_registry.set(name, spec);
  },
  hostcall(name, args = {}) {
    const raw = globalThis.__drpa_hostcall(String(name), JSON.stringify(args ?? {}));
    const value = JSON.parse(raw);
    if (value && value.__drpaHostError) throw new Error(String(value.__drpaHostError));
    return value;
  },
  tool(name, args = {}) {
    return Promise.resolve(globalThis.pi.hostcall(name, args));
  },
  log(...values) {
    return values.map((value) => typeof value === "string" ? value : JSON.stringify(value)).join(" ");
  }
});
globalThis.__drpa_call_tool = async function(name, callId, args, ctx) {
  const spec = globalThis.__drpa_tool_registry.get(name);
  if (!spec) throw new Error("extension tool not found: " + name);
  const signal = Object.freeze({ aborted: false });
  const onUpdate = function() {};
  return await spec.execute(callId, args ?? {}, signal, onUpdate, ctx ?? {});
};
"#;

const BUNDLED_EXAMPLE_SOURCE: &str = r#"
export default function (pi) {
  pi.registerTool({
    name: "project_file_overview",
    label: "项目文件概览",
    description: "通过 DRPA hostcall 快速统计当前项目匹配的文件。",
    parameters: {
      type: "object",
      properties: {
        pattern: { type: "string", description: "文件 glob，默认 **/*" }
      },
      additionalProperties: false
    },
    execute(_callId, input) {
      const result = pi.hostcall("find_files", {
        pattern: input && input.pattern ? String(input.pattern) : "**/*",
        limit: 200
      });
      const files = Array.isArray(result.files) ? result.files : [];
      return {
        content: [{ type: "text", text: `找到 ${files.length} 个项目文件` }],
        details: result,
        isError: false
      };
    }
  });
}
"#;

pub(crate) type ExtensionHostcall =
    Arc<dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentExtensionTool {
    pub name: String,
    pub label: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentExtensionManifest {
    schema: u16,
    id: String,
    name: String,
    version: String,
    description: String,
    enabled: bool,
    runtime: String,
    source: String,
    entry: String,
    integrity: String,
    tools: Vec<AgentExtensionTool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentExtensionToolSummary {
    pub name: String,
    pub exposed_name: String,
    pub label: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentExtensionSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub runtime: String,
    pub source: String,
    pub integrity: String,
    pub directory: String,
    pub tools: Vec<AgentExtensionToolSummary>,
}

#[derive(Debug)]
pub(crate) struct AgentExtensionExecution {
    pub output: Value,
    pub summary: String,
}

pub(crate) fn list_extensions(workspace_root: &Path) -> Result<Vec<AgentExtensionSummary>, String> {
    ensure_bundled_example(workspace_root)?;
    let root = extensions_root(workspace_root);
    let mut extensions = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|error| error.to_string())?
        .flatten()
    {
        if !entry.path().is_dir() || entry.file_name().to_string_lossy().starts_with(".install-") {
            continue;
        }
        let manifest = match read_manifest(&entry.path()) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        extensions.push(summary_from_manifest(&entry.path(), &manifest));
    }
    extensions.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    Ok(extensions)
}

pub(crate) fn tool_definitions(workspace_root: &Path) -> Result<Vec<Value>, String> {
    let mut definitions = Vec::new();
    for extension in list_extensions(workspace_root)? {
        if !extension.enabled {
            continue;
        }
        for tool in extension.tools {
            definitions.push(json!({
                "type": "function",
                "function": {
                    "name": tool.exposed_name,
                    "description": format!("扩展“{}”：{}", extension.name, tool.description),
                    "parameters": tool.parameters
                }
            }));
        }
    }
    Ok(definitions)
}

pub(crate) fn execute_tool(
    workspace_root: &Path,
    exposed_name: &str,
    call_id: &str,
    arguments: &Value,
    context_payload: Value,
    hostcall: ExtensionHostcall,
) -> Option<Result<AgentExtensionExecution, String>> {
    if !exposed_name.starts_with("ext__") {
        return None;
    }
    Some(execute_tool_inner(
        workspace_root,
        exposed_name,
        call_id,
        arguments,
        context_payload,
        hostcall,
    ))
}

fn execute_tool_inner(
    workspace_root: &Path,
    exposed_name: &str,
    call_id: &str,
    arguments: &Value,
    context_payload: Value,
    hostcall: ExtensionHostcall,
) -> Result<AgentExtensionExecution, String> {
    let extension = list_extensions(workspace_root)?
        .into_iter()
        .find_map(|extension| {
            if !extension.enabled {
                return None;
            }
            extension
                .tools
                .iter()
                .find(|tool| tool.exposed_name == exposed_name)
                .cloned()
                .map(|tool| (extension, tool))
        })
        .ok_or_else(|| format!("扩展工具不存在或已停用：{exposed_name}"))?;
    let (summary, tool) = extension;
    let root = extensions_root(workspace_root).join(&summary.id);
    let manifest = read_manifest(&root)?;
    let entry = resolve_entry(&root, &manifest.entry)?;
    let source = read_extension_source(&entry)?;
    let output = run_quickjs_tool(
        &source,
        &tool.name,
        call_id,
        arguments,
        &context_payload,
        hostcall,
    )?;
    Ok(AgentExtensionExecution {
        output,
        summary: format!("扩展 {} 已执行 {}", summary.name, tool.label),
    })
}

pub(crate) fn install_extension(
    workspace_root: &Path,
    package_path: &str,
) -> Result<AgentExtensionSummary, String> {
    let source =
        fs::canonicalize(package_path).map_err(|error| format!("定位扩展包失败：{error}"))?;
    if !source.is_file() {
        return Err("请选择单个 JS/MJS 文件或 npm .tgz 离线包".to_owned());
    }
    let metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_EXTENSION_ARCHIVE_BYTES {
        return Err("扩展包超过 32 MiB 限制".to_owned());
    }
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "js" | "mjs" => install_javascript_file(workspace_root, &source),
        "tgz" => install_npm_archive(workspace_root, &source),
        "ts" => Err(
            "当前离线 QuickJS 运行时不直接编译 TypeScript；请导入无外部依赖的单文件 JS/MJS bundle"
                .to_owned(),
        ),
        _ => Err("只支持 .js、.mjs 或 npm .tgz 离线扩展包".to_owned()),
    }
}

pub(crate) fn set_extension_enabled(
    workspace_root: &Path,
    extension_id: &str,
    enabled: bool,
) -> Result<AgentExtensionSummary, String> {
    validate_extension_id(extension_id)?;
    let root = extensions_root(workspace_root).join(extension_id);
    let mut manifest = read_manifest(&root)?;
    manifest.enabled = enabled;
    write_manifest(&root, &manifest)?;
    Ok(summary_from_manifest(&root, &manifest))
}

pub(crate) fn remove_extension(workspace_root: &Path, extension_id: &str) -> Result<(), String> {
    validate_extension_id(extension_id)?;
    if extension_id == "drpa-quickjs-example" {
        return Err("内置 QuickJS 示例不能卸载，只能停用".to_owned());
    }
    let root = extensions_root(workspace_root);
    let target = root.join(extension_id);
    if !target.is_dir() {
        return Err("扩展不存在".to_owned());
    }
    let canonical_root = fs::canonicalize(&root).map_err(|error| error.to_string())?;
    let canonical_target = fs::canonicalize(&target).map_err(|error| error.to_string())?;
    if canonical_target.parent() != Some(canonical_root.as_path()) {
        return Err("扩展目录超出允许范围".to_owned());
    }
    fs::remove_dir_all(canonical_target).map_err(|error| format!("卸载扩展失败：{error}"))
}

fn install_javascript_file(
    workspace_root: &Path,
    source_path: &Path,
) -> Result<AgentExtensionSummary, String> {
    let source = read_extension_source(source_path)?;
    let tools = collect_registered_tools(&source)?;
    if tools.is_empty() {
        return Err("扩展没有通过 pi.registerTool 注册任何工具".to_owned());
    }
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("local-extension");
    let id = normalized_extension_id(stem);
    let root = extensions_root(workspace_root);
    let target = root.join(&id);
    if target.exists() {
        return Err(format!("扩展已存在：{id}"));
    }
    let stage = root.join(format!(".install-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&stage).map_err(|error| error.to_string())?;
    let result = (|| {
        fs::write(stage.join("index.js"), source.as_bytes()).map_err(|error| error.to_string())?;
        let manifest = AgentExtensionManifest {
            schema: EXTENSION_SCHEMA,
            id: id.clone(),
            name: stem.to_owned(),
            version: "local".to_owned(),
            description: "本地导入的 Pi 风格 QuickJS 工具扩展".to_owned(),
            enabled: true,
            runtime: "QuickJS".to_owned(),
            source: "local-js".to_owned(),
            entry: "index.js".to_owned(),
            integrity: sha256_bytes(source.as_bytes()),
            tools,
        };
        write_manifest(&stage, &manifest)?;
        fs::rename(&stage, &target).map_err(|error| format!("提交扩展安装失败：{error}"))?;
        Ok(summary_from_manifest(&target, &manifest))
    })();
    if result.is_err() && stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn install_npm_archive(
    workspace_root: &Path,
    archive_path: &Path,
) -> Result<AgentExtensionSummary, String> {
    let root = extensions_root(workspace_root);
    let stage = root.join(format!(".install-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&stage).map_err(|error| error.to_string())?;
    let result = (|| {
        unpack_npm_tgz(archive_path, &stage)?;
        let package_source = fs::read_to_string(stage.join("package.json"))
            .map_err(|error| format!("npm 包缺少 package.json：{error}"))?;
        let package: Value = serde_json::from_str(&package_source)
            .map_err(|error| format!("package.json 无效：{error}"))?;
        let package_name = package
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "package.json 缺少 name".to_owned())?;
        let version = package
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("local");
        let description = package
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("离线 npm Pi 扩展");
        let entry = package
            .pointer("/pi/extensions/0")
            .and_then(Value::as_str)
            .ok_or_else(|| "package.json 缺少 pi.extensions[0]".to_owned())?
            .trim_start_matches("./");
        if entry.to_ascii_lowercase().ends_with(".ts") {
            return Err(
                "此 npm 扩展入口是 TypeScript，且可能依赖 Node/Pi peer；请先离线打包为无外部依赖的单文件 JS。pi-subagents 属于此类，不能安全地用空 shim 假装兼容"
                    .to_owned(),
            );
        }
        let entry_path = resolve_entry(&stage, entry)?;
        let source = read_extension_source(&entry_path)?;
        let tools = collect_registered_tools(&source)?;
        if tools.is_empty() {
            return Err("扩展没有通过 pi.registerTool 注册任何工具".to_owned());
        }
        let id = normalized_extension_id(package_name);
        let target = root.join(&id);
        if target.exists() {
            return Err(format!("扩展已存在：{id}"));
        }
        let archive_bytes = fs::read(archive_path).map_err(|error| error.to_string())?;
        let manifest = AgentExtensionManifest {
            schema: EXTENSION_SCHEMA,
            id: id.clone(),
            name: package_name.to_owned(),
            version: version.to_owned(),
            description: description.to_owned(),
            enabled: true,
            runtime: "QuickJS".to_owned(),
            source: "npm-tgz-offline".to_owned(),
            entry: entry.replace('\\', "/"),
            integrity: sha256_bytes(&archive_bytes),
            tools,
        };
        write_manifest(&stage, &manifest)?;
        fs::rename(&stage, &target).map_err(|error| format!("提交扩展安装失败：{error}"))?;
        Ok(summary_from_manifest(&target, &manifest))
    })();
    if result.is_err() && stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn unpack_npm_tgz(archive_path: &Path, target: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = Archive::new(GzDecoder::new(file));
    let mut files = 0usize;
    let mut total = 0u64;
    for item in archive.entries().map_err(|error| error.to_string())? {
        let mut item = item.map_err(|error| error.to_string())?;
        let kind = item.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err("npm 扩展包包含不支持的链接或特殊文件".to_owned());
        }
        let original = item.path().map_err(|error| error.to_string())?;
        let mut components = original.components();
        let first = components.next();
        let relative: PathBuf = if matches!(first, Some(Component::Normal(value)) if value == "package")
        {
            components.collect()
        } else {
            original.into_owned()
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let relative =
            safe_relative_path(&relative.to_string_lossy()).map_err(|error| error.to_string())?;
        let output = target.join(relative);
        if kind.is_dir() {
            fs::create_dir_all(&output).map_err(|error| error.to_string())?;
            continue;
        }
        files = files.saturating_add(1);
        if files > MAX_EXTENSION_FILES {
            return Err("npm 扩展包文件数量超过 1024".to_owned());
        }
        let size = item.header().size().map_err(|error| error.to_string())?;
        total = total.saturating_add(size);
        if total > MAX_EXTENSION_ARCHIVE_BYTES {
            return Err("npm 扩展包解压内容超过 32 MiB".to_owned());
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut bytes = Vec::with_capacity(size.min(1024 * 1024) as usize);
        item.read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        fs::write(output, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn ensure_bundled_example(workspace_root: &Path) -> Result<(), String> {
    let root = extensions_root(workspace_root);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let target = root.join("drpa-quickjs-example");
    if target.join(MANIFEST_FILE).is_file() {
        return Ok(());
    }
    if target.exists() {
        return Err("内置 QuickJS 示例目录不完整，请删除后重试".to_owned());
    }
    fs::create_dir_all(&target).map_err(|error| error.to_string())?;
    fs::write(target.join("index.js"), BUNDLED_EXAMPLE_SOURCE)
        .map_err(|error| error.to_string())?;
    let tools = collect_registered_tools(BUNDLED_EXAMPLE_SOURCE)?;
    let manifest = AgentExtensionManifest {
        schema: EXTENSION_SCHEMA,
        id: "drpa-quickjs-example".to_owned(),
        name: "DRPA QuickJS Hostcall 示例".to_owned(),
        version: "1.0.0".to_owned(),
        description: "无 Node 依赖的 Pi 风格扩展，用于验证 registerTool 与受控 hostcall。"
            .to_owned(),
        enabled: true,
        runtime: "QuickJS".to_owned(),
        source: "bundled".to_owned(),
        entry: "index.js".to_owned(),
        integrity: sha256_bytes(BUNDLED_EXAMPLE_SOURCE.as_bytes()),
        tools,
    };
    write_manifest(&target, &manifest)
}

fn collect_registered_tools(source: &str) -> Result<Vec<AgentExtensionTool>, String> {
    let source = normalize_extension_source(source)?;
    let runtime = configured_runtime()?;
    let context = Context::full(&runtime).map_err(js_error)?;
    context.with(|ctx| {
        ctx.globals()
            .set(
                "__drpa_hostcall",
                Func::from(|_name: String, _arguments: String| {
                    json!({"__drpaHostError":"安装预检期间不能执行 hostcall"}).to_string()
                }),
            )
            .map_err(js_error)?;
        ctx.eval::<(), _>(QUICKJS_BOOTSTRAP).map_err(js_error)?;
        ctx.eval::<(), _>(source.as_str()).map_err(js_error)?;
        let init: Promise = ctx
            .eval(
                "Promise.resolve(typeof globalThis.__drpa_extension_default === 'function' ? globalThis.__drpa_extension_default(globalThis.pi) : undefined)",
            )
            .map_err(js_error)?;
        let _: JsValue = init.finish().map_err(js_error)?;
        let serialized: String = ctx
            .eval(
                "JSON.stringify(Array.from(globalThis.__drpa_tool_registry.values()).map((tool) => ({ name: String(tool.name), label: String(tool.label || tool.name), description: String(tool.description || ''), parameters: tool.parameters || { type: 'object', properties: {}, additionalProperties: false } })))",
            )
            .map_err(js_error)?;
        let tools: Vec<AgentExtensionTool> = serde_json::from_str(&serialized)
            .map_err(|error| format!("扩展工具定义无效：{error}"))?;
        let mut names = HashSet::new();
        for tool in &tools {
            if !valid_tool_name(&tool.name) || !names.insert(tool.name.as_str()) {
                return Err(format!("扩展工具名称无效或重复：{}", tool.name));
            }
            if !tool.parameters.is_object() {
                return Err(format!("扩展工具 {} 的 parameters 必须是 JSON Schema 对象", tool.name));
            }
        }
        Ok(tools)
    })
}

fn run_quickjs_tool(
    source: &str,
    tool_name: &str,
    call_id: &str,
    arguments: &Value,
    context_payload: &Value,
    hostcall: ExtensionHostcall,
) -> Result<Value, String> {
    let source = normalize_extension_source(source)?;
    let runtime = configured_runtime()?;
    let context = Context::full(&runtime).map_err(js_error)?;
    context.with(|ctx| {
        let callback = Arc::clone(&hostcall);
        ctx.globals()
            .set(
                "__drpa_hostcall",
                Func::from(move |name: String, arguments_json: String| {
                    let result = serde_json::from_str::<Value>(&arguments_json)
                        .map_err(|error| format!("hostcall 参数不是 JSON：{error}"))
                        .and_then(|arguments| callback(&name, &arguments));
                    match result {
                        Ok(value) => value.to_string(),
                        Err(error) => json!({"__drpaHostError":error}).to_string(),
                    }
                }),
            )
            .map_err(js_error)?;
        ctx.eval::<(), _>(QUICKJS_BOOTSTRAP).map_err(js_error)?;
        ctx.eval::<(), _>(source.as_str()).map_err(js_error)?;
        let init: Promise = ctx
            .eval(
                "Promise.resolve(typeof globalThis.__drpa_extension_default === 'function' ? globalThis.__drpa_extension_default(globalThis.pi) : undefined)",
            )
            .map_err(js_error)?;
        let _: JsValue = init.finish().map_err(js_error)?;
        let invocation = format!(
            "Promise.resolve(globalThis.__drpa_call_tool({}, {}, {}, {})).then((value) => JSON.stringify(value))",
            serde_json::to_string(tool_name).map_err(|error| error.to_string())?,
            serde_json::to_string(call_id).map_err(|error| error.to_string())?,
            arguments,
            context_payload,
        );
        let promise: Promise = ctx.eval(invocation).map_err(js_error)?;
        let serialized: String = promise.finish().map_err(js_error)?;
        serde_json::from_str(&serialized)
            .map_err(|error| format!("扩展工具返回值不是有效 JSON：{error}"))
    })
}

fn configured_runtime() -> Result<Runtime, String> {
    let runtime = Runtime::new().map_err(js_error)?;
    runtime.set_memory_limit(QUICKJS_MEMORY_LIMIT);
    runtime.set_max_stack_size(QUICKJS_STACK_LIMIT);
    let started = Instant::now();
    runtime.set_interrupt_handler(Some(Box::new(move || started.elapsed() > QUICKJS_TIMEOUT)));
    Ok(runtime)
}

fn normalize_extension_source(source: &str) -> Result<String, String> {
    if source.len() as u64 > MAX_EXTENSION_SOURCE_BYTES {
        return Err("扩展入口超过 2 MiB 限制".to_owned());
    }
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("import ")
            || (trimmed.starts_with("export {") && trimmed.contains(" from "))
        {
            return Err(
                "QuickJS 离线扩展必须是无外部 import 的单文件 bundle；检测到模块依赖".to_owned(),
            );
        }
    }
    if source.contains("export default") {
        Ok(source.replacen("export default", "globalThis.__drpa_extension_default =", 1))
    } else {
        Ok(source.to_owned())
    }
}

fn extensions_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("agent").join("extensions")
}

fn resolve_entry(root: &Path, entry: &str) -> Result<PathBuf, String> {
    let relative = safe_relative_path(entry).map_err(|error| error.to_string())?;
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let target = fs::canonicalize(root.join(relative))
        .map_err(|error| format!("定位扩展入口失败：{error}"))?;
    if !target.starts_with(&canonical_root) || !target.is_file() {
        return Err("扩展入口超出扩展目录或不是普通文件".to_owned());
    }
    Ok(target)
}

fn read_extension_source(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_EXTENSION_SOURCE_BYTES {
        return Err("扩展入口超过 2 MiB 限制".to_owned());
    }
    fs::read_to_string(path).map_err(|error| format!("扩展入口必须是 UTF-8：{error}"))
}

fn read_manifest(root: &Path) -> Result<AgentExtensionManifest, String> {
    let source = fs::read_to_string(root.join(MANIFEST_FILE))
        .map_err(|error| format!("读取扩展清单失败：{error}"))?;
    let manifest: AgentExtensionManifest =
        serde_json::from_str(&source).map_err(|error| format!("扩展清单无效：{error}"))?;
    validate_manifest(&manifest)?;
    if root.file_name().and_then(|value| value.to_str()) != Some(manifest.id.as_str()) {
        return Err("扩展目录与清单 ID 不一致".to_owned());
    }
    Ok(manifest)
}

fn write_manifest(root: &Path, manifest: &AgentExtensionManifest) -> Result<(), String> {
    validate_manifest(manifest)?;
    let serialized = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    fs::write(root.join(MANIFEST_FILE), format!("{serialized}\n"))
        .map_err(|error| format!("写入扩展清单失败：{error}"))
}

fn validate_manifest(manifest: &AgentExtensionManifest) -> Result<(), String> {
    if manifest.schema != EXTENSION_SCHEMA {
        return Err(format!("不支持的扩展 schema：{}", manifest.schema));
    }
    validate_extension_id(&manifest.id)?;
    if manifest.name.trim().is_empty()
        || manifest.version.trim().is_empty()
        || manifest.runtime != "QuickJS"
    {
        return Err("扩展名称、版本或运行时无效".to_owned());
    }
    let mut names = HashSet::new();
    for tool in &manifest.tools {
        if !valid_tool_name(&tool.name) || !names.insert(tool.name.as_str()) {
            return Err(format!("扩展工具名称无效或重复：{}", tool.name));
        }
    }
    let _ = safe_relative_path(&manifest.entry).map_err(|error| error.to_string())?;
    Ok(())
}

fn summary_from_manifest(root: &Path, manifest: &AgentExtensionManifest) -> AgentExtensionSummary {
    AgentExtensionSummary {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        enabled: manifest.enabled,
        runtime: manifest.runtime.clone(),
        source: manifest.source.clone(),
        integrity: manifest.integrity.clone(),
        directory: root.to_string_lossy().into_owned(),
        tools: manifest
            .tools
            .iter()
            .map(|tool| AgentExtensionToolSummary {
                name: tool.name.clone(),
                exposed_name: exposed_tool_name(&manifest.id, &tool.name),
                label: tool.label.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            })
            .collect(),
    }
}

fn normalized_extension_id(value: &str) -> String {
    let normalized = value
        .trim_start_matches('@')
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let normalized = normalized
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if normalized.is_empty() {
        "local-extension".to_owned()
    } else {
        normalized.chars().take(64).collect()
    }
}

fn validate_extension_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("扩展 ID 只能包含小写字母、数字和中划线".to_owned());
    }
    Ok(())
}

fn valid_tool_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn exposed_tool_name(extension_id: &str, tool_name: &str) -> String {
    let base = format!(
        "ext__{}__{}",
        extension_id.replace('-', "_"),
        tool_name.replace('-', "_")
    );
    if base.len() <= 64 {
        return base;
    }
    let digest = sha256_bytes(base.as_bytes());
    format!("{}__{}", &base[..54], &digest[..8])
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn js_error(error: rquickjs::Error) -> String {
    format!("QuickJS：{error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("drpa-extension-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn quickjs_extension_registers_and_calls_a_host_tool() {
        let tools = collect_registered_tools(BUNDLED_EXAMPLE_SOURCE).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "project_file_overview");
        let output = run_quickjs_tool(
            BUNDLED_EXAMPLE_SOURCE,
            "project_file_overview",
            "call-1",
            &json!({"pattern":"**/*.rs"}),
            &json!({"cwd":"project"}),
            Arc::new(|name, arguments| {
                assert_eq!(name, "find_files");
                assert_eq!(arguments["pattern"], "**/*.rs");
                Ok(json!({"ok":true,"files":["src/main.rs"]}))
            }),
        )
        .unwrap();
        assert_eq!(output["details"]["files"][0], "src/main.rs");
    }

    #[test]
    fn typescript_npm_extension_is_rejected_with_an_actionable_message() {
        let root = temporary_root();
        let extension_root = root.join("agent/extensions");
        fs::create_dir_all(&extension_root).unwrap();
        let error = normalize_extension_source(
            "import type { ExtensionAPI } from '@earendil-works/pi';\nexport default function(pi: ExtensionAPI) {}",
        )
        .unwrap_err();
        assert!(error.contains("单文件 bundle"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exposed_names_are_bounded_and_stable() {
        let name = exposed_tool_name(
            "a-very-long-extension-identifier-that-needs-to-be-shortened",
            "a_very_long_tool_name_that_also_needs_to_be_shortened",
        );
        assert!(name.len() <= 64);
        assert_eq!(
            name,
            exposed_tool_name(
                "a-very-long-extension-identifier-that-needs-to-be-shortened",
                "a_very_long_tool_name_that_also_needs_to_be_shortened"
            )
        );
    }
}
