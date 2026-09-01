use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use drpa_host::HostState;
use drpa_install::{
    ComponentManifest, ComponentState, CoreFileState, InstallLayout, build_component_pack,
    current_platform, discover_installation, register_installation, resolve_browser,
    resolve_fixed_webview2, system_webview2_version, unregister_installation,
    verify_component_pack,
};
use drpa_kernel::{
    WorkspaceManager, execute_python_run, list_runtime_profiles, locate_runtime, runtime_status,
    select_runtime_profile,
};
use drpa_protocol::RuntimeEvent;

mod service;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Context {
    pub(crate) json: bool,
    pub(crate) install_root: Option<PathBuf>,
    pub(crate) data_root: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("错误：{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let context = parse_global_options(&mut args)?;
    let Some(command) = args.first().cloned() else {
        print_help();
        return Ok(());
    };
    args.remove(0);
    match command.as_str() {
        "help" | "--help" | "-h" => print_help(),
        "version" | "--version" | "-V" => println!("drpa {VERSION}"),
        "status" => status(&context)?,
        "doctor" => doctor(&context)?,
        "serve" => service::serve_stdio(&context, &args)?,
        "install" => install_command(&context, &args)?,
        "component" => component_command(&context, &args)?,
        "workspace" => workspace_command(&context, &args)?,
        "runtime" => runtime_command(&context, &args)?,
        "rpaz" => rpaz_command(&context, &args)?,
        _ => return Err(format!("未知命令：{command}。运行 `drpa help` 查看用法")),
    }
    Ok(())
}

fn parse_global_options(args: &mut Vec<String>) -> Result<Context, String> {
    let mut context = Context {
        json: false,
        install_root: None,
        data_root: None,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                context.json = true;
                args.remove(index);
            }
            "--install-root" => {
                if index + 1 >= args.len() {
                    return Err("--install-root 缺少路径".to_owned());
                }
                context.install_root = Some(PathBuf::from(args.remove(index + 1)));
                args.remove(index);
            }
            "--data-root" => {
                if index + 1 >= args.len() {
                    return Err("--data-root 缺少路径".to_owned());
                }
                context.data_root = Some(PathBuf::from(args.remove(index + 1)));
                args.remove(index);
            }
            _ => index += 1,
        }
    }
    Ok(context)
}

fn status(context: &Context) -> Result<(), String> {
    let layout = optional_layout(context)?;
    let data_root = resolve_data_root(context, layout.as_ref())?;
    let workspaces = WorkspaceManager::new(&data_root)?.list()?;
    let components = layout
        .as_ref()
        .map(InstallLayout::read_state)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let browser = resolve_browser(layout.as_ref()).map_err(|error| error.to_string())?;
    let fixed_webview =
        resolve_fixed_webview2(layout.as_ref()).map_err(|error| error.to_string())?;
    let system_webview = system_webview2_version();
    if context.json {
        print_json(&serde_json::json!({
            "version": VERSION,
            "platform": current_platform(),
            "installRoot": layout.as_ref().map(|item| item.root()),
            "dataRoot": data_root,
            "components": components,
            "workspaces": workspaces,
            "browser": browser.as_ref().map(|item| serde_json::json!({
                "source": item.source.display_name(),
                "executable": item.executable,
            })),
            "webview2": if let Some(path) = fixed_webview.as_ref() {
                serde_json::json!({ "source": "DRPA Fixed Version 组件", "path": path })
            } else if let Some(version) = system_webview.as_ref() {
                serde_json::json!({ "source": "系统 Evergreen Runtime", "version": version })
            } else {
                serde_json::Value::Null
            },
        }))?;
    } else {
        println!("DRPA Core {VERSION} · {}", current_platform());
        println!(
            "安装目录：{}",
            layout
                .as_ref()
                .map(|item| item.root().display().to_string())
                .unwrap_or_else(|| "未注册（仅 CLI 模式）".to_owned())
        );
        println!("数据目录：{}", data_root.display());
        println!("活动组件：{}", components.active.len());
        println!("工作区：{}", workspaces.len());
        println!(
            "浏览器：{}",
            browser
                .as_ref()
                .map(|item| format!(
                    "{} · {}",
                    item.source.display_name(),
                    item.executable.display()
                ))
                .unwrap_or_else(|| "未检测到 Chromium 兼容浏览器".to_owned())
        );
        #[cfg(windows)]
        println!(
            "WebView2：{}",
            fixed_webview
                .as_ref()
                .map(|path| format!("DRPA Fixed Version 组件 · {}", path.display()))
                .or_else(|| system_webview
                    .as_ref()
                    .map(|version| format!("系统 Evergreen Runtime · {version}")))
                .unwrap_or_else(|| "未检测到；桌面启动时将打开组件向导".to_owned())
        );
        #[cfg(not(windows))]
        println!("桌面内核：由活动 Desktop UI 组件提供；Core CLI 可独立运行");
    }
    Ok(())
}

