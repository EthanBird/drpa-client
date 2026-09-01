use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use tauri::State;

use crate::{
    AppPaths, configure_linux_process_group, configure_python_module_command, hide_child_window,
    locate_runtime,
};

const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[tauri::command]
pub(crate) async fn parse_python_flow(
    source: String,
    source_name: Option<String>,
    paths: State<'_, AppPaths>,
) -> Result<Value, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("Python 源码超过 2 MiB，请拆分模块后再生成流程视图".to_owned());
    }
    let paths = paths.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let response = invoke_converter(
            &paths,
            json!({
                "operation": "python-to-flow",
                "source": source,
                "sourceName": source_name.unwrap_or_else(|| "main.py".to_owned()),
            }),
        )?;
        response
            .get("flow")
            .cloned()
            .ok_or_else(|| "Python Flow 转换器没有返回流程图".to_owned())
    })
    .await
    .map_err(|error| format!("Python Flow 转换任务异常：{error}"))?
}

#[tauri::command]
pub(crate) async fn render_python_flow(
    flow: Value,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    let paths = paths.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let response =
            invoke_converter(&paths, json!({"operation": "flow-to-python", "flow": flow}))?;
        response
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "Python Flow 转换器没有返回源码".to_owned())
    })
    .await
    .map_err(|error| format!("Python Flow 写回任务异常：{error}"))?
}

#[tauri::command]
pub(crate) async fn validate_python_flow(
    flow: Value,
    paths: State<'_, AppPaths>,
) -> Result<Value, String> {
    let paths = paths.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        invoke_converter(&paths, json!({"operation": "validate-flow", "flow": flow}))
    })
    .await
    .map_err(|error| format!("Python Flow 校验任务异常：{error}"))?
}

fn invoke_converter(paths: &AppPaths, request: Value) -> Result<Value, String> {
    let request = serde_json::to_vec(&request)
        .map_err(|error| format!("序列化 Python Flow 请求失败：{error}"))?;
    if request.len() > MAX_RESPONSE_BYTES {
        return Err("Python Flow 请求超过 16 MiB".to_owned());
    }

    let runtime = locate_runtime(paths)?;
    let mut command = Command::new(&runtime.python);
    configure_python_module_command(
        &mut command,
        "drpa_runner.python_flow",
        &runtime.package_overlay,
        runtime.python_path.as_deref(),
    );
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    configure_linux_process_group(&mut command);
    hide_child_window(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 Python Flow 转换器失败：{error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "Python Flow 转换器输入通道未建立".to_owned())?
        .write_all(&request)
        .map_err(|error| format!("写入 Python Flow 请求失败：{error}"))?;

    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 Python Flow 转换器失败：{error}"))?;
    if output.stdout.len() > MAX_RESPONSE_BYTES {
        return Err("Python Flow 转换结果超过 16 MiB，请拆分当前模块".to_owned());
    }
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("Python Flow 转换器退出码：{}", output.status)
        } else {
            format!("Python Flow 转换失败：{detail}")
        });
    }

    let response: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("解析 Python Flow 转换结果失败：{error}"))?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        let detail = response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("转换器返回了未说明的错误");
        return Err(detail.to_owned());
    }
    Ok(response)
}
