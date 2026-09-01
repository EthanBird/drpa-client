# GUI 测试记录

## 2026-08-05 Studio 多页签与凭据保险箱

- 浏览器预览以 1280×720 截图复核开发工作室：同时打开 `main.py` 与 `manifest.yaml` 后出现两个独立页签，当前页签和关闭按钮清晰，快捷键提示位于页签条右侧，编辑区未溢出。
- 凭据保险箱覆盖初始化总览、Google Authenticator 二维码/手动密钥、一次性恢复码和三栏管理工作区四种状态；主工作区在 1280×720 下完整显示分类、列表、详情和服务控制，没有黑色缺失配色或遮挡。
- 浏览器控制台四个状态均为 0 error / 0 warning。
- 截图保存在 `target/v2.0.2-screenshots/`：`studio-tabs.png`、`vault-onboarding.png`、`vault-totp-setup.png`、`vault-recovery.png`、`vault-workspace.png`。
- Testing Library 新增多页签保存/关闭快捷键、TOTP 初始化与恢复码一次性展示回归；Rust 新增 RFC 6238、密文往返/锁定和恢复码轮换测试。

## 2026-07-14 · Markdown 知识库回归

- 构建并启动 `target/debug/drpa-desktop.exe`，实际 Host 在 `target/debug/data/knowledge/RPAZ 开发指南/` 首次生成 11 篇 Markdown；逐文件确认存在，`03_ctx上下文与默认配置.md` 为 UTF-8 且内容完整。
- Windows Computer Use 已启动无边框 Tauri 测试窗口，但截图辅助返回 `SetIsBorderRequired failed: 不支持此接口 (0x80004002)`，因此停止该控制链路；这是测试辅助对窗口捕获的兼容问题，未观察到 DRPA 崩溃。
- 隐藏启动 Vite 后使用应用 GUI 浏览器预览回归，避免再弹出控制台窗口。1280×720 下知识库 header、270px 文件树、阅读区和底部状态栏没有溢出。
- 通过 GUI 验证：从“知识文档”入口打开、相对 Markdown 链接跳转、编辑模式、分栏实时渲染、GFM 任务列表、代码块、内联新建并自动补 `.md`、目录名称搜索。
- AI Agent 未绑定项目时显示 `3 ACTIVE`，`knowledge_list_documents`、`knowledge_read_document`、`knowledge_write_document` 可见；绑定项目后合计 9 个工具。
- 浏览器控制台 warning/error 为 0。GUI 预览结束后关闭测试标签和隐藏 Vite 服务，没有保留可见控制台。
- 自动化门禁：Vitest 15/15、desktop Rust 12/12、workspace Rust 全部通过、Python 5/5；`cargo clippy --workspace --all-targets -- -D warnings` 通过。

## 2026-07-14 全量运行时实体文件回归

- 复现：本地 `target/update-debug/v2-gui/install` 曾使用指向旧安装目录的 Junction 复用 `runtime/python`、`runtime/browser`、`runtime/tools` 与 `webview2`。删除旧安装后，运行环境页报告缺少 `runtime/browser/chrome-win64/chrome.exe`。
- 根因：测试夹具不是自包含安装，库存生成器又会跟随 Windows Junction，因而没有在装配阶段暴露外部依赖。
- 修复：本地夹具重新放入实体 CPython 3.11.9、Chrome for Testing 150.0.7871.115、uv 0.11.28、59 个离线 wheels 与 Fixed Version WebView2 150.0.4078.65；删除并重建生成环境。
- 防回归：`build_update_package.py` 改用不跟随链接的目录遍历，并拒绝 symlink、Junction 与其他 reparse point。新增 Windows Junction 回归测试，修复前失败、修复后通过。
- 实际安装布局冒烟测试通过：59 个包纯离线安装、`drpa_runner/DrissionPage/ipykernel/jupyter_client/zmq/nbformat/pandas/openpyxl` 导入、Chrome 本地 HTML 无头访问以及连续两次 Jupyter Kernel 执行均成功。
- 新库存包含 4,626 个实体文件：4,056 个 runtime 文件、308 个 Chrome 文件与 260 个 WebView2 文件；新程序进程路径确认为 `G:\workspace\drpa\target\update-debug\v2-gui\install\DRPA Next.exe`。
- 新构建的可访问性树确认运行环境页显示“运行环境健康”“Python、Jupyter Kernel 与浏览器自动化依赖已就绪”、Python 3.11.9、Bundle 0.3.0-dev 及安装目录内的 Chrome for Testing 路径，不再出现缺失文件提示。

