# DRPA Next 开发与交接手册

本文档描述 `codex/drpa-next-platform` 分支和 `1.0.0` 基线的当前事实，供后续维护者定位代码、复现发布和继续扩展。旧 PySide6 代码与文档只是迁移参考，不能作为 DRPA Next 的实现说明。Linux 接手者还应阅读 [`LINUX_DEVELOPMENT.md`](LINUX_DEVELOPMENT.md)。

## 1. 产品状态

当前正式交付包括 **Windows x64 Setup**，以及 **Linux x86_64 AppImage、现代 deb 与 UOS 20 专用 deb**。以下能力是两端共享的离线桌面基座，安装和更新策略按平台分离：

- 前端：React 19、TypeScript、Vite、Monaco。
- 桌面壳：Tauri 2。
- 权限与控制面：Rust Host。
- 自动化运行时：封装的 CPython 3.11.9 和 Python adapter。
- Notebook：真实 IPython Kernel、Jupyter Client 与 ZMQ。
- 浏览器：Chrome for Testing；Windows UI WebView 使用 Fixed Version WebView2，Linux 发行包携带私有 WebKitGTK 4.1 闭包。
- 安装：无管理员权限、无应用注册表写入的 NSIS 引导安装器。
- 更新：本地 `.drpa-update` 文件级更新。

Linux x86_64 已完成第一轮发行适配：Rust core/Tauri Host、XDG 数据目录、`xdg-open`、平台能力协议、sealed runtime、AppImage/deb 内嵌资源定位和 Linux 进程组取消均有实现；`.github/workflows/linux-desktop.yml` 在 Ubuntu 22.04 构建 runtime-complete AppImage，再由已验证 AppDir 生成 `/opt/drpa-next` 现代 deb 和 `/opt/drpa-next-uos20` UOS 兼容 deb。现代包门禁会卸载 Runner 的系统 WebKitGTK；UOS 包还会内置固定 glibc/C++ 运行层并在 Debian 10/glibc 2.28 容器中验证 Python/Jupyter 原生扩展、Chrome 和 X11。普通 push 只上传 Actions artifact；显式发布会把三种包、SHA-256、两个 deb manifest 和 wheelhouse lock 写入 `desktop-v1.0.0`。Ubuntu 24.04、真实 UOS 20、Wayland 与人工 GUI 验收仍是持续回归项。Windows Release workflow 保持独立。

## 2. 仓库结构与所有权

```text
apps/desktop/
  src/                       React 页面、组件、状态和 typed gateway
  src-tauri/src/lib.rs       Tauri commands、运行时定位、Studio/Jupyter 生命周期
  src-tauri/src/bin/         独立 Windows 更新器；Linux 不复用其发布协议
crates/
  drpa-protocol/             DTO、运行事件和协议版本
  drpa-package/              schema v2、归档与路径安全
  drpa-host/                 包安装、运行准备、状态与历史
runtime/python/
  src/drpa_runner/           worker、Context、事件和 Jupyter bridge
offline/
  runtime-spec.json          Python/uv/Chrome 与平台声明
  requirements/runtime.txt  完整精确依赖集合
  bootstrap/                 离线环境初始化
tools/offline/               sealed runtime 构建和依赖政策检查
tools/linux/                 现代/UOS deb 构建、私有 ELF 运行层、Linux 布局与发布验证
tools/windows/               .drpa-update 构建
installer/windows/           NSIS 安装脚本
examples/                    示例项目与已构建 RPAZ
.github/workflows/           CI、sealed runtime 与 Windows Release
src/drpa_client/            旧 PySide6 兼容参考；不接收新功能
```

### 2.1 前端边界

`apps/desktop/src/infra/gateway.ts` 是 React 与 Host 的唯一业务入口。新增功能时：

1. 在领域模型中定义返回值。
2. 扩展 `DesktopGateway`。
3. 同时实现 `tauriGateway` 和浏览器开发用 `mockGateway`。
4. 在 Rust command 中重新校验参数。
5. 添加组件测试和 Host 测试。