fn doctor(context: &Context) -> Result<(), String> {
    let layout = optional_layout(context)?;
    let data_root = resolve_data_root(context, layout.as_ref())?;
    let manager = WorkspaceManager::new(&data_root)?;
    let workspace_root = manager.active_root()?;
    let legacy_roots = legacy_runtime_roots(layout.as_ref());
    let mut issues = Vec::new();
    if let Some(layout) = &layout {
        let state = layout.read_state().map_err(|error| error.to_string())?;
        for id in state.active.keys() {
            if let Err(error) = layout.verify_component(id) {
                issues.push(format!("组件 {id}：{error}"));
            }
        }
    } else {
        issues.push("没有安装定位信息；可继续以 --data-root 使用 CLI".to_owned());
    }
    let runtime = locate_runtime(layout.as_ref(), &data_root, &workspace_root, &legacy_roots);
    if let Err(error) = &runtime {
        issues.push(error.clone());
    }
    let result = serde_json::json!({
        "ok": issues.is_empty(),
        "issues": issues,
        "workspaceRoot": workspace_root,
        "runtime": runtime.ok().map(|item| serde_json::json!({
            "python": item.python,
            "browser": item.browser,
            "runtimeRoot": item.runtime_root,
        })),
    });
    if context.json {
        print_json(&result)?;
    } else {
        println!(
            "DRPA Doctor：{}",
            if result["ok"].as_bool() == Some(true) {
                "正常"
            } else {
                "需要处理"
            }
        );
        println!(
            "工作区：{}",
            result["workspaceRoot"].as_str().unwrap_or_default()
        );
        for issue in result["issues"].as_array().into_iter().flatten() {
            println!("- {}", issue.as_str().unwrap_or_default());
        }
    }
    if result["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err("诊断发现问题".to_owned())
    }
}

