# DRPA Next 开发与交接手册

本文档描述 `codex/drpa-next-platform` 分支的当前事实，供后续维护者定位代码、复现发布和继续扩展。旧 PySide6 代码与文档只是迁移参考，不能作为 DRPA Next 的实现说明。

## 1. 产品状态

当前交付目标只有 **Windows x64 离线桌面版**：

- 前端：React 19、TypeScript、Vite、Monaco。
- 桌面壳：Tauri 2。
- 权限与控制面：Rust Host。
- 自动化运行时：封装的 CPython 3.11.9 和 Python adapter。
- Notebook：真实 IPython Kernel、Jupyter Client 与 ZMQ。
- 浏览器：Chrome for Testing；UI WebView 使用 Fixed Version WebView2。
- 安装：无管理员权限、无应用注册表写入的 NSIS 引导安装器。
- 更新：本地 `.drpa-update` 文件级更新。

不要将“Rust crate 能在 Linux/macOS CI 编译”解释为这些平台已经可发布。平台定义保留用于未来适配，当前 Release workflow 只产出 Windows。

## 2. 仓库结构与所有权

```text
apps/desktop/
  src/                       React 页面、组件、状态和 typed gateway
  src-tauri/src/lib.rs       Tauri commands、运行时定位、Studio/Jupyter 生命周期
  src-tauri/src/bin/         独立 Windows 更新器
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

正式安装采用应用旁数据模型：

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

## 4. 离线运行时

`offline/runtime-spec.json` 固定 Python、uv、Chrome 和目标平台。`offline/requirements/runtime.txt` 必须包含直接与传递依赖的精确版本，不允许 VCS URL、editable、索引覆盖或未固定版本。

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

`execute_studio_cell` 是异步 Tauri command，所有运行时定位、Kernel 创建和阻塞式 JSONL 读取都进入 blocking worker。Notebook 打开后在后台调用 `prepare_studio_kernel` 预热；React 先提交“正在运行”状态并等待一帧再 invoke。UI 使用完整的 `minmax(0, 1fr)`/`min-height: 0` 容器链和内部滚动区，大量单元格不会扩张主工作区；单个 Monaco 编辑器最高 420 px，超出部分在编辑器内滚动。

这不是 VS Code Extension Host。新增 notebook 能力时优先遵守 nbformat 和 Jupyter 消息规范，不要复制依赖 `NotebookController`、VS Code 命令或扩展市场的代码。详细对照见 [`JUPYTER_INTEGRATION.md`](JUPYTER_INTEGRATION.md)。

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

## 7.1 AI Agent

Agent UI 位于基础设施导航。`run_agent_turn` 使用后台 Rust worker 调用 OpenAI-compatible Chat Completions，并执行最多 8 轮 function tools。三个知识库工具始终可用；绑定项目后再启用六个项目文件、manifest、构建和 30 秒 sealed Python 工具。API key 只保存在前端会话内。前端持久化最多 50 个本地对话及每个对话最近 120 条消息，支持逐会话项目绑定和配置面板折叠。实现与约束见 [`AI_AGENT_DESIGN.md`](AI_AGENT_DESIGN.md)。

“知识文档”使用安装数据目录 `knowledge/` 作为 Markdown 工作区。Rust Host 提供列表、UTF-8 读写、内联创建、重命名、递归删除、导入和导出命令，并对相对路径、扩展名、符号链接和 8 MiB 单文档上限做校验；写入使用同目录临时文件和替换。`apps/desktop/src-tauri/knowledge_seed/` 通过 `include_str!` 编译进 Host，首次初始化为 `RPAZ 开发指南/`，marker 存在后不覆盖用户修改。前端 `DocsPage.tsx` 提供树、搜索、自动保存、预览/编辑/分栏、GFM 渲染、相对文档跳转和拖拽导入。

当前仍从本地介质导入更新；在线 feed 与清单签名属于后续路线，见 [`ROADMAP.md`](ROADMAP.md)。

## 8. 开发与验证

### 8.1 前端

```bash
npm ci
npm run typecheck
npm run test
npm run build
```

### 8.2 Rust

```bash
cargo fmt --all --check
cargo clippy -p drpa-protocol -p drpa-package -p drpa-host --all-targets -- -D warnings
cargo test -p drpa-protocol -p drpa-package -p drpa-host
cargo check -p drpa-desktop
```

### 8.3 Python 与离线政策

```bash
python -m pip install -e "./runtime/python[test]"
python -m pytest -q runtime/python/tests
python tools/offline/validate_requirements.py offline/requirements/runtime.txt
python -m unittest discover -s tools/offline/tests -v
python -m compileall -q tools/offline offline/bootstrap runtime/python/src
```

### 8.4 必须在 Windows runner 验证的内容

- GUI subsystem 不弹 CMD。
- Fixed Version WebView2 可启动。
- runtime manifest 解析到基础 Python，而不是 venv 模板。
- 在最终安装布局创建环境并导入所有关键依赖。
- 真实 Jupyter/ZMQ 两单元执行。
- NSIS 编译与注册表指令守卫。
- `.drpa-update` 构建、内容保护和发布资产数量。

## 9. CI 与发布

- `.github/workflows/ci.yml`：前端、Python adapter、离线政策、Rust core 和桌面 Host 编译检查。
- `.github/workflows/offline-runtime.yml`：Windows sealed runtime 原生构建、air-gap smoke 和 prerelease。
- `.github/workflows/desktop-release.yml`：push 默认发布两个轻量 update 资产；手工选择 `full` 时才组合 runtime、WebView2、Host、更新器、示例和安装器。

发布前检查：

1. 更新 `CHANGELOG.md`。
2. 修改版本时同步 root/npm/Tauri/NSIS/runtime package/runtime spec/示例包/desktop workflow/offline workflow 中的版本来源，避免只改文件名。
3. 确认 `offline/requirements/runtime.txt` 已通过精确依赖政策检查。
4. 日常发布观察轻量 update 构建、基线库存合并与两个资产集合检查通过；完整发布还需观察 runtime bootstrap、Jupyter smoke、NSIS guard。
5. update Release 只包含 `.drpa-update` 与 `install-manifest.json`；full 基线 Release 只包含 Setup、`install-manifest.json` 与示例 RPAZ，不附带无法跨协议使用的轻量包。
6. 在独立 Windows 测试机安装到非系统盘，执行环境验证、Bing 示例、Notebook 两单元和一次更新回滚演练。
7. 预览版未签名时必须在发行说明中显式提示。

## 10. 新功能设计规则

- 先定义领域状态和失败恢复，再画页面。
- 把“已实现”“实验性”“仅设计”显示在文档和 UI 中。
- 离线优先：新功能不得隐式下载 Python 包、浏览器、模型或前端 CDN。
- Windows 优先不等于把平台判断散落到业务层；平台差异应隔离在 adapter。
- 长任务必须可取消、可恢复或明确不可恢复，并产生日志和审计事件。
- 密钥不写入 manifest、任务参数快照、日志或命令行。
- AI Agent 只能通过受策略控制的工具调用 Host，不能获得任意 Tauri invoke 或任意 shell 权限。

## 11. 接手清单

接手开发前建议依次完成：

1. 阅读本文件、`ROADMAP.md`、`offline/README.md` 和 `JUPYTER_INTEGRATION.md`。
2. 安装 preview-10 到非系统盘，确认 `data/` 实际位置。
3. 安装并运行 Bing 每日一图示例，检查日志与产物。
4. 在 Studio 新建中文名称项目，运行源码和两个 notebook 单元。
5. 阅读 `gateway.ts` 与 `src-tauri/src/lib.rs` 的对应 command。
6. 运行第 8 节全部本地检查。
7. 查看最新 Windows Actions 与 Release，确认分支和产物基线。
8. 开始新功能前建立 ADR 或更新 `ROADMAP.md` 的对应阶段与验收条件。

当前主开发分支：`codex/drpa-next-platform`。当前交接 PR：<https://github.com/EthanBird/drpa-client/pull/2>。