组件不能直接访问 SQLite、启动进程或拼接安装区路径。浏览器 mock 只用于 UI 开发，不是集成测试替代品。

### 2.2 Host 边界

Rust Host 负责：

- 校验 RPAZ 和 manifest。
- 控制安装、卸载和工作副本。
- 定位且验证 sealed runtime 清单。
- 生成运行配置并监督 worker 生命周期。
- 保存包、运行、日志和产物元数据。
- 校验与应用更新包。
- 限制所有来自 WebView 的路径和参数。

Python adapter 负责执行用户代码和生成结构化事件。不能把安全策略只写在 Python 或 React 中。

## 3. 数据与安装布局

Windows 正式安装采用应用旁数据模型：

```text
<install>/
├── DRPA Next.exe
├── drpa-updater.exe
├── runtime/                         只读 sealed runtime 源
├── webview2/                        Fixed Version WebView2
├── examples/
└── data/
    ├── packages/                    已安装 RPAZ 版本
    ├── projects/                    Studio 工作副本
    ├── build/                       Studio 导出包
    ├── runs/                        运行日志与产物
    ├── system/drpa.sqlite3          Host 运行、事件与产物索引
    ├── databases/workspace.sqlite3 RPAZ `ctx.sql` 与数据工作台
    ├── runtime-environment/
    │   └── environment/             最终位置创建的 Python 环境
    └── updates/                     更新暂存、备份和状态
```

关键约束：

- 不把业务数据默认放到 `%APPDATA%`、`%LOCALAPPDATA%` 或用户系统盘。
- 不把预先创建的 venv 打入安装包。它包含不可移植的绝对路径，必须在最终安装位置用 `bootstrap_runtime.py` 离线创建。生成环境的 marker 分别记录 Python/requirements 与轻量 adapter wheel；仅 adapter 变化时原位重装 adapter，不重建完整依赖环境。
- `data/` 永远不进入更新清单；修复运行时只重建 `data/runtime-environment/`。
- 已安装包不可原地编辑。“在工作室打开”会复制到 `data/projects/`。
- 用户移动整个安装目录后，应从新位置启动并执行运行环境验证；不要只移动 `runtime/`。

Linux 默认使用 `app.path().app_local_data_dir()/workspace`，由 Tauri 按 XDG 规则解析；开发测试应通过 `DRPA_DATA_DIR` 指向仓库内隔离目录。Linux 的只读应用布局、runtime 资源位置和 AppImage/deb 安装合同见 [`LINUX_DEVELOPMENT.md`](LINUX_DEVELOPMENT.md)，不要照搬 Windows 的应用旁 `data/`。

## 4. 离线运行时

`offline/runtime-spec.json` 固定 Python、uv、Chrome 和目标平台，当前声明 `windows-x86_64`、`linux-x86_64` 与两个 macOS 架构。`offline/requirements/runtime.txt` 必须包含直接与传递依赖的精确版本，不允许 VCS URL、editable、索引覆盖或未固定版本。构建器为每个平台生成 `wheelhouse-lock.json`，记录实际 wheel 文件、大小与 SHA-256；Linux 最终布局再由 `tools/linux/verify_runtime_layout.py` 检查 ABI 污染、散列和可执行位。任何平台都必须在原生 runner 完成 air-gap 证明后才能发布。

sealed runtime 的 `manifest.json` 是机器可验证合同，至少包含：

- bundle/schema/platform/version；
- 精确的 `pythonExecutable` 和 `browserExecutable` 相对路径；
- 每个文件的大小与 SHA-256。

曾经的 `No pyvenv.cfg file` 根因是递归搜索命中了 CPython 自带的 venv 模板启动器。禁止重新引入“找到第一个 `python.exe`”的逻辑；只允许读取并验证 manifest 中的精确路径。