fn install_command(context: &Context, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("init") => {
            let root = args
                .get(1)
                .map(PathBuf::from)
                .or_else(|| context.install_root.clone())
                .ok_or_else(|| "用法：drpa install init <安装目录>".to_owned())?;
            let channel = option_value(args, "--channel").unwrap_or("stable");
            let layout =
                InstallLayout::initialize(root, channel).map_err(|error| error.to_string())?;
            let locator = register_installation(&layout).map_err(|error| error.to_string())?;
            if context.json {
                print_json(
                    &serde_json::json!({ "installRoot": layout.root(), "installId": layout.marker().install_id, "locator": locator }),
                )?;
            } else {
                println!("已初始化 DRPA 安装目录：{}", layout.root().display());
                println!("安装定位索引：{}", locator.display());
            }
        }
        Some("locate") => {
            let layout = require_layout(context)?;
            if context.json {
                print_json(
                    &serde_json::json!({ "root": layout.root(), "marker": layout.marker() }),
                )?;
            } else {
                println!("{}", layout.root().display());
            }
        }
        Some("unregister") => {
            let layout = require_layout(context)?;
            unregister_installation(&layout.marker().install_id)
                .map_err(|error| error.to_string())?;
            println!("已从用户级定位索引移除：{}", layout.root().display());
        }
        Some("reconcile-core") => {
            let manifest_path = args
                .get(1)
                .ok_or_else(|| "用法：drpa install reconcile-core <core-files.json>".to_owned())?;
            let manifest: CoreFileState = serde_json::from_slice(
                &fs::read(manifest_path)
                    .map_err(|error| format!("读取核心文件清单失败：{error}"))?,
            )
            .map_err(|error| format!("核心文件清单无效：{error}"))?;
            if manifest.schema != 1 {
                return Err(format!("不支持的核心文件清单 schema：{}", manifest.schema));
            }
            let removed = require_layout(context)?
                .reconcile_core_files(&manifest.files)
                .map_err(|error| error.to_string())?;
            if context.json {
                print_json(&serde_json::json!({ "removed": removed }))?;
            } else if removed.is_empty() {
                println!("核心文件已符合当前版本");
            } else {
                println!("已删除旧版核心文件：{}", removed.join(", "));
            }
        }
        _ => return Err("用法：drpa install <init|locate|unregister|reconcile-core>".to_owned()),
    }
    Ok(())
}

