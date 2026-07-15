# 更新日志

本项目从 `0.2.0` 开始维护面向开发者和发布使用者的变更记录。格式参考 Keep a Changelog；预览版可能继续调整内部协议，稳定版发布前必须明确迁移策略。

## [Unreleased]

## [1.0.0] - 2026-07-15

### Linux x86_64 发布

- 新增 Ubuntu 22.04 原生离线桌面流水线：构建 CPython 3.11 sealed runtime，将 Jupyter、DrissionPage 和 Chrome for Testing 作为 Tauri resource 内嵌 AppImage 与 deb，并上传两种包、SHA-256、deb manifest 与 wheelhouse lock。
- Host 通过 Tauri `resource_dir` 定位 AppImage 内的只读 runtime；生成环境、缓存、项目和日志继续写入 XDG 本地数据目录。
- 新增平台能力协议；Linux 设置页隐藏 Windows `.drpa-update`，运行环境和目录打开文案使用 Linux/XDG 语义。
- Linux Python worker 与 Studio Kernel 进入独立 process group；取消任务先发送 `SIGTERM`，超时后发送 `SIGKILL`，避免浏览器等后代进程残留，且 cancelled 状态不会被后台退出覆盖为 failed。
- sealed runtime 新增 `wheelhouse-lock.json`，记录实际 wheel 文件、大小和 SHA-256；最终 AppImage 解包验收拒绝 Windows/macOS/musl wheel、缺失文件和丢失的可执行位。
- AppImage、deb、各自 SHA-256、deb manifest 与 wheelhouse lock 随 `desktop-v1.0.0` 正式发布；deb 通过元数据、真实安装、X11 启动、卸载和用户数据保留验证。Ubuntu 24.04、Wayland 与人工 GUI 回归仍在持续补充。

### 发布

- 发布首个稳定版 Windows x64 全量离线安装包，统一桌面端、Rust workspace、Python 运行时、安装器、示例包和发布流水线的产品版本。
- 全量安装包包含 DRPA Next、独立更新 Worker、封装 Python 3.11/Jupyter/DrissionPage 运行时、Chrome for Testing、Fixed Version WebView2、知识文档和 Bing 每日一图示例。
- GitHub 全量发布使用稳定标签 `desktop-v1.0.0`；后续无必要时继续发布轻量 `.drpa-update`，避免重复分发浏览器和运行时依赖。

## [0.3.0] - 2026-07-14

### 新增

- 安装文件库存与逐文件差量更新：Release 发布 `install-manifest.json`，更新包内嵌独立 Worker，并在应用内显示校验、替换、回滚和重启进度。
- 设置页增加亮色/暗色主题，默认亮色；移除紧凑/舒适密度切换。
- 开发工作室增加项目右键删除、文件内联新建、重命名、删除和外部拖入。
- 运行工作台增加分级日志、搜索/过滤、最新任务摘要和专业时间戳。
- 自动化计划只读预览页：展示任务、触发器、下一次运行、执行策略与健康状态，并在 Host/前端快照协议中新增 `automations` 字段。
- 开发工作室文件菜单：新建文件、文件夹、选择导入与拖拽导入。
- 运行工作台支持创建本地任务配置，并读取 manifest 参数默认值。
- 基础设施增加轻量 RPAZ AI Agent：支持 OpenAI-compatible URL、model、会话级可选 key、项目绑定、六个项目工具、三个知识库工具和最多 8 轮 function calling。
- AI Agent 工具支持项目文件读取/写入、manifest 校验、RPAZ 构建和 30 秒 sealed Python 辅助，并显示工具事件、用量与耗时。
- AI Agent 增加本地对话列表：支持新建、切换、自动命名、重命名、删除和清空会话；每个会话独立保存项目绑定与最近消息，配置面板可随时隐藏或显示。
- 原静态 HTML“开发文档”升级为本地 Markdown“知识文档”：支持目录树、搜索、阅读/编辑/分栏渲染、自动保存、相对链接、内联创建/重命名/删除、拖拽导入和原生导入导出；首次初始化 11 篇详细 RPAZ 指南，重点覆盖 manifest、ctx 默认配置和 DrissionPage。
- 左下角账户卡片从 Host 获取当前系统用户，不再显示固定示例账户。

### 修复

