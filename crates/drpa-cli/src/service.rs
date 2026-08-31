use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use drpa_host::HostState;
use drpa_kernel::{
    WorkspaceManager, execute_python_run, list_runtime_profiles, locate_runtime, runtime_status,
    select_runtime_profile,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Context, VERSION, legacy_runtime_roots, optional_layout, resolve_data_root};

const CORE_PROTOCOL: u32 = 1;
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRequest {
    #[serde(default = "default_protocol")]
    protocol: u32,
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceResponse<'a> {
    protocol: u32,
    #[serde(rename = "type")]
    message_type: &'static str,
    id: &'a Value,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ServiceError>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceError {
    code: &'static str,
    message: String,
}

enum Control {
    Continue(Value),
    Shutdown(Value),
}

pub(crate) fn serve_stdio(context: &Context, args: &[String]) -> Result<(), String> {
    if !args.is_empty() && !args.iter().all(|arg| arg == "--stdio") {
        return Err("用法：drpa serve --stdio".to_owned());
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(
        context,
        BufReader::new(stdin.lock()),
        BufWriter::new(stdout.lock()),
    )
}

fn serve<R: BufRead, W: Write>(
    context: &Context,
    mut reader: R,
    mut writer: W,
) -> Result<(), String> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_REQUEST_BYTES {
            write_error(
                &mut writer,
                &Value::Null,
                "request_too_large",
                "Core 请求超过 4 MiB",
            )?;
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<ServiceRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                write_error(
                    &mut writer,
                    &Value::Null,
                    "invalid_json",
                    &format!("请求 JSON 无效：{error}"),
                )?;
                continue;
            }
        };
        if request.protocol != CORE_PROTOCOL {
            write_error(
                &mut writer,
                &request.id,
                "protocol_mismatch",
                &format!("Core 仅支持协议 {CORE_PROTOCOL}"),
            )?;
            continue;
        }
        match handle(context, &request, &mut writer) {
            Ok(Control::Continue(result)) => write_success(&mut writer, &request.id, result)?,
            Ok(Control::Shutdown(result)) => {
                write_success(&mut writer, &request.id, result)?;
                break;
            }
            Err(error) => write_error(&mut writer, &request.id, "request_failed", &error)?,
        }
    }
    Ok(())
}