日期：2026-07-14
环境：Windows，本地 `npm run tauri:dev`，真实 Tauri/WebView 窗口。

## 已验证流程

- 总览页：空工作区可正常加载，Host 显示已连接。
- 自动化计划页：可从命令面板进入；真实空 Host 数据目录下显示空态。
- 脚本包页：无已安装包时显示空态与安装/新建入口。
- 开发工作室：
  - 可新建中文名称项目。
  - 项目 ID 自动生成，热重启后项目仍能恢复。
  - `main.py`、`manifest.yaml`、`notebook.ipynb` 文件列表正常。
  - 真实 Tauri 可访问性树可见“新建文件”“新建文件夹”“导入文件”三个文件操作入口。
  - Studio 直接运行成功后，运行记录页出现成功记录，进度为 100%。
- Bing 每日一图：使用真实示例目录和 `zh-CN`、`image_count=1` 参数运行，事件序列完整到 `completed(exit_code=0)`，产出 JSON 元数据和 JPG 图片。
- 前端回归：右键文件菜单、创建并选中任务配置、命令面板 Enter 执行均由 Vitest/Testing Library 覆盖。

## 发现并已修复

1. **Studio 直接运行失败，提示 `stream did not contain valid UTF-8`**
   - 现象：开发模式直接运行新建项目时，Python worker 输出中文 JSONL 后 Host 读取失败。
   - 根因：Windows Python 子进程 stdout 使用本地代码页，协议通道要求 UTF-8。
   - 修复：启动 Studio Kernel 和 worker 时设置 `PYTHONUTF8=1`、`PYTHONIOENCODING=utf-8`；debug 模式优先使用仓库 `.venv` Python。

2. **运行后 Studio 文件列表显示 `__pycache__/main.cpython-311.pyc`**
   - 现象：项目运行后 Python 字节码缓存进入 Studio 文件树，存在被导出进 `.rpaz` 的风险。
   - 修复：项目文件收集时过滤 `__pycache__`、`.pyc`、`.pyo`。

3. **命令面板无法搜索进入“运行记录”，且输入框 Enter 不执行首个结果**
   - 现象：搜索“记录”无结果；搜索后需要额外 Tab 才能选择命令。
   - 修复：新增“打开运行记录”命令；输入框按 Enter 执行首个匹配命令。

4. **开发工作室缺少文件管理入口**
   - 修复：文件面板增加右键菜单和顶部快捷按钮，支持新建文件、新建文件夹、文件选择导入、浏览器文件拖拽，以及 Tauri 原生路径拖拽导入。
   - 回归：项目文件列表现在包含目录项，并继续过滤 `__pycache__`、`.pyc`、`.pyo`。

5. **脚本包参数没有显式默认值，运行工作台缺少“创建任务配置”**
   - 修复：manifest 参数模型、Host 摘要协议和前端模型贯通 `default/defaultValue`；Bing 示例显式声明 `market=zh-CN`、`image_count=1`。
   - 修复：运行工作台可以创建本地任务配置、持久化参数、重置到 manifest 默认值，并复用脚本包的运行 profile 提交任务。

## 仍需后续设计确认

- 自动化计划页当前是只读预览。真实 Host 数据只有带 schedule 的 profile 才会派生计划；现有示例 `.rpaz` 没有 schedule，因此该页在真实桌面 Host 中为空。
- 当前阶段“新建计划”按钮保持 disabled，后续 P2 Scheduler 需要补创建向导、持久化、触发器计算和执行语义。
- Computer-use 截图接口在该 Tauri 窗口上返回 `SetIsBorderRequired failed`，本次使用可访问性树检查真实窗口，并用前端集成测试补齐右键菜单交互覆盖；这属于测试工具与窗口捕获兼容性记录，不影响应用运行。

## 2026-07-14 热更新与资源管理回归

### 真实开发版窗口

- 通过 `npm run tauri:dev` 启动 `target/debug/drpa-desktop.exe`，确认命令面板可在真实 WebView 窗口切换页面。
- 设置页可访问性树确认：默认亮色说明、亮色/暗色单选控件、`Windows 轻量热更新` 卡片与“选择更新包”入口均已出现；紧凑/舒适布局入口已移除。
- 开发工作室可访问性树确认：项目列表、项目项、新建文件、新建文件夹、导入文件均已出现；项目删除、文件内联新建/重命名/删除由 Testing Library 回归覆盖。
- 运行日志的分级、搜索、跟随与 UTF-8 容错标记由前端回归和 Rust 事件测试覆盖；当前开发数据目录没有已安装脚本包，因此真实窗口进入的是工作台空状态。