- 修复旧安装缺少库存清单、更新协议又没有明确兼容边界时仍可能进入退出阶段的问题。`0.3.0` 建立 schema-2 全量基线；Host 在退出前检查 schema、Host/Worker 协议、最低客户端版本和精确 `baseVersion`。
- 更新 Worker 采用独立进程组并优先脱离父进程 Job；新程序固定从安装目录启动，只有在新 Host 创建主窗口并写入 `startup-ack` 后才清理备份，早退或 30 秒未确认时自动结束新进程、回滚并恢复旧版本。
- 更新应用取消逐文件 SHA-256 校验，改为 schema、平台、基线版本、安全路径、文件数量和写入大小检查；桌面 Release 取消独立 `.sha256` 资产。
- 修复旧更新流程过早退出、替换失败后不恢复窗口的问题；普通文件现在保持应用打开进行替换，失败自动回滚，主 EXE 只在末段短暂重启。
- 日常更新不再重复携带 Fixed Version WebView2、Chrome 和未变化的 sealed runtime；WebView2 保持安装器专用基线组件。
- Windows Python 运行时事件统一为 UTF-8/ASCII-safe JSONL，并对异常字节进行容错解码，修复 Bing 每日一图运行失败。
- Studio 文件列表和 RPAZ 构建过滤 Python 字节码缓存。
- Notebook Kernel 初始化和单元执行移入后台 worker，首次运行不再阻塞 WebView；多单元列表和超长代码单元使用分层滚动约束，避免溢出工作区。
- 桌面发布流水线默认只生成轻量 update 与完整合并库存；sealed runtime、Chrome、WebView2、NSIS Setup 和示例只在手工 full 发布时重建。
- 安装库存构建现在拒绝符号链接、目录联接和其他 reparse point，避免完整安装包或本地验收夹具依赖另一个安装目录；全量基线中的 Python、Chrome、uv 与 WebView2 必须是实体文件。

### 计划

- Windows 自动化任务调度、重试和并发策略。
- 更新包签名、更新通道和离线补丁目录。
- RPAZ 能力权限、依赖扩展包和包来源治理。
- AI Agent 流式输出、diff/checkpoint、长期会话与日志/Notebook 工具。

## [0.2.0-preview.10] - 2026-07-14

### 新增

- Windows x64 无注册表 NSIS 引导安装器，支持选择非系统盘目录。
- `.drpa-update` 文件级更新器：清单与 SHA-256 校验、暂存、原子替换、备份、失败回滚和重启。
- 任意页面拖拽一个或多个 `.rpaz` 安装。
- 脚本包右键菜单：运行、复制到工作室、卸载。
- 运行环境页面：查看状态和路径、初始化、验证、修复。
- 工作室直接运行源码、打开已安装包副本和基于名称自动生成项目/包 ID。
- 真实 Jupyter/IPython Kernel：标准输出、执行结果、图片、HTML、错误、变量浏览和 notebook 输出回写。
- Bing 每日一图 `0.2.0` 示例包与独立 SHA-256 文件。

### 修复

- 修复 Windows 离线 Python 误选 `Lib\\venv\\scripts\\nt\\python.exe` 后报 `No pyvenv.cfg file` 的严重问题。运行时现在从受校验清单读取精确解释器路径。
- Windows GUI 和 Jupyter/worker 子进程使用无控制台窗口启动参数，不再先弹出 CMD。
- 修复自定义窗口拖动、缩放、最大化、最小化和关闭交互。
- 修复设置、工作台参数和多个仅展示但没有 Host 行为的入口。

### 交付变化

- 离线运行时和桌面 Release 收敛为 Windows x64；暂不发布 Linux/macOS 安装包。
- GitHub Actions 在最终便携/安装目录结构中初始化 Python 环境并导入运行时、浏览器和 Jupyter 依赖。
- GitHub Actions 启动真实 Jupyter/ZMQ Kernel，连续执行两个单元并校验状态保持。
- 安装脚本 CI 禁止 `WriteReg`、`DeleteReg`、`ReadReg` 和 `InstallDirRegKey`。
- 工作数据固定保存在安装目录的 `data/`，更新器禁止覆盖该目录。

### 已知限制

- preview-10 是热更新基线；更旧版本仍需重新安装一次。
- 当前 `.drpa-update` 由用户在设置中本地选择，尚未自动发现或下载更新。
- 预览包未签名；转移到离线机器前后必须验证配套 SHA-256。
- Jupyter 尚不覆盖远程 Kernel、ipywidgets、VS Code 调试器和完整扩展生态。

[0.2.0-preview.10]: https://github.com/EthanBird/drpa-client/releases/tag/desktop-v0.2.0-preview-10