依赖增加、缺包诊断与重新发布流程见 [`../offline/README.md`](../offline/README.md)。

## 5. RPAZ 与运行协议

新包使用 `manifest.yaml` schema v2。最小开发流程和 Runtime Context API 见 [`RPAZ_DEVELOPMENT.md`](RPAZ_DEVELOPMENT.md)。

运行链路：

```text
Workbench / Studio
  → Tauri command
  → Rust Host prepare_run / prepare_development_run
  → sealed environment python -I
  → drpa_runner worker
  → JSONL RuntimeEvent
  → Host state / run history / UI
```

兼容性规则：

- 协议字段只增不删时，新增字段应提供默认值。
- 破坏性协议变化必须递增协议/schema 版本并写迁移说明。
- entrypoint、资产、wheel 和输出路径必须 containment-check。
- worker stdout 是结构化通道，业务日志由 Context 转成事件，不应随意打印非协议数据。
- 包不能在线调用 pip；额外依赖只能来自 sealed baseline 或经过锁定、校验的平台 wheel 集。

## 6. Studio 与 Jupyter

Studio 项目名称允许自然语言；项目 ID 与 package ID 由 Host 生成，用户无需输入包名格式。项目可直接运行，不需要先安装。

Notebook 的进程边界：

```text
React notebook UI
  → execute_studio_cell
  → Rust StudioKernelManager（每项目一个 bridge）
  → Python drpa_runner.kernel
  → jupyter_client KernelManager
  → ipykernel + ZMQ shell/iopub/control
```

Rust 与 Python bridge 之间使用 JSONL；bridge 与 IPython Kernel 之间使用真实 Jupyter wire protocol。当前支持标准 `stream`、`execute_result`、`display_data`、`error` 和变量读取。

Studio Python 补全复用同一个项目 Kernel：Monaco 调用 `complete_studio_python`，Host 发送 JSONL `complete` 请求，bridge 使用 Jupyter `complete_request` 获取 IPython/Jedi 结果。前端必须在 Monaco UTF-16 offset 与 Jupyter Unicode code-point offset 之间转换，避免中文和 emoji 之前的光标范围错位。

`execute_studio_cell` 是异步 Tauri command，所有运行时定位、Kernel 创建和阻塞式 JSONL 读取都进入 blocking worker。Notebook 打开后在后台调用 `prepare_studio_kernel` 预热；React 先提交“正在运行”状态并等待一帧再 invoke。UI 使用完整的 `minmax(0, 1fr)`/`min-height: 0` 容器链和内部滚动区，大量单元格不会扩张主工作区；单个 Monaco 编辑器最高 420 px，超出部分在编辑器内滚动。

这不是 VS Code Extension Host。新增 notebook 能力时优先遵守 nbformat 和 Jupyter 消息规范，不要复制依赖 `NotebookController`、VS Code 命令或扩展市场的代码。详细对照见 [`JUPYTER_INTEGRATION.md`](JUPYTER_INTEGRATION.md)。

## 6.1 运行记录与数据工作台

`crates/drpa-host/src/run_store.rs` 使用 `data/system/drpa.sqlite3` 持久化 run、事件和产物。Host 启动时把上次异常退出后仍为 running/queued 的记录标记为 interrupted；运行记录页通过 `get_run_detail` 读取完整概览、日志、参数、错误回溯和产物，而不是只依赖当前进程内存。

用户数据使用独立的 `data/databases/workspace.sqlite3`。RPAZ 通过 `ctx.sql` 访问，数据工作台通过 `apps/desktop/src-tauri/src/database.rs` 访问；React 始终经过 `DesktopGateway`。实现和扩展约定见 [`DATA_WORKBENCH.md`](DATA_WORKBENCH.md)。

## 7. Windows 更新

`tools/windows/build_update_package.py` 从最终 stage 生成完整 `install-manifest.json` 和 `.drpa-update`。