### 测试工具问题

1. Windows.Graphics.Capture 对该无边框 Tauri 窗口仍返回 `SetIsBorderRequired failed: 不支持此接口 (0x80004002)`；可访问性文本读取正常。
2. 该窗口的 UIA 点击/赋值接口分别返回“先调用 get_window_state”和 `read UIA value read-only state ... 0x80070057`，所以未通过坐标猜测继续操作；对应交互使用真实 Tauri 页面可访问性检查与 11 项 Vitest 回归交叉验证。

## 2026-07-14 Notebook、当前用户与 AI Agent 回归

### 真实桌面与 GUI

- 构建并启动最新 `target/debug/drpa-desktop.exe`，确认真实 Tauri 主窗口出现；Windows.Graphics.Capture 仍在该无边框窗口返回 `SetIsBorderRequired failed: 不支持此接口 (0x80004002)`，因此没有继续对该窗口注入输入。
- 使用同一 React 构建的 Chrome 本地 GUI 进行可见交互回归：基础设施导航出现 AI Agent，URL/model/key/项目配置、空态建议、消息提交和工具列表均正常，控制台无 error/warning。
- 账户卡片显示 gateway 返回的“本地用户 / 本机用户”；总览问候也已移除固定示例名并复用当前用户来源。
- GUI 创建“Notebook GUI 回归”项目，进入 `notebook.ipynb`，连续添加 11 个代码单元。实测 `.notebook-scroll` 为 `clientHeight=426`、`scrollHeight=1220`，垂直滚动已生效；工作区底部、滚动区底部和任务输出顶部均为 662/663 px，没有挤出主 UI。
- Agent 绑定该项目后，界面明确显示 `6 ACTIVE`，六个 `rpaz_*` 工具均可见。

### 本轮发现并修复

1. **浏览器预览创建 Studio 项目后立即从列表消失**
   - 根因：mock gateway 的 `createStudioProject` 返回项目，但 `listStudioProjects` 永远返回空数组，创建后的 refresh 覆盖前端状态。
   - 修复：mock gateway 增加内存项目与文件存储，覆盖创建、读取、写入、目录、重命名和删除，浏览器 GUI 现在可以完整演练 Studio → Notebook → Agent 项目绑定。

2. **总览仍显示固定示例问候名**
   - 修复：总览和左下角账户卡片都读取 `get_current_user`；浏览器预览使用“本地用户”，真实桌面使用 Windows `USERNAME`。

3. **Notebook 首次执行阻塞与多单元溢出**
   - 修复：Kernel 预热与 execute command 改为 Tauri async + blocking worker；React 在 invoke 前先绘制运行态。
   - 修复：补齐 Studio/Notebook 的 grid containment、`min-height: 0`、内部滚动和 toolbar 横向滚动；单个 Monaco 代码单元最高 420 px。
   - 回归：Vitest 使用 30 单元 Notebook 和延迟执行 Promise，验证滚动容器存在且“正在运行”在执行完成前已渲染。

## 2026-07-14 热更新闪退诊断

- 检查用户现有安装目录后确认其没有 `install-manifest.json` 和受管更新器，属于 protocol-2 之前的安装基线；Release 14 轻量包却依赖 Release 13 库存，旧版与新版之间没有可靠的协议迁移边界，这是“退出后未更新”的主要兼容性缺陷。
- 直接 Worker harness 曾出现“completed 后子进程消失”，但同一新主程序从安装目录和会话目录直接启动都能稳定存活；该现象受测试执行器的 Windows Job 生命周期影响，因此错误工作目录只作为加固项，不再记录为唯一根因。
- 修复：`0.3.0` 建立 schema-2 全量基线；轻量包明确声明 Host/Worker protocol、最低 Host 版本和精确 baseVersion，不兼容时保持旧应用打开并要求全量安装。
- 修复：包内 Worker 优先脱离父 Job，写入 ready 后才允许 Host 退出；新版本从安装目录启动并在主窗口创建后写入 startup ack，Worker 在收到确认前保留备份，早退/超时则结束新进程、回滚并恢复旧版本。
- 回归：Rust 测试覆盖协议拒绝、暂存大小检查、安装目录、启动确认环境、子进程早退与回滚；另执行 schema-2 真实双版本更新演练。
- 更新读取取消逐文件 SHA-256 校验，保留 schema、平台、基线版本、安全路径、文件数量、压缩率和写入大小检查；日常 Release 资产缩减为 `.drpa-update` 与 `install-manifest.json`。
