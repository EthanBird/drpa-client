# DRPA Next

DRPA Next 是一个本地优先、面向 Windows 与 Linux 的可扩展代码包运行管理器。用户无需配置系统 Python，即可安装、开发、运行和观察 `.rpaz` 自动化脚本包。

当前版本为 `1.0.0`，正式提供 Windows x64 Setup，以及 Linux x86_64 AppImage、现代发行版 deb 与 UOS Desktop 20 专用 deb。原有 PySide6 客户端已冻结，只作为 `.rpaz` v1 行为和迁移参考；新功能只进入 Tauri + React + Rust 架构。

## 当前能力

- 中文默认界面，支持正常移动、缩放、最大化、最小化和关闭窗口。
- 引导式、无注册表写入的 Windows NSIS 安装器；应用和用户数据均可放在非系统盘安装目录。
- 内置 CPython 3.11、完整离线 wheels、Chrome for Testing、Fixed Version WebView2 和真实 Jupyter Kernel 依赖。
- 安装、拖拽导入、运行、取消和卸载 `.rpaz`；包操作集中在右键菜单。
- 工作室可新建项目、编辑源码和 Notebook、直接运行工作副本、导出 `.rpaz`，也可将已安装包复制为可编辑项目。
- 基于 `ipykernel`、`jupyter_client`、`pyzmq`、`nbformat` 的真实 Jupyter 执行链路。
- 基础设施内置轻量 RPAZ AI Agent：可配置 OpenAI-compatible URL、model 与会话级可选 key，通过六个项目工具和三个知识库工具完成读取、写入、校验、构建和 sealed Python 辅助；支持本地会话列表、逐会话项目绑定、重命名/删除及配置面板折叠。
- 内置本地 Markdown 知识库：支持目录树、阅读/编辑/分栏渲染、相对文档跳转、内联新建/重命名/删除、拖拽导入、原生导入导出与自动保存，并首次初始化多篇详细 RPAZ 开发指南。
- 协议化 `.drpa-update` 差量更新，包含可视化进度、结构/大小检查、精确基线匹配、独立 Worker、失败回滚和新 Host 启动确认；日常更新不重复携带 WebView2/Chrome，并始终保护安装目录下的 `data/`。

正式版本下载：<https://github.com/EthanBird/drpa-client/releases/tag/desktop-v1.0.0>

> `1.0.0` 是当前推荐的全量安装基线。`0.2.x` 及更早安装缺少 protocol-2 安装库存，升级时直接运行最新 Setup；完成一次全量安装后，后续版本使用轻量 `.drpa-update`，不会再次打包 WebView2、未变化的 Chrome/runtime 或用户数据。

## 平台状态

| 平台 | 源码开发 | CI | 正式发行 |
| --- | --- | --- | --- |
| Windows x64 | 完整支持 | 前端、Rust、Python、runtime、安装器 | `1.0.0` 全量安装包与轻量更新 |
| Linux x86_64 | Host、Python/Jupyter、XDG 与平台 UI 已适配 | Ubuntu 22.04 构建；Ubuntu 22.04 与 Debian 10/glibc 2.28 安装、离线运行和 X11 门禁 | `1.0.0` AppImage、现代 deb 与 UOS 20 专用 deb |
| macOS | Rust core 与桌面 Host 编译检查 | 编译检查 | 尚未发布 |

Linux x86_64 已把 sealed CPython 3.11/Jupyter/Chrome 作为只读 Tauri resource 放入 AppImage 与 deb，Host 通过 `resource_dir` 定位，生成环境与用户数据写入 XDG 目录。现代 deb `1.0.0-2` 把 WebKitGTK 4.1、JavaScriptCoreGTK、GTK、GStreamer 和 helper process 私有安装到 `/opt/drpa-next`，适用于 glibc 2.35+。`drpa-next-1.0.0-linux-x86_64-uos20.deb` 面向 UOS Desktop 20 Professional（eagle）/glibc 2.28：固定安装到 `/opt/drpa-next-uos20`，额外内置 glibc 2.35 动态加载器、libstdc++ 与 libgcc，递归补齐 X11、音频、字体等非驱动 ELF 依赖，并固定所有应用 ELF 的解释器和传递型 RPATH；CI 在 Debian 10/glibc 2.28 容器中完成安装、离线 Python/Jupyter 原生扩展、Chrome、X11、卸载与数据保留测试。两种 deb 都不依赖系统 `libwebkit2gtk-4.1-0`；EGL/GL/GBM/DRM 仍由目标机提供以匹配显卡驱动。Linux 暂不支持 `.drpa-update`。接手 Linux 端请从 [Linux 开发与移植交接](docs/LINUX_DEVELOPMENT.md) 开始。

## 架构边界

```text
React / TypeScript UI
        │ typed Tauri invoke
        ▼
Rust Host ── package / runtime / run / update policy
        │ versioned JSONL + Jupyter wire protocol + Agent tool broker
        ▼
Sealed Python 3.11 ── RPAZ worker / IPython kernel / Chrome
```