安装库存记录每个受管文件的路径、大小、构建指纹与组件；构建指纹只用于 CI 比较版本差异，不在客户端更新时重复计算或校验。传入 `--base-manifest` 时只打包新增/变化文件，并生成旧文件删除列表。

`0.3.0` 起使用更新 schema 2 / Host protocol 2 / Worker protocol 2。delta 包必须声明 `packageKind=delta`、`minimumHostVersion` 和精确 `baseVersion`；任一兼容条件不满足时 Host 保持运行并提示使用全量 Setup，不创建退出请求。

默认策略：

1. `data/` 和 Fixed Version WebView2 始终受保护；更新器从会话副本运行，因此安装目录中的更新器属于可更新受管文件。
2. 没有基线清单时，sealed runtime 与 Chrome 不进入日常更新包；有基线时也只有摘要变化的文件才会进入差量包。
3. 更新包内嵌当前版本的独立 Worker，因此 Worker 自身可以随包升级，而不覆盖正在使用的安装副本。
4. Host 在应用保持打开时检查 schema、Host/Worker protocol、最低 Host 版本、平台、精确基线、安全路径、文件数量和写入大小，停止 Studio Kernel，并在 `data/updates/sessions/<id>/` 创建可审计会话。
5. 包内 Worker 优先脱离父进程 Job，先复核暂存文件并写入 `worker-ready`；Host 只有同时读到 `waitingForRestart` 和就绪标记才创建 `restart-requested` 并退出。
6. Worker 确认主程序文件锁已释放后才替换所有受管文件，从安装目录启动新主程序，并通过 `DRPA_UPDATE_SESSION_ID` 要求新 Host 在主窗口构建成功后写入 `startup-ack`。
7. 只有收到启动确认才删除备份；新进程早退、30 秒未确认或任一替换失败时，Worker 会结束新进程、按逆序恢复全部文件、持久化失败状态并从安装目录重新启动旧版本。

CI 默认执行 `update` 发布：stage 主程序、更新器、`bootstrap_runtime.py` 和当前 `drpa-runtime-python` wheel，使用 `--partial` 合并上一 Release 的完整库存，不删除 stage 未包含的 CPython、Chrome、WebView2 或文档。adapter 更新在下次运行时定位时增量安装，通常不超过数秒。只有手工 `workflow_dispatch(release_kind=full)` 才构建完整 sealed runtime、WebView2、NSIS Setup 和示例资产。

完整安装 stage 禁止符号链接、Windows Junction 和其他 reparse point。`build_update_package.py` 在生成库存前会逐目录检查并直接失败，保证 `runtime/`、`webview2/` 和应用文件全部来自当前安装 stage，而不是外部目录。

## 8. AI Agent

Agent UI 位于基础设施导航。`run_agent_turn` 使用后台 Rust worker 调用 OpenAI-compatible Chat Completions，并执行最多 8 轮 function tools。三个知识库工具始终可用；绑定项目后再启用六个项目文件、manifest、构建和 30 秒 sealed Python 工具。API key 只保存在前端会话内。前端持久化最多 50 个本地对话及每个对话最近 120 条消息，支持逐会话项目绑定和配置面板折叠。实现与约束见 [`AI_AGENT_DESIGN.md`](AI_AGENT_DESIGN.md)。

“知识文档”使用安装数据目录 `knowledge/` 作为 Markdown 工作区。Rust Host 提供列表、UTF-8 读写、内联创建、重命名、递归删除、导入和导出命令，并对相对路径、扩展名、符号链接和 8 MiB 单文档上限做校验；写入使用同目录临时文件和替换。`apps/desktop/src-tauri/knowledge_seed/` 通过 `include_str!` 编译进 Host，首次初始化为 `RPAZ 开发指南/`，marker 存在后不覆盖用户修改。前端 `DocsPage.tsx` 提供树、搜索、自动保存、预览/编辑/分栏、GFM 渲染、相对文档跳转和拖拽导入。

