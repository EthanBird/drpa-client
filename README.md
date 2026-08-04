# DRPA Next

DRPA Next 是一个本地优先、面向 Windows 与 Linux 的可扩展代码包运行管理器。用户无需配置系统 Python，即可安装、开发、运行和观察 `.rpaz` 自动化脚本包。

当前版本为 `2.0.0`。本次正式发行仅提供 Windows x64 全量 Setup；Linux x86_64 继续使用 `1.0.0` AppImage、现代发行版 deb 与 UOS Desktop 20 专用 deb。原有 PySide6 客户端已冻结，只作为 `.rpaz` v1 行为和迁移参考；新功能只进入 Tauri + React + Rust 架构。

## 当前能力

- 中文默认界面，支持正常移动、缩放、最大化、最小化和关闭窗口。
- 引导式、无注册表写入的 Windows NSIS 安装器；应用和用户数据均可放在非系统盘安装目录。
- 内置 CPython 3.11、完整离线 wheels、Chrome for Testing、Fixed Version WebView2 和真实 Jupyter Kernel 依赖。
- 安装、拖拽导入、运行、取消和卸载 `.rpaz`；包操作集中在右键菜单。
- 工作室可新建项目、编辑源码和 Notebook、直接运行工作副本、导出 `.rpaz`，也可将已安装包复制为可编辑项目。
- 基于 `ipykernel`、`jupyter_client`、`pyzmq`、`nbformat` 的真实 Jupyter 执行链路，并通过标准 `complete_request` / `inspect_request` 为 Python 文件和 Notebook 提供离线补全、悬停文档与参数提示。
- 运行记录持久化到 Host SQLite；可进入详情查看完整时间线、筛选/复制日志、参数、产物、错误回溯和重启中断状态。
- 内置自有数据工作台：工作区 SQLite、外部 SQLite、PostgreSQL/MySQL 与只读 Excel 工作簿数据源，对象树、Monaco SQL 编辑器、AI 写 SQL、结果网格、字段结构和查询历史；Excel sheet 会映射为可查询表，RPAZ 可通过 `ctx.sql` 事务化读写共享的工作区 SQLite。
- 基础设施内置双模式 AI Agent：RPAZ Agent 通过 ProviderAdapter 与 ToolRegistry 编排内置、Skills 2.0 和插件工具；JCode 开发者 Agent 提供完整文件、命令和开发工具。两种模式复用 OpenAI-compatible URL/model/key、流式 Markdown、持久会话和逐会话项目绑定，开发工作室右侧可直接切换。
- 内置 DRPA Local Dify：管理 Chat/Completion/Workflow/Chatflow 应用和 OpenAI-compatible Provider；提供可视化工作流画布、节点连线与属性、图校验、本地节点执行、流式 Markdown 调试、SQLite 运行轨迹、Dify YAML DSL 导入导出，以及带独立应用 Token 的本地 Dify Service API；通用 Provider 插件可把外部模型服务接入 AI Agent。
- Skills 2.0 使用 `skill.yaml + instructions.md` 能力包，可携带工作流、资源、可执行 Python/Command 工具和可调用代码库；设置页提供目录树、Monaco 编辑及文件/目录创建、重命名、删除，旧版 `SKILL.md` 自动迁移。
- 内置能力驱动的离线插件系统：支持 `.drpa-plugin` 安装、配置、启停、多服务、Provider、工具、调试端点、结构化事件和自定义面板；插件开发工作台可生成 Tool/Service/Bundle 模板并验证、构建、安装。内置 Dify2API 可把 Dify App API 转换为本地 OpenAI 兼容接口、适配 `tool_calls`，并提供服务与上游调试器。
- 内置本地 Markdown 知识库：支持目录树、阅读/编辑/分栏渲染、相对文档跳转、内联新建/重命名/删除、拖拽导入、原生导入导出与自动保存，并首次初始化多篇详细 RPAZ 开发指南。
- 协议化 `.drpa-update` 差量更新，包含可视化进度、结构/大小检查、精确基线匹配、独立 Worker、失败回滚和新 Host 启动确认；日常更新不重复携带 WebView2/Chrome，并始终保护安装目录下的 `data/`。
- Windows 上 `ctx.browser()` 使用工作区级持久 Chrome/Profile；任务成功、失败或调用 `page.quit()` 都只释放当前脚本句柄，不退出浏览器，后续 Studio 与多个 RPAZ 任务可直接复用登录态和调试页面。

Windows 2.0.0 正式版下载：<https://github.com/EthanBird/drpa-client/releases/tag/desktop-v2.0.0>

> `2.0.0` 是当前推荐的 Windows 全量安装基线。既有安装可直接运行最新 Setup 完成大版本升级；完成一次全量安装后，后续版本使用轻量 `.drpa-update`，不会再次打包 WebView2、未变化的 Chrome/runtime 或用户数据。

## 平台状态

| 平台 | 源码开发 | CI | 正式发行 |
| --- | --- | --- | --- |
| Windows x64 | 完整支持 | 前端、Rust、Python、runtime、安装器 | `2.0.0` 全量安装包与后续轻量更新 |
| Linux x86_64 | Host、Python/Jupyter、XDG 与平台 UI 已适配 | Ubuntu 22.04 构建；Debian 10 与 Deepin 20.8/glibc 2.28 实装、React/IPC 和非白屏截图门禁 | `1.0.0` AppImage、现代 deb 与 UOS 20 专用 deb |
| macOS | Rust core 与桌面 Host 编译检查 | 编译检查 | 尚未发布 |