fn component_command(context: &Context, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("list") => {
            let layout = require_layout(context)?;
            let state = layout.read_state().map_err(|error| error.to_string())?;
            if context.json {
                print_json(&state)?;
            } else {
                print_components(&state);
            }
        }
        Some("install") => {
            let path = args
                .get(1)
                .ok_or_else(|| "用法：drpa component install <组件.drpac>".to_owned())?;
            let layout = require_layout(context)?;
            let manifest = layout
                .install_component(path)
                .map_err(|error| error.to_string())?;
            if context.json {
                print_json(&manifest)?;
            } else {
                println!(
                    "已安装并激活：{} {}",
                    manifest.display_name, manifest.version
                );
            }
        }
        Some("inspect") => {
            let path = args
                .get(1)
                .ok_or_else(|| "用法：drpa component inspect <组件.drpac>".to_owned())?;
            let manifest = verify_component_pack(path).map_err(|error| error.to_string())?;
            if context.json {
                print_json(&manifest)?;
            } else {
                let bytes = manifest.files.iter().map(|item| item.bytes).sum::<u64>();
                println!(
                    "组件包校验通过：{} {} · {} 个文件 · {} 字节",
                    manifest.id,
                    manifest.version,
                    manifest.files.len(),
                    bytes
                );
            }
        }
        Some("remove") => {
            let id = args
                .get(1)
                .ok_or_else(|| "用法：drpa component remove <组件ID> [--purge]".to_owned())?;
            require_layout(context)?
                .remove_component(id, args.iter().any(|arg| arg == "--purge"))
                .map_err(|error| error.to_string())?;
            println!("已移除组件：{id}");
        }
        Some("verify") => {
            let layout = require_layout(context)?;
            if let Some(id) = args.get(1) {
                let manifest = layout
                    .verify_component(id)
                    .map_err(|error| error.to_string())?;
                if context.json {
                    print_json(&manifest)?;
                } else {
                    println!("组件校验通过：{} {}", manifest.id, manifest.version);
                }
            } else {
                let state = layout.read_state().map_err(|error| error.to_string())?;
                for id in state.active.keys() {
                    layout
                        .verify_component(id)
                        .map_err(|error| error.to_string())?;
                    println!("组件校验通过：{id}");
                }
            }
        }
        Some("activate") => {
            let id = args
                .get(1)
                .ok_or_else(|| "用法：drpa component activate <组件ID> <版本>".to_owned())?;
            let version = args
                .get(2)
                .ok_or_else(|| "组件激活命令缺少版本".to_owned())?;
            let manifest = require_layout(context)?
                .activate_component(id, version)
                .map_err(|error| error.to_string())?;
            if context.json {
                print_json(&manifest)?;
            } else {
                println!("已切换组件：{} {}", manifest.id, manifest.version);
            }
        }
        Some("reconcile") => {
            let desired_path = args.get(1).ok_or_else(|| {
                "用法：drpa component reconcile <期望组件列表文件> --managed ID,ID".to_owned()
            })?;
            let desired = fs::read_to_string(desired_path)
                .map_err(|error| format!("读取期望组件列表失败：{error}"))?
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect::<BTreeSet<_>>();
            let managed = option_value(args, "--managed")
                .ok_or_else(|| "reconcile 命令缺少 --managed".to_owned())?
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect::<BTreeSet<_>>();
            if !desired.is_subset(&managed) {
                return Err("期望组件列表包含非托管组件".to_owned());
            }
            let removed = require_layout(context)?
                .reconcile_components(&desired, &managed)
                .map_err(|error| error.to_string())?;
            if context.json {
                print_json(&serde_json::json!({ "removed": removed }))?;
            } else if removed.is_empty() {
                println!("组件状态已符合安装选择");
            } else {
                println!("已移除未选组件：{}", removed.join(", "));
            }
        }
        Some("gc") => {
            let layout = require_layout(context)?;
            let keep = option_value(args, "--keep")
                .unwrap_or("2")
                .parse::<usize>()
                .map_err(|_| "--keep 必须是正整数".to_owned())?;
            let state = layout.read_state().map_err(|error| error.to_string())?;
            let ids = if args.iter().any(|item| item == "--all") {
                state.active.keys().cloned().collect::<Vec<_>>()
            } else {
                vec![
                    args.get(1)
                        .ok_or_else(|| {
                            "用法：drpa component gc <组件ID>|--all [--keep 2]".to_owned()
                        })?
                        .clone(),
                ]
            };
            let mut removed = BTreeMap::new();
            for id in ids {
                let versions = layout
                    .garbage_collect_versions(&id, keep)
                    .map_err(|error| error.to_string())?;
                if !versions.is_empty() {
                    removed.insert(id, versions);
                }
            }
            if context.json {
                print_json(&removed)?;
            } else if removed.is_empty() {
                println!("没有可清理的旧组件版本");
            } else {
                for (id, versions) in removed {
                    println!("{id}：已清理 {}", versions.join(", "));
                }
            }
        }
        Some("pack") => component_pack(context, args)?,
        _ => {
            return Err(
                "用法：drpa component <list|install|inspect|remove|verify|activate|reconcile|gc|pack>"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn component_pack(context: &Context, args: &[String]) -> Result<(), String> {
    let source = args.get(1).ok_or_else(|| {
        "用法：drpa component pack <源目录> <输出.drpac> --id ID --version VERSION --name NAME"
            .to_owned()
    })?;
    let output = args.get(2).ok_or_else(|| "组件包缺少输出路径".to_owned())?;
    let id = option_value(args, "--id").ok_or_else(|| "组件包缺少 --id".to_owned())?;
    let version =
        option_value(args, "--version").ok_or_else(|| "组件包缺少 --version".to_owned())?;
    let display_name = option_value(args, "--name").unwrap_or(id);
    let platform = option_value(args, "--platform").unwrap_or(current_platform());
    let provides = option_values(args, "--provide");
    let entrypoints = option_values(args, "--entry")
        .into_iter()
        .map(|value| {
            value
                .split_once('=')
                .map(|(key, path)| (key.to_owned(), path.to_owned()))
                .ok_or_else(|| format!("入口点格式应为 name=path：{value}"))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let manifest = build_component_pack(
        source,
        output,
        ComponentManifest {
            schema: 1,
            id: id.to_owned(),
            version: version.to_owned(),
            platform: platform.to_owned(),
            display_name: display_name.to_owned(),
            description: String::new(),
            provides: provides.into_iter().map(str::to_owned).collect(),
            requires: BTreeMap::new(),
            entrypoints,
            files: Vec::new(),
        },
    )
    .map_err(|error| error.to_string())?;
    if context.json {
        print_json(&manifest)?;
    } else {
        println!("组件包已生成：{output}（{} 个文件）", manifest.files.len());
    }
    Ok(())
}

fn workspace_command(context: &Context, args: &[String]) -> Result<(), String> {
    let layout = optional_layout(context)?;
    let manager = WorkspaceManager::new(resolve_data_root(context, layout.as_ref())?)?;
    match args.first().map(String::as_str) {
        Some("list") => {
            let items = manager.list()?;
            if context.json {
                print_json(&items)?;
            } else {
                for item in items {
                    println!(
                        "{} {}\t{}\t{}",
                        if item.active { "*" } else { " " },
                        item.id,
                        item.name,
                        item.path
                    );
                }
            }
        }
        Some("create") => {
            let name = args
                .get(1)
                .ok_or_else(|| "用法：drpa workspace create <名称>".to_owned())?;
            let item = manager.create(name)?;
            if context.json {
                print_json(&item)?;
            } else {
                println!("已创建工作区：{} ({})", item.name, item.id);
            }
        }
        Some("use") => {
            let id = args
                .get(1)
                .ok_or_else(|| "用法：drpa workspace use <工作区ID>".to_owned())?;
            let root = manager.activate(id)?;
            println!("当前工作区：{}", root.display());
        }
        _ => return Err("用法：drpa workspace <list|create|use>".to_owned()),
    }
    Ok(())
}

fn runtime_command(context: &Context, args: &[String]) -> Result<(), String> {
    let layout = optional_layout(context)?;
    let data_root = resolve_data_root(context, layout.as_ref())?;
    let manager = WorkspaceManager::new(&data_root)?;
    let workspace_root = manager.active_root()?;
    let legacy_roots = legacy_runtime_roots(layout.as_ref());
    match args.first().map(String::as_str) {
        Some("list") => {
            let profiles =
                list_runtime_profiles(layout.as_ref(), &data_root, &workspace_root, &legacy_roots)?;
            if context.json {
                print_json(&profiles)?;
            } else if profiles.is_empty() {
                println!("没有已安装的 Python Runtime Profile");
            } else {
                for profile in profiles {
                    println!(
                        "{} {} · Python {} · {}{}{}",
                        if profile.selected { "*" } else { " " },
                        profile.name,
                        profile.python_version,
                        if profile.ready {
                            "已就绪"
                        } else {
                            "待初始化"
                        },
                        if profile.in_use > 0 {
                            format!(" · {} 个活动租约", profile.in_use)
                        } else {
                            String::new()
                        },
                        format!(" · {}", profile.id)
                    );
                }
            }
        }
        Some("select") => {
            let profile_id = args
                .get(1)
                .ok_or_else(|| "用法：drpa runtime select <Profile ID>".to_owned())?;
            let profile = select_runtime_profile(
                layout.as_ref(),
                &data_root,
                &workspace_root,
                &legacy_roots,
                profile_id,
            )?;
            if context.json {
                print_json(&profile)?;
            } else {
                println!("已切换工作区默认运行时：{} · {}", profile.name, profile.id);
                println!("新任务立即生效，已运行任务继续使用原 Profile");
            }
        }
        Some("status") => {
            let status =
                runtime_status(layout.as_ref(), &data_root, &workspace_root, &legacy_roots);
            if context.json {
                print_json(&status)?;
            } else if status.ready {
                println!("运行时已就绪：{}", status.profile_id);
                println!("Python：{}", status.python);
            } else {
                return Err(status.source);
            }
        }
        _ => return Err("用法：drpa runtime <list|select|status>".to_owned()),
    }
    Ok(())
}

fn rpaz_command(context: &Context, args: &[String]) -> Result<(), String> {
    let layout = optional_layout(context)?;
    let data_root = resolve_data_root(context, layout.as_ref())?;
    let manager = WorkspaceManager::new(&data_root)?;
    let workspace_root = manager.active_root()?;
    let host = HostState::try_new(workspace_root.clone()).map_err(|error| error.to_string())?;
    match args.first().map(String::as_str) {
        Some("list") => {
            let packages = host.snapshot().packages;
            if context.json {
                print_json(&packages)?;
            } else {
                for package in packages {
                    println!("{}\t{}\t{}", package.id, package.version, package.name);
                    for profile in package.profiles {
                        println!("  - {}\t{}", profile.id, profile.name);
                    }
                }
            }
        }
        Some("install") => {
            let path = args
                .get(1)
                .ok_or_else(|| "用法：drpa rpaz install <包.rpaz>".to_owned())?;
            let package = host
                .install_package(Path::new(path))
                .map_err(|error| error.to_string())?;
            if context.json {
                print_json(&package)?;
            } else {
                println!("已安装 RPAZ 包：{} {}", package.name, package.version);
            }
        }
        Some("uninstall") => {
            let id = args
                .get(1)
                .ok_or_else(|| "用法：drpa rpaz uninstall <包ID>".to_owned())?;
            host.uninstall_package(id)
                .map_err(|error| error.to_string())?;
            println!("已卸载 RPAZ 包：{id}");
        }
        Some("run") => run_rpaz(
            context,
            args,
            layout.as_ref(),
            &data_root,
            &workspace_root,
            &host,
        )?,
        _ => return Err("用法：drpa rpaz <list|install|uninstall|run>".to_owned()),
    }
    Ok(())
}

fn run_rpaz(
    context: &Context,
    args: &[String],
    layout: Option<&InstallLayout>,
    data_root: &Path,
    workspace_root: &Path,
    host: &HostState,
) -> Result<(), String> {
    let package_id = args.get(1).ok_or_else(|| {
        "用法：drpa rpaz run <包ID> [--profile ID] [--params JSON|@文件]".to_owned()
    })?;
    let package = host
        .snapshot()
        .packages
        .into_iter()
        .find(|item| item.id == *package_id)
        .ok_or_else(|| format!("找不到 RPAZ 包：{package_id}"))?;
    let profile_id = option_value(args, "--profile")
        .map(str::to_owned)
        .or_else(|| package.profiles.first().map(|item| item.id.clone()))
        .ok_or_else(|| "RPAZ 包没有可运行的任务配置".to_owned())?;
    let parameters = parse_json_value(option_value(args, "--params").unwrap_or("{}"))?;
    let launch = host
        .prepare_run(package_id, &profile_id, &parameters)
        .map_err(|error| error.to_string())?;
    let runtime = locate_runtime(
        layout,
        data_root,
        workspace_root,
        &legacy_runtime_roots(layout),
    )?;
    execute_python_run(
        host,
        workspace_root,
        &runtime,
        &launch,
        &parameters,
        |event| {
            if context.json {
                println!(
                    "{}",
                    serde_json::to_string(event).unwrap_or_else(|_| "{}".to_owned())
                );
            } else {
                print_runtime_event(event);
            }
        },
    )?;
    if !context.json {
        println!("运行完成：{}", launch.run_id);
    }
    Ok(())
}

fn optional_layout(context: &Context) -> Result<Option<InstallLayout>, String> {
    match discover_installation(context.install_root.as_deref()) {
        Ok(layout) => Ok(Some(layout)),
        Err(drpa_install::InstallError::InstallationNotFound) if context.data_root.is_some() => {
            Ok(None)
        }
        Err(drpa_install::InstallError::InstallationNotFound) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn require_layout(context: &Context) -> Result<InstallLayout, String> {
    discover_installation(context.install_root.as_deref())
        .map_err(|error| format!("{error}。先运行 `drpa install init <目录>`"))
}

fn resolve_data_root(context: &Context, layout: Option<&InstallLayout>) -> Result<PathBuf, String> {
    if let Some(root) = &context.data_root {
        return Ok(root.clone());
    }
    if let Some(root) = env::var_os("DRPA_DATA_ROOT") {
        return Ok(PathBuf::from(root));
    }
    if let Some(layout) = layout {
        return Ok(layout.data_root());
    }
    Err("无法确定数据目录；请指定 --data-root 或先初始化安装目录".to_owned())
}

fn legacy_runtime_roots(layout: Option<&InstallLayout>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = env::var_os("DRPA_RUNTIME_ROOT") {
        roots.push(PathBuf::from(root));
    }
    if let Some(layout) = layout {
        roots.push(layout.root().join("runtime"));
    }
    if let Ok(executable) = env::current_exe() {
        if let Some(parent) = executable.parent() {
            roots.push(parent.join("runtime"));
        }
    }
    roots
}

fn parse_json_value(source: &str) -> Result<serde_json::Value, String> {
    let source = if let Some(path) = source.strip_prefix('@') {
        fs::read_to_string(path).map_err(|error| format!("读取参数文件失败：{error}"))?
    } else {
        source.to_owned()
    };
    serde_json::from_str(&source).map_err(|error| format!("参数 JSON 无效：{error}"))
}

fn option_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn option_values<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .collect()
}

fn print_components(state: &ComponentState) {
    if state.active.is_empty() {
        println!("没有活动组件");
        return;
    }
    for (id, item) in &state.active {
        println!("{id}\t{}\t{}", item.version, item.relative_root);
    }
}

fn print_runtime_event(event: &RuntimeEvent) {
    match event {
        RuntimeEvent::Ready { .. } => println!("运行时已就绪"),
        RuntimeEvent::Log { message, .. }
        | RuntimeEvent::Warning { message, .. }
        | RuntimeEvent::Error { message, .. } => println!("{message}"),
        RuntimeEvent::Progress { value, message, .. } => println!(
            "{:.0}% {}",
            value.clamp(0.0, 100.0),
            message.as_deref().unwrap_or_default()
        ),
        RuntimeEvent::Artifact { label, path, .. } => println!("产物：{label} · {path}"),
        RuntimeEvent::OpenDirectory { path, .. } => println!("输出目录：{path}"),
        RuntimeEvent::Completed { exit_code, .. } => println!("运行时结束，退出码 {exit_code}"),
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn print_help() {
    println!(
        r#"DRPA Core {VERSION} — 无 WebView、无注册表的最小运行内核

用法：drpa [--install-root PATH] [--data-root PATH] [--json] <命令>

  status                              查看安装、组件与工作区状态
  doctor                              校验组件并初始化/检查 Python 运行环境
  serve --stdio                       启动本地 JSON Lines Core 协议
  install init <目录> [--channel C]   初始化并登记安装目录
  install locate|unregister           输出或注销当前安装目录
  install reconcile-core <清单>       删除上个版本已移除的核心文件
  component list                      列出活动组件
  component install <文件.drpac>      安装并激活离线组件
  component inspect <文件.drpac>      不安装，流式校验组件包及全部哈希
  component remove <ID> [--purge]     停用并删除组件
  component verify [ID]               校验组件文件哈希
  component activate <ID> <版本>       切换/回滚到已安装版本
  component reconcile <文件> ...       按安装器期望状态移除未选组件
  component gc <ID>|--all              仅保留活动版本和一个回滚点
  component pack <源> <输出> ...      创建离线组件包
  workspace list|create|use           管理隔离工作区
  runtime list|select|status          查看和切换工作区 Python Profile
  rpaz list|install|uninstall|run      管理与运行 RPAZ 包

组件打包示例：
  drpa component pack runtime runtime.drpac --id org.drpa.python-runtime --version 2.1.1 --name Python运行环境 --provide runtime.python
  drpa component pack chrome chrome.drpac --id org.drpa.browser.chromium --version 1 --name Chromium --provide browser.chromium --entry browser=chrome.exe
"#
    );
}