当前仍从本地介质导入更新；在线 feed 与清单签名属于后续路线，见 [`ROADMAP.md`](ROADMAP.md)。

## 9. 开发与验证

### 9.1 前端

```bash
npm ci
npm run typecheck
npm run test
npm run build
```

### 9.2 Rust

```bash
cargo fmt --all --check
cargo clippy -p drpa-protocol -p drpa-package -p drpa-host --all-targets -- -D warnings
cargo test -p drpa-protocol -p drpa-package -p drpa-host
cargo check -p drpa-desktop
```

### 9.3 Python 与离线政策

```bash
python -m pip install -e "./runtime/python[test]"
python -m pytest -q runtime/python/tests
python tools/offline/validate_requirements.py offline/requirements/runtime.txt
python -m unittest discover -s tools/offline/tests -v
python -m compileall -q tools/offline offline/bootstrap runtime/python/src
```

### 9.4 Linux 源码联调

Ubuntu/Debian 的系统依赖、开发 Python、环境变量和真实 Tauri 启动命令见 [`LINUX_DEVELOPMENT.md`](LINUX_DEVELOPMENT.md)。最小联调顺序是：

1. `npm run dev` 验证浏览器 mock UI；
2. `cargo check -p drpa-desktop` 验证 WebKitGTK 链接；
3. 使用 `DRPA_RUNTIME_PYTHON`、`DRPA_RUNTIME_PYTHONPATH` 和隔离的 `DRPA_DATA_DIR` 启动 `npm run tauri:dev`；
4. 跑通普通 RPAZ、Bing 示例、两个 Notebook 单元、知识文档和目录打开；
5. 再切换到 `DRPA_RUNTIME_ROOT` 验证 `linux-x86_64` sealed runtime。

源码联调通过不等于 Linux 包可发布。当前 AppImage 与两种 deb 都把 runtime 放入只读 resource，并由 CI 验证断网初始化；deb 还必须证明 WebKitGTK/JavaScriptCoreGTK/GTK 私有闭包完整、`Depends` 不含系统 WebKitGTK，并通过真实安装、启动、卸载和用户数据保留检查。UOS 包还必须验证私有加载器、glibc/C++ 库、全部 ELF 的解释器/RPATH，并在 glibc 2.28 用户态运行 Python/Jupyter、Chrome 和 GUI。跨发行版、断网、只读应用目录、X11 与 Wayland 条件仍需持续验收。

### 9.5 必须在 Windows runner 验证的内容

- GUI subsystem 不弹 CMD。
- Fixed Version WebView2 可启动。
- runtime manifest 解析到基础 Python，而不是 venv 模板。
- 在最终安装布局创建环境并导入所有关键依赖。
- 真实 Jupyter/ZMQ 两单元执行。
- NSIS 编译与注册表指令守卫。
- `.drpa-update` 构建、内容保护和发布资产数量。

## 10. CI 与发布

- `.github/workflows/ci.yml`：前端、Python adapter、离线政策、Rust core 和桌面 Host 编译检查；Ubuntu 目前只做到 Host 编译，没有 GUI/runtime 最终包验收。
- `.github/workflows/offline-runtime.yml`：Windows sealed runtime 原生构建、air-gap smoke 和 prerelease。
- `.github/workflows/desktop-release.yml`：push 默认发布两个轻量 update 资产；手工选择 `full` 时才组合 runtime、WebView2、Host、更新器、示例和安装器。
- `.github/workflows/linux-desktop.yml`：Ubuntu 22.04 构建 AppImage、现代 deb `1.0.0-2` 与 UOS deb `1.0.0-2+uos20.3`，验证 sealed runtime、私有 WebKitGTK/Mesa llvmpipe 闭包、XIM 输入桥，以及 Debian 10 与 Deepin 20.8/glibc 2.28 的真实输入点击/键入/后续交互、React/IPC、非白屏 UI、卸载和发布资产；可见文字组件由有系统字体的 Debian 10 门禁负责。