Linux x86_64 已把 sealed CPython 3.11/Jupyter/Chrome 作为只读 Tauri resource 放入 AppImage 与 deb，Host 通过 `resource_dir` 定位，生成环境与用户数据写入 XDG 目录。现代 deb `1.0.0-2` 把 WebKitGTK 4.1、JavaScriptCoreGTK、GTK、GStreamer 和 helper process 私有安装到 `/opt/drpa-next`，适用于 glibc 2.35+。UOS 包修订 `1.0.0-2+uos20.3` 面向 UOS Desktop 20 Professional（eagle）/glibc 2.28：固定安装到 `/opt/drpa-next-uos20`，内置 glibc/C++/NSS、WebKitGTK、GBM/libdrm，以及隔离的 GLVND、Mesa EGL 和 swrast/llvmpipe 软件渲染闭包，不再依赖目标机的 `libwebkit2gtk-4.1-0`、EGL/GL 或 DRI 包；GTK 固定走 XIM 输入法桥，避免聚焦输入框后页面冻结。CI 会在 Debian 10 和 Deepin 20.8 用户态真实安装，要求离线 Python/Jupyter、Chrome、React 挂载、真实 X11 输入框点击/键入/后续按钮点击、Tauri IPC、存活的 WebKitWebProcess 和非白屏截图全部通过；Debian 10 额外执行可见文字像素门禁，Deepin 固定镜像无系统字体时以布局与真实交互标记验收。内核、X11 server、系统字体和 XIM 输入法服务仍由目标机提供；真实 UOS 20/DDE/kernel 4.19/Fantasy II-M 实体机仍需人工复核。Linux 暂不支持 `.drpa-update`。接手 Linux 端请先阅读 [Linux 开发与移植交接](docs/LINUX_DEVELOPMENT.md)；复现或维护 UOS 包请阅读 [UOS 20 构建与打包手册](docs/UOS20_PACKAGING.md)。

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
- [UOS 20 构建与打包手册](docs/UOS20_PACKAGING.md)：glibc 2.28 兼容层、私有依赖、ELF 修补、容器门禁、故障复盘和实体机验收。
- [功能扩展路线](docs/ROADMAP.md)：离线基础环境、自动化任务和 AI Agent 辅助开发。
- [RPAZ 开发](docs/RPAZ_DEVELOPMENT.md)：schema v2、Runtime Context、直接运行和示例包。
- [Jupyter 集成](docs/JUPYTER_INTEGRATION.md)：真实能力、VS Code Jupyter 对照和明确边界。
- [数据工作台与 `ctx.sql`](docs/DATA_WORKBENCH.md)：SQLite 存储分层、Host API、脚本 API 与扩展约定。
- [AI Agent 设计](docs/AI_AGENT_DESIGN.md)：OpenAI-compatible 对话循环、RPAZ 工具和配置边界。
- [Local Dify 开发平台](docs/LOCAL_DIFY.md)：应用、Provider、调试、Service API、DSL 兼容和套娃链路。
- [Local Dify 工作流设计器](docs/LOCAL_DIFY_WORKFLOW.md)：Workflow IR、可视化画布、节点执行、校验、调试和 Dify Graph 互操作。
- [Skills 2.0 与插件系统](docs/SKILLS_AND_PLUGINS.md)：能力包代码工具、插件清单、进程生命周期与 Dify 工具桥。
- [离线运行时](offline/README.md)：依赖策略、构建证明和缺包处理流程。
- [Windows 发布说明](docs/PORTABLE_RELEASE.md)：安装、数据目录和热更新。
- [架构设计](docs/architecture/DRPA_NEXT.md)：长期模块边界和安全原则。
- [更新日志](CHANGELOG.md)：面向发布和接手者的变更记录。

## 当前限制

- Windows 当前稳定 Release 为 `desktop-v2.0.0` x64 Setup；Linux 发行资产继续保持在 `desktop-v1.0.0`，包含 runtime-complete AppImage、现代 deb 与 UOS 20 专用 deb。现代包要求系统 glibc 2.35+；UOS 包以系统 glibc 2.28、x86_64 为最低目标并携带私有 glibc/C++/WebKitGTK/Mesa llvmpipe 运行层。UOS 包固定 X11 软件渲染以换取老显卡兼容性，仍需要系统内核和 X11 server；真实 UOS 20/Fantasy II-M、Ubuntu 24.04 与 Wayland 人工回归仍需持续记录，macOS 仍只有编译级基础。
- 当前更新入口使用本地 `.drpa-update`，已支持逐文件差量；尚未实现在线更新源、签名信任链和大文件块级差分。
- Jupyter 使用真实协议，但不是完整 VS Code Extension Host；远程 Kernel、ipywidgets、VS Code 调试器和所有第三方 MIME renderer 尚未实现。
- AI Agent 当前采用单 Orchestrator，已支持流式 Markdown、ProviderAdapter、动态 ToolRegistry、Skills 2.0、插件 Provider/Tool 与本地会话；diff/checkpoint、会话导出、长期服务双向 JSON-RPC 和 Skill evaluation 仍在后续阶段。
- Local Dify 已执行 Chat、Completion、Workflow 与 Chatflow；基础节点可本地运行，导入的扩展 Dify 节点会保留并标记兼容状态，Iteration/Loop、知识检索、Agent 与并行汇聚继续按节点逐步接入。
