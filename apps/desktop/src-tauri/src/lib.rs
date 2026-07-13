use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use drpa_host::{HostState, RunLaunch};
use drpa_package::{PackageManifest, safe_relative_path, validate_package_id};
use drpa_protocol::{PackageSummary, RUNTIME_PROTOCOL_VERSION, RuntimeEvent, WorkspaceSnapshot};
use serde::Serialize;
use tauri::{Manager, State};
use zip::write::SimpleFileOptions;

struct AppPaths {
    workspace_root: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioProject {
    id: String,
    name: String,
    files: Vec<String>,
}

#[tauri::command]
fn get_workspace_snapshot(state: State<'_, HostState>) -> WorkspaceSnapshot {
    state.snapshot()
}

#[tauri::command]
fn install_package(
    archive_path: String,
    state: State<'_, HostState>,
) -> Result<PackageSummary, String> {
    state
        .install_package(Path::new(&archive_path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_run(
    package_id: String,
    profile_id: String,
    parameters: serde_json::Value,
    state: State<'_, HostState>,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    let launch = state
        .prepare_run(&package_id, &profile_id, &parameters)
        .map_err(|error| error.to_string())?;
    let run_id = launch.run_id.clone();
    if let Err(error) = execute_python_run(&state, &paths, &launch, &parameters) {
        state.fail_run(&run_id, error.clone());
        return Err(error);
    }
    Ok(run_id)
}

#[tauri::command]
fn cancel_run(run_id: String, state: State<'_, HostState>) -> Result<(), String> {
    state.cancel_run(&run_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn list_studio_projects(paths: State<'_, AppPaths>) -> Result<Vec<StudioProject>, String> {
    let projects_root = paths.workspace_root.join("projects");
    fs::create_dir_all(&projects_root).map_err(|error| error.to_string())?;
    let mut projects = Vec::new();
    for entry in fs::read_dir(projects_root)
        .map_err(|error| error.to_string())?
        .flatten()
        .filter(|entry| entry.path().is_dir())
    {
        let id = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = entry.path().join("manifest.yaml");
        let name = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|source| PackageManifest::from_yaml(&source).ok())
            .map_or_else(|| id.clone(), |manifest| manifest.name);
        let mut files = Vec::new();
        collect_files(&entry.path(), &entry.path(), &mut files)
            .map_err(|error| error.to_string())?;
        files.sort();
        projects.push(StudioProject { id, name, files });
    }
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(projects)
}

#[tauri::command]
fn create_studio_project(
    project_id: String,
    name: String,
    paths: State<'_, AppPaths>,
) -> Result<StudioProject, String> {
    validate_package_id(&project_id).map_err(|error| error.to_string())?;
    if name.trim().is_empty() {
        return Err("项目名称不能为空".to_owned());
    }
    let root = paths.workspace_root.join("projects").join(&project_id);
    if root.exists() {
        return Err("同 ID 项目已经存在".to_owned());
    }
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let yaml_name = serde_json::to_string(name.trim()).map_err(|error| error.to_string())?;
    let manifest = format!(
        "schema: 2\nid: {project_id}\nname: {yaml_name}\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\nruntime:\n  python: \"3.11.*\"\ncapabilities:\n  network:\n    allow: []\n  filesystem:\n    read: []\n    write: [\"$outputs\"]\nparameters: []\n"
    );
    fs::write(root.join("manifest.yaml"), manifest).map_err(|error| error.to_string())?;
    fs::write(
        root.join("main.py"),
        "def main(ctx):\n    ctx.log.info(\"任务开始\")\n    ctx.progress(100, \"任务完成\")\n",
    )
    .map_err(|error| error.to_string())?;
    Ok(StudioProject {
        id: project_id,
        name: name.trim().to_owned(),
        files: vec!["main.py".to_owned(), "manifest.yaml".to_owned()],
    })
}

#[tauri::command]
fn read_project_file(
    project_id: String,
    relative_path: String,
    paths: State<'_, AppPaths>,
) -> Result<String, String> {
    validate_package_id(&project_id).map_err(|error| error.to_string())?;
    let relative = safe_relative_path(&relative_path).map_err(|error| error.to_string())?;
    fs::read_to_string(
        paths
            .workspace_root
            .join("projects")
            .join(project_id)
            .join(relative),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn write_project_file(
    project_id: String,
    relative_path: String,
    content: String,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    validate_package_id(&project_id).map_err(|error| error.to_string())?;
    let relative = safe_relative_path(&relative_path).map_err(|error| error.to_string())?;
    let target = paths
        .workspace_root
        .join("projects")
        .join(project_id)
        .join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(target, content).map_err(|error| error.to_string())
}

#[tauri::command]
fn build_studio_project(project_id: String, paths: State<'_, AppPaths>) -> Result<String, String> {
    validate_package_id(&project_id).map_err(|error| error.to_string())?;
    let root = paths.workspace_root.join("projects").join(&project_id);
    let manifest_source = fs::read_to_string(root.join("manifest.yaml"))
        .map_err(|error| format!("无法读取 manifest.yaml：{error}"))?;
    let manifest =
        PackageManifest::from_yaml(&manifest_source).map_err(|error| error.to_string())?;
    let output_root = paths.workspace_root.join("build");
    fs::create_dir_all(&output_root).map_err(|error| error.to_string())?;
    let output = output_root.join(format!("{}-{}.rpaz", manifest.id, manifest.version));
    let file = File::create(&output).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files).map_err(|error| error.to_string())?;
    files.sort();
    for relative in files {
        let source = root.join(&relative);
        archive
            .start_file(relative.replace('\\', "/"), options)
            .map_err(|error| error.to_string())?;
        let mut content = Vec::new();
        File::open(source)
            .and_then(|mut file| file.read_to_end(&mut content))
            .map_err(|error| error.to_string())?;
        archive
            .write_all(&content)
            .map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    Ok(output.to_string_lossy().into_owned())
}

fn collect_files(root: &Path, current: &Path, output: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(current)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, output)?;
        } else if path.is_file() {
            if let Ok(relative) = path.strip_prefix(root) {
                output.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

struct RuntimeEnvironment {
    python: PathBuf,
    python_path: Option<PathBuf>,
    browser: Option<PathBuf>,
}

fn execute_python_run(
    state: &HostState,
    paths: &AppPaths,
    launch: &RunLaunch,
    parameters: &serde_json::Value,
) -> Result<(), String> {
    let runtime = locate_runtime(paths)?;
    fs::create_dir_all(&launch.output_dir).map_err(|error| error.to_string())?;
    let run_root = launch
        .output_dir
        .parent()
        .ok_or_else(|| "无效的运行输出目录".to_owned())?;
    fs::create_dir_all(run_root).map_err(|error| error.to_string())?;
    let request_path = run_root.join("request.json");
    let request = serde_json::json!({
        "protocol": RUNTIME_PROTOCOL_VERSION,
        "run_id": launch.run_id,
        "package_id": launch.package_id,
        "package_dir": launch.package_dir,
        "output_dir": launch.output_dir,
        "entrypoint": launch.entrypoint,
        "callable": launch.callable,
        "parameters": parameters,
    });
    let request_file = File::create(&request_path).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(request_file, &request).map_err(|error| error.to_string())?;

    let mut command = Command::new(&runtime.python);
    command
        .args(["-m", "drpa_runner.cli", "--request"])
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1");
    if let Some(python_path) = runtime.python_path {
        command.env("PYTHONPATH", python_path);
    }
    if let Some(browser) = runtime.browser {
        command.env("DRPA_BROWSER_PATH", browser);
    }
    hide_child_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动封装 Python：{error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 Python 标准输出".to_owned())?;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("读取运行时事件失败：{error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<RuntimeEvent>(&line) {
            Ok(event) => state.record_runtime_event(&launch.run_id, event),
            Err(error) => state.fail_run(
                &launch.run_id,
                format!("运行时返回了无效事件：{error} · {line}"),
            ),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 Python 进程失败：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if !stderr.is_empty() {
            return Err(format!("Python 进程异常退出：{stderr}"));
        }
    }
    Ok(())
}

fn locate_runtime(paths: &AppPaths) -> Result<RuntimeEnvironment, String> {
    if let Some(python) = std::env::var_os("DRPA_RUNTIME_PYTHON") {
        return Ok(RuntimeEnvironment {
            python: PathBuf::from(python),
            python_path: std::env::var_os("DRPA_RUNTIME_PYTHONPATH").map(PathBuf::from),
            browser: std::env::var_os("DRPA_BROWSER_PATH").map(PathBuf::from),
        });
    }

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut roots = Vec::new();
    if let Some(root) = std::env::var_os("DRPA_RUNTIME_ROOT") {
        roots.push(PathBuf::from(root));
    }
    if let Some(parent) = executable.parent() {
        roots.push(parent.join("runtime"));
        #[cfg(target_os = "macos")]
        if let Some(contents) = parent.parent() {
            roots.push(contents.join("Resources/runtime"));
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(app_image) = std::env::var_os("APPIMAGE")
        && let Some(parent) = Path::new(&app_image).parent()
    {
        roots.push(parent.join("runtime"));
    }

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let environment = paths.workspace_root.join("runtime-environment");
        let python = prepare_sealed_runtime(&root, &environment)?;
        return Ok(RuntimeEnvironment {
            python,
            python_path: None,
            browser: find_named_file(&root.join("browser"), &browser_names()),
        });
    }

    #[cfg(debug_assertions)]
    {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../runtime/python/src");
        if source.is_dir() {
            return Ok(RuntimeEnvironment {
                python: PathBuf::from(if cfg!(windows) { "python" } else { "python3" }),
                python_path: Some(source),
                browser: std::env::var_os("DRPA_BROWSER_PATH").map(PathBuf::from),
            });
        }
    }

    Err(format!(
        "未找到封装 Python 运行时。请把平台 runtime 放到应用同目录的 runtime 文件夹；工作区：{}",
        paths.workspace_root.display()
    ))
}

fn prepare_sealed_runtime(root: &Path, environment: &Path) -> Result<PathBuf, String> {
    let environment_python = environment.join(if cfg!(windows) {
        "environment/Scripts/python.exe"
    } else {
        "environment/bin/python"
    });
    let bundled = find_named_file(
        &root.join("python"),
        if cfg!(windows) {
            &["python.exe"]
        } else {
            &["python3.11", "python3"]
        },
    )
    .ok_or_else(|| "封装运行时缺少 CPython 可执行文件".to_owned())?;
    let bootstrap = root.join("bootstrap_runtime.py");
    if !bootstrap.is_file() {
        return Err("封装运行时缺少 bootstrap_runtime.py".to_owned());
    }
    let mut command = Command::new(bundled);
    command
        .arg(bootstrap)
        .arg("--environment")
        .arg(environment.join("environment"))
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_child_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法初始化离线 Python：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "离线 Python 初始化失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if environment_python.is_file() {
        Ok(environment_python)
    } else {
        Err("离线 Python 初始化完成但没有生成解释器".to_owned())
    }
}

fn browser_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["chrome.exe"]
    } else if cfg!(target_os = "macos") {
        vec!["Google Chrome for Testing"]
    } else {
        vec!["chrome"]
    }
}

fn find_named_file(root: &Path, names: &[&str]) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_named_file(&path, names) {
                return Some(found);
            }
        } else if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| names.contains(&name))
        {
            return Some(path);
        }
    }
    None
}

#[cfg(windows)]
fn hide_child_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_window(_command: &mut Command) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let workspace_root = app.path().app_local_data_dir()?.join("workspace");
            fs::create_dir_all(&workspace_root)?;
            app.manage(HostState::new(workspace_root.clone()));
            app.manage(AppPaths { workspace_root });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_workspace_snapshot,
            install_package,
            start_run,
            cancel_run,
            list_studio_projects,
            create_studio_project,
            read_project_file,
            write_project_file,
            build_studio_project
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DRPA Next desktop host");
}