发布前检查：

1. 更新 `CHANGELOG.md`。
2. 修改版本时同步 root/npm/Tauri/NSIS/runtime package/runtime spec/示例包/desktop workflow/offline workflow 中的版本来源，避免只改文件名。
3. 确认 `offline/requirements/runtime.txt` 已通过精确依赖政策检查。
4. 日常发布观察轻量 update 构建、基线库存合并与两个资产集合检查通过；完整发布还需观察 runtime bootstrap、Jupyter smoke、NSIS guard。
5. update Release 只包含 `.drpa-update` 与 `install-manifest.json`；full 基线 Release 只包含 Setup、`install-manifest.json` 与示例 RPAZ，不附带无法跨协议使用的轻量包。
6. 在独立 Windows 测试机安装到非系统盘，执行环境验证、Bing 示例、Notebook 两单元和一次更新回滚演练。
7. 资产未签名时必须在发行说明中显式说明，不得因版本号进入稳定版就省略供应链状态。

## 11. 新功能设计规则

- 先定义领域状态和失败恢复，再画页面。
- 把“已实现”“实验性”“仅设计”显示在文档和 UI 中。
- 离线优先：新功能不得隐式下载 Python 包、浏览器、模型或前端 CDN。
- Windows 优先不等于把平台判断散落到业务层；平台差异应隔离在 adapter。
- UI 必须根据 Host 暴露的平台能力决定功能是否显示，不能让 Linux 用户点击 Windows-only 更新入口后才得到错误。
- 长任务必须可取消、可恢复或明确不可恢复，并产生日志和审计事件。
- 密钥不写入 manifest、任务参数快照、日志或命令行。
- AI Agent 只能通过受策略控制的工具调用 Host，不能获得任意 Tauri invoke 或任意 shell 权限。

## 12. 接手清单

接手开发前建议依次完成：

1. 阅读本文件、`ROADMAP.md`、`offline/README.md` 和 `JUPYTER_INTEGRATION.md`；Linux 开发者额外完整阅读 `LINUX_DEVELOPMENT.md`。
2. 查看分支、PR、最新提交和工作区状态，先区分当前 Tauri 实现与冻结的 PySide6 参考代码。
3. Windows 维护者安装 `1.0.0` 到非系统盘；Linux 维护者使用隔离的 `DRPA_DATA_DIR` 启动真实 Tauri Host，确认数据目录实际位置。
4. 安装并运行 Bing 每日一图示例，检查实时日志、进度、输出目录和产物。
5. 在 Studio 新建中文名称项目，运行源码、Markdown 单元和两个 Python notebook 单元。
6. 阅读 `apps/desktop/src/infra/gateway.ts` 与 `apps/desktop/src-tauri/src/lib.rs` 的对应 command，确认参数在 Host 重新验证。
7. 运行第 9 节全部本地检查，并对目标平台执行原生 GUI/runtime 测试。
8. 查看最新 Windows/Linux Actions 与 `desktop-v1.0.0` Release，确认 Setup、AppImage、现代/UOS deb、wheelhouse lock 和对应清单均来自成功的原生 runner；两个 deb manifest 版本应分别为 `1.0.0-2` 与 `1.0.0-2+uos20.3`，且 `depends` 都不含 `libwebkit2gtk-4.1-0`。
9. 开始新功能前建立 ADR 或更新 `ROADMAP.md` 的对应阶段与验收条件。

当前主开发分支：`codex/drpa-next-platform`。当前交接 PR：<https://github.com/EthanBird/drpa-client/pull/2>。Windows `1.0.0` 发布基线提交为 `5e6c793`；Linux `1.0.0` 由专用 Ubuntu 22.04 workflow 构建并发布，后续继续完成 [`LINUX_DEVELOPMENT.md`](LINUX_DEVELOPMENT.md) 中的 Ubuntu 24.04、Wayland 和人工 GUI 回归。