fn handle<W: Write>(
    context: &Context,
    request: &ServiceRequest,
    writer: &mut W,
) -> Result<Control, String> {
    let result = match request.method.as_str() {
        "ping" => serde_json::json!({
            "version": VERSION,
            "protocol": CORE_PROTOCOL,
            "platform": drpa_install::current_platform(),
        }),
        "shutdown" => return Ok(Control::Shutdown(serde_json::json!({ "stopped": true }))),
        "status" => status_value(context)?,
        "component.list" => {
            let layout = super::require_layout(context)?;
            serde_json::to_value(layout.read_state().map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
        }
        "component.install" => {
            let path = required_string(&request.params, "path")?;
            let manifest = super::require_layout(context)?
                .install_component(path)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(manifest).map_err(|error| error.to_string())?
        }
        "component.remove" => {
            let id = required_string(&request.params, "id")?;
            let purge = request
                .params
                .get("purge")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            super::require_layout(context)?
                .remove_component(id, purge)
                .map_err(|error| error.to_string())?;
            serde_json::json!({ "removed": id, "purged": purge })
        }
        "workspace.list" => serde_json::to_value(workspace_manager(context)?.list()?)
            .map_err(|error| error.to_string())?,
        "workspace.create" => {
            let name = required_string(&request.params, "name")?;
            serde_json::to_value(workspace_manager(context)?.create(name)?)
                .map_err(|error| error.to_string())?
        }
        "workspace.use" => {
            let id = required_string(&request.params, "id")?;
            let root = workspace_manager(context)?.activate(id)?;
            serde_json::json!({ "id": id, "path": root })
        }
        "runtime.list" => {
            let layout = optional_layout(context)?;
            let data_root = resolve_data_root(context, layout.as_ref())?;
            let workspace_root = WorkspaceManager::new(&data_root)?.active_root()?;
            serde_json::to_value(list_runtime_profiles(
                layout.as_ref(),
                &data_root,
                &workspace_root,
                &legacy_runtime_roots(layout.as_ref()),
            )?)
            .map_err(|error| error.to_string())?
        }
        "runtime.select" => {
            let profile_id = required_string(&request.params, "profileId")?;
            let layout = optional_layout(context)?;
            let data_root = resolve_data_root(context, layout.as_ref())?;
            let workspace_root = WorkspaceManager::new(&data_root)?.active_root()?;
            serde_json::to_value(select_runtime_profile(
                layout.as_ref(),
                &data_root,
                &workspace_root,
                &legacy_runtime_roots(layout.as_ref()),
                profile_id,
            )?)
            .map_err(|error| error.to_string())?
        }
        "runtime.status" => {
            let layout = optional_layout(context)?;
            let data_root = resolve_data_root(context, layout.as_ref())?;
            let workspace_root = WorkspaceManager::new(&data_root)?.active_root()?;
            serde_json::to_value(runtime_status(
                layout.as_ref(),
                &data_root,
                &workspace_root,
                &legacy_runtime_roots(layout.as_ref()),
            ))
            .map_err(|error| error.to_string())?
        }
        "rpaz.list" => {
            let (_data_root, _workspace_root, host) = host(context)?;
            serde_json::to_value(host.snapshot().packages).map_err(|error| error.to_string())?
        }
        "rpaz.install" => {
            let path = required_string(&request.params, "path")?;
            let (_data_root, _workspace_root, host) = host(context)?;
            serde_json::to_value(
                host.install_package(Path::new(path))
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?
        }
        "rpaz.uninstall" => {
            let id = required_string(&request.params, "id")?;
            let (_data_root, _workspace_root, host) = host(context)?;
            host.uninstall_package(id)
                .map_err(|error| error.to_string())?;
            serde_json::json!({ "removed": id })
        }
        "rpaz.run" => run_rpaz(context, request, writer)?,
        method => return Err(format!("未知 Core 方法：{method}")),
    };
    Ok(Control::Continue(result))
}

fn status_value(context: &Context) -> Result<Value, String> {
    let layout = optional_layout(context)?;
    let data_root = resolve_data_root(context, layout.as_ref())?;
    let workspaces = WorkspaceManager::new(&data_root)?.list()?;
    let components = layout
        .as_ref()
        .map(|item| item.read_state())
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    Ok(serde_json::json!({
        "version": VERSION,
        "platform": drpa_install::current_platform(),
        "installRoot": layout.as_ref().map(|item| item.root()),
        "dataRoot": data_root,
        "components": components,
        "workspaces": workspaces,
    }))
}

fn workspace_manager(context: &Context) -> Result<WorkspaceManager, String> {
    let layout = optional_layout(context)?;
    WorkspaceManager::new(resolve_data_root(context, layout.as_ref())?)
}

fn host(context: &Context) -> Result<(std::path::PathBuf, std::path::PathBuf, HostState), String> {
    let layout = optional_layout(context)?;
    let data_root = resolve_data_root(context, layout.as_ref())?;
    let workspace_root = WorkspaceManager::new(&data_root)?.active_root()?;
    let host = HostState::try_new(workspace_root.clone()).map_err(|error| error.to_string())?;
    Ok((data_root, workspace_root, host))
}

fn run_rpaz<W: Write>(
    context: &Context,
    request: &ServiceRequest,
    writer: &mut W,
) -> Result<Value, String> {
    let package_id = required_string(&request.params, "packageId")?;
    let layout = optional_layout(context)?;
    let data_root = resolve_data_root(context, layout.as_ref())?;
    let workspace_root = WorkspaceManager::new(&data_root)?.active_root()?;
    let host = HostState::try_new(workspace_root.clone()).map_err(|error| error.to_string())?;
    let package = host
        .snapshot()
        .packages
        .into_iter()
        .find(|item| item.id == package_id)
        .ok_or_else(|| format!("找不到 RPAZ 包：{package_id}"))?;
    let profile_id = request
        .params
        .get("profileId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| package.profiles.first().map(|item| item.id.clone()))
        .ok_or_else(|| "RPAZ 包没有可运行的任务配置".to_owned())?;
    let parameters = request
        .params
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let launch = host
        .prepare_run(package_id, &profile_id, &parameters)
        .map_err(|error| error.to_string())?;
    let runtime = locate_runtime(
        layout.as_ref(),
        &data_root,
        &workspace_root,
        &legacy_runtime_roots(layout.as_ref()),
    )?;
    let request_id = request.id.clone();
    let mut write_error = None;
    execute_python_run(
        &host,
        &workspace_root,
        &runtime,
        &launch,
        &parameters,
        |event| {
            if write_error.is_some() {
                return;
            }
            let notification = serde_json::json!({
                "protocol": CORE_PROTOCOL,
                "type": "event",
                "requestId": request_id,
                "event": "rpaz.run.event",
                "payload": event,
            });
            if let Err(error) = write_line(writer, &notification) {
                write_error = Some(error);
            }
        },
    )?;
    if let Some(error) = write_error {
        return Err(error);
    }
    serde_json::to_value(
        host.get_run_detail(&launch.run_id)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn required_string<'a>(params: &'a Value, name: &str) -> Result<&'a str, String> {
    params
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("参数 {name} 必须是非空字符串"))
}

fn write_success(writer: &mut impl Write, id: &Value, result: Value) -> Result<(), String> {
    write_line(
        writer,
        &ServiceResponse {
            protocol: CORE_PROTOCOL,
            message_type: "response",
            id,
            ok: true,
            result: Some(result),
            error: None,
        },
    )
}

fn write_error(
    writer: &mut impl Write,
    id: &Value,
    code: &'static str,
    message: &str,
) -> Result<(), String> {
    write_line(
        writer,
        &ServiceResponse {
            protocol: CORE_PROTOCOL,
            message_type: "response",
            id,
            ok: false,
            result: None,
            error: Some(ServiceError {
                code,
                message: message.to_owned(),
            }),
        },
    )
}

fn write_line(writer: &mut impl Write, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, value).map_err(|error| error.to_string())?;
    writer.write_all(b"\n").map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())
}

const fn default_protocol() -> u32 {
    CORE_PROTOCOL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_multiple_requests_and_clean_shutdown() {
        let temporary = tempfile::TempDir::new().unwrap();
        let context = Context {
            json: false,
            install_root: None,
            data_root: Some(temporary.path().join("data")),
        };
        let input = concat!(
            "{\"protocol\":1,\"id\":1,\"method\":\"ping\"}\n",
            "{\"protocol\":1,\"id\":2,\"method\":\"status\"}\n",
            "{\"protocol\":1,\"id\":3,\"method\":\"shutdown\"}\n"
        );
        let mut output = Vec::new();
        serve(&context, input.as_bytes(), &mut output).unwrap();
        let responses = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0]["result"]["protocol"], CORE_PROTOCOL);
        assert_eq!(responses[1]["ok"], true);
        assert_eq!(responses[2]["result"]["stopped"], true);
    }
}