- `apps/desktop/`：Tauri 2 桌面壳、React UI、窗口和更新器。
- `crates/drpa-package/`：manifest、归档和路径安全规则。
- `crates/drpa-host/`：包、运行记录和 Host 领域服务。
- `crates/drpa-protocol/`：前后端与运行时共享 DTO/事件协议。
- `runtime/python/`：RPAZ Python adapter、Runtime Context 和 Jupyter bridge。
- `offline/`：离线运行时规范、精确依赖锁和引导脚本。
- `installer/windows/`：无注册表 NSIS 安装器。
- `tools/windows/`：Windows 文件级更新包生成器。

UI 不是安全边界。所有文件路径、包清单、更新清单和运行请求都必须由 Rust Host 再次验证。

## 本地开发

要求 Node.js 24+ 和 Rust stable。桌面联调还需要目标平台的 Tauri 2 系统依赖；Python/RPA 联调建议使用 CPython 3.11.9。前端浏览器预览使用确定性的 mock gateway，不需要启动 Rust Host：

```bash
npm ci
npm run dev
npm run typecheck
npm run test
npm run build
```

桌面 Host 联调：

```bash
npm run tauri:dev
```

Linux **源码编译和 `tauri:dev`** 需要先安装 WebKitGTK 4.1 开发包，并为开发 Host 设置 `DRPA_DATA_DIR`、`DRPA_RUNTIME_PYTHON` 和 `DRPA_RUNTIME_PYTHONPATH`；正式 AppImage 与两种 deb 已携带桌面运行库。UOS 20 用户必须选择文件名带 `uos20` 的 deb。完整命令、平台边界和发布验收见 [`docs/LINUX_DEVELOPMENT.md`](docs/LINUX_DEVELOPMENT.md)。

Rust 与 Python 验证：

```bash
cargo fmt --all --check
cargo clippy -p drpa-protocol -p drpa-package -p drpa-host --all-targets -- -D warnings
cargo test -p drpa-protocol -p drpa-package -p drpa-host
python -m pip install -e "./runtime/python[test]"
python -m pytest -q runtime/python/tests
python tools/offline/validate_requirements.py offline/requirements/runtime.txt
```

正式 Windows 安装包必须由 GitHub Actions 的原生 Windows runner 构建和进行最终安装布局验证，不要将本地前端 build 当成离线发布验收。

## 文档导航

- [开发与交接手册](docs/DEVELOPMENT.md)：当前实现、目录、数据、测试、发布和接手清单。
- [Linux 开发与移植交接](docs/LINUX_DEVELOPMENT.md)：Ubuntu 开发环境、真实 Tauri 联调、sealed runtime、打包阻塞项与发布验收。
- [功能扩展路线](docs/ROADMAP.md)：离线基础环境、自动化任务和 AI Agent 辅助开发。
- [RPAZ 开发](docs/RPAZ_DEVELOPMENT.md)：schema v2、Runtime Context、直接运行和示例包。
- [Jupyter 集成](docs/JUPYTER_INTEGRATION.md)：真实能力、VS Code Jupyter 对照和明确边界。
- [AI Agent 设计](docs/AI_AGENT_DESIGN.md)：OpenAI-compatible 对话循环、RPAZ 工具和配置边界。
- [离线运行时](offline/README.md)：依赖策略、构建证明和缺包处理流程。
- [Windows 发布说明](docs/PORTABLE_RELEASE.md)：安装、数据目录和热更新。
- [架构设计](docs/architecture/DRPA_NEXT.md)：长期模块边界和安全原则。
- [更新日志](CHANGELOG.md)：面向发布和接手者的变更记录。

## 当前限制

- 当前稳定 Release 发布 Windows x64 Setup，以及 Linux x86_64 runtime-complete AppImage、现代 deb 与 UOS 20 专用 deb。现代包要求系统 glibc 2.35+；UOS 包以系统 glibc 2.28、x86_64 为最低目标并携带私有 glibc/C++ 运行层。两者仍需要内核、图形会话以及与显卡驱动匹配的 EGL/GL/GBM；真实 UOS 20、Ubuntu 24.04 与 Wayland 人工回归仍需持续记录，macOS 仍只有编译级基础。
- 当前更新入口使用本地 `.drpa-update`，已支持逐文件差量；尚未实现在线更新源、签名信任链和大文件块级差分。
- Jupyter 使用真实协议，但不是完整 VS Code Extension Host；远程 Kernel、ipywidgets、VS Code 调试器和所有第三方 MIME renderer 尚未实现。
- AI Agent 当前是单 Agent MVP，尚未提供流式输出、diff/checkpoint、会话导出或运行日志工具；自动调度、浏览器录制器、包签名和私有仓库仍处于路线阶段。
