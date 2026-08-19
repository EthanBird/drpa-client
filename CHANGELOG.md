# 更新日志

本项目从 `0.2.0` 开始维护面向开发者和发布使用者的变更记录。格式参考 Keep a Changelog；预览版可能继续调整内部协议，稳定版发布前必须明确迁移策略。

## [Unreleased]

### UOS 插件服务

- UOS 包修订提升为 `2.1.1-1+uos20.4`；Dify2API gateway 升级至 1.0.2，DRPA 会监督用户已启动的插件服务，在 UOS 偶发异常退出后按退避策略自动恢复，显式停止或禁用仍保持停止。
- Linux 子进程退出现在记录真实信号编号而不是笼统的 `-1`；Dify2API 收到 `SIGTERM` 或中断时也会在关闭前记录原因。

## [2.1.1] - 2026-08-17

### Dify2API

- Dify2API 1.0.1 的上游客户端允许自签名和内部 CA HTTPS 证书，修复 UOS 内网 Dify 连通性测试被 TLS 信任链拦截并返回 502 的问题。

### 光学文件传输

- 桌面端新增“手机零安装”入口，以二维码直达 GitHub Pages 网页版，并保留发送、接收模式快捷切换。
- 重构接收端高帧率解码与喷泉码尾部恢复流程，修复全部帧已接收却停在 99% 的问题。

### 发布

- 产品版本统一提升为 `2.1.1`，继续发布注册表无关的 Windows x64 完整离线 Setup 与 UOS Desktop 20 x86_64 专用 deb。
- Windows 与 UOS 均沿用完整包覆盖升级，用户数据目录保持不变。

### AI Agent

- Provider 网络超时改为分阶段握手时限与 Agent Run 总预算：持续产生事件的长流不再被固定 120 秒 `global` 时限中断，取消与最大运行时长继续由统一 `AgentRunControl` 管理。
- MiniMax Token Plan/OpenAI-compatible 请求自动启用 `reasoning_split`；SSE 与普通 JSON 会把 `reasoning_content`、`reasoning_details` 及 `<think>` 标签内容保留在模型上下文通道，同时只把最终答案送入 Markdown 正文。
- JCode sidecar 更新至 `0.74.0`，并在 DRPA 的 NDJSON adapter 增加推理标签隔离，覆盖旧 Provider 输出和已有兼容接口差异。

### 开发工作室

- Python 文件的“代码 / 流程图”切换器移出可横向滚动的文件页签区，固定显示在编辑区右上角；入口明确标记为 `Python Flow · Beta`，多页签时仍保持可见。

## [2.1.0] - 2026-08-10

### 开发工作室与 Python Flow

- 新建开发项目和从 RPAZ 包创建的开发副本会补齐 `README.md`，记录 manifest、`ctx`、离线依赖、测试、导出及 AI 协作顺序；已有用户 README 保持原内容。
- 新增 Python Flow Beta：通过静态 AST 把 `main(ctx)` 投影为开始、调用、`ctx`、RPA、条件、循环、异常、返回和原始代码节点，支持画布拖拽、自动布局、撤销重做、属性编辑、源码定位与结构校验。
- Python 源码继续作为唯一事实源；流程图写回当前 Monaco 页签并复用保存、直接运行、运行记录和 RPAZ 导出链路。未编辑往返保留关键字参数、RPA 别名、返回表达式、注释和空行。

### 离线自动化运行时

- sealed runtime 固定内置 `rpa==1.50.0`、`tagui==1.50.0` 和 Windows/Linux/macOS 平台 TagUI 引擎，运行任务时通过 Host 管理的 `DRPA_RPA_HOME` 与 `DRPA_RPA_BUNDLE` 完成离线初始化。
- 新增 RPA 源码包、平台资产、Delta 文件、生成 wheel 与最终引擎包的 SHA-256、字节数和提交锁；Windows 和 Linux 发布门禁验证安装布局、来源清单及 RPA/Python Flow 导入。

### 发布

- 产品版本提升至 `2.1.0`，发布 Windows x64 Setup、Linux runtime-complete AppImage、现代 deb 与 UOS Desktop 20 专用 deb 全量离线包。
- 继续采用全量升级策略，不生成 `.drpa-update`。

### UOS 交互与插件服务

- UOS 固定根启动器彻底隔离目标系统的 GTK/Fcitx/AT-SPI 动态模块，使用 GTK 内置简单输入上下文与 X11 core events，修复 UOS 20 Professional 1070 中输入框、弹窗、工作区菜单及多数自定义控件聚焦后页面停滞的问题。
- UOS 门禁注入 DDE 风格的 `GTK_MODULES`、`GTK3_MODULES`、Fcitx 和系统 `GTK_PATH` 污染，并读取真实 Host 进程环境确认启动器完成清理。
- Dify2API 启动前检测监听端口；遇到旧 sidecar 或其他本机进程占用时自动分配并保存新端口。Linux 子服务绑定父进程生命周期，启动错误会附带已脱敏的 stdout/stderr 摘要。
- 自动化、流程设计、Agent、知识库、插件、凭据保险箱、工作区与数据库等业务 Tauri command 改为异步调度，避免文件、SQLite、加密和进程操作占用 GTK/WebKit 主线程；新增静态策略测试防止同步 command 回归。

## [2.0.4] - 2026-08-09

### BI 与工作区状态

- BI 栅格改为以组件排序关系驱动的自适应布局：拖动只调整目标排序，布局引擎再根据组件尺寸从左到右、从上到下自动填充，减少无关卡片位移。
- 开发工作室、知识文档等需要保留现场的页面在导航切换时维持挂载状态，继续保留未保存编辑内容、打开页签与阅读位置。

### AI Agent

- Agent 运行、取消、会话修订、浏览器租约、工具权限与 Provider 调用收敛到统一 Host 生命周期，RPAZ Agent、JCode 和 Local Dify 复用一致的运行事件与资源回收模型。
- 补强上下文预算、工具协议、会话并发、文档附件、浏览器隔离和 Python 子进程边界测试。

### UOS 与插件服务

- UOS 页面导航仅保留必要的持久工作区，释放非活跃瞬态页面，并减少系统指标轮询造成的 WebKitGTK 主线程压力。
- Dify2API 可执行服务与封装 Python 初始化解耦；在 UOS 会话运行目录中暂存静态 sidecar，兼容用户数据分区的 `noexec` 挂载。
- loopback 健康检查与调试请求固定直连，避免系统代理影响本机服务探测；UOS 容器门禁新增真实 Dify2API `/healthz` 启动验证。

### 发布与验证

- 产品版本统一提升为 `2.0.4`，发布 Windows x64 全量离线 Setup、Linux runtime-complete AppImage、现代 deb 与 UOS Desktop 20 x86_64 专用 deb。
- 全量包继续携带封装 Python 3.11、Chrome/Jupyter 运行环境；不生成增量更新包。

## [2.0.3] - 2026-08-05

### BI 主页

- 编辑模式改为指针驱动的实时栅格拖拽；组件进入已占用位置时优先交换到原空位，并在复杂碰撞中寻找最近可用位置。
- 拖动、缩放、删除和响应式列投影统一通过无重叠布局引擎处理，自动压缩纵向空白并保持组件在栅格边界内。
- 被让位组件使用轻量 FLIP 动画过渡，并遵循系统的减少动态效果设置。

### 开发凭据保险箱

- 修复手动新增凭据时，在名称、账号、Secret、地址或备注输入框输入内容会因延迟读取 React 事件对象而触发页面白屏的问题。
- 新增完整表单连续输入回归，覆盖六个字段、动态标题和页面挂载状态。

### 发布与验证

- 产品版本统一提升为 `2.0.3`；发布 Windows x64 全量离线 Setup，并同步构建 UOS Desktop 20 x86_64 专用 deb。
- UOS 包继续执行密封运行时、glibc 2.28/Deepin 20.8、真实输入、IPC、WebKitWebProcess 和截图门禁。
- 桌面端 54 项测试、TypeScript、Vite 生产构建和真实浏览器交互验证通过；凭据表单验证期间控制台无 error/warning。

## [2.0.2] - 2026-08-05

### 开发工作室

- 源码和 Notebook 支持 VS Code 风格多文件页签；每个页签独立保存内容、已保存基线、加载与错误状态，项目切换时保留当前项目的打开集合。
- 新增 `Ctrl/Cmd+S` 当前保存、`Ctrl/Cmd+Shift+S` 项目全部保存、`Ctrl/Cmd+W`/`Ctrl/Cmd+F4` 关闭和 `Ctrl/Cmd+Tab` 正反向切换；关闭脏页签会显示保存/放弃/取消确认。
- 文件、目录和项目重命名/删除同步更新页签；Notebook 使用真实相对路径持久化，不再固定写入 `notebook.ipynb`。

### 开发凭据保险箱

- 新增 Bitwarden 风格本地凭据工作区，支持登录账号、API Key、令牌、数据库、SSH、安全笔记、收藏、标签、搜索、显隐复制、编辑和删除确认。
- 初始化采用 Google Authenticator 兼容 `otpauth://` 二维码、RFC 6238 6 位 TOTP 和 10 分钟 setup；恢复码只展示一次并支持下载，恢复解锁后立即轮换。
- 完整 Payload 使用随机 Vault Key 与 AES-256-GCM 加密；Windows 通过当前用户 DPAPI 保护 TOTP seed/device secret，恢复 key-wrap 独立于设备保护，密钥会话退出、锁定或 24 小时到期后清除。
- 新增显式启动的 `127.0.0.1` 凭据读写 API，TOTP verify 后签发随机 Bearer Token，限制请求体且禁止缓存；手动锁定或会话到期立即使 Token 失效。
- RPAZ Agent 与 JCode MCP 新增 `vault_list_credentials`、`vault_get_credential`、`vault_upsert_credential`；敏感读取只进入当前模型工具回合，持久工具事件脱敏。

### 发布与验证

- Windows 版本统一提升为 `2.0.2`，只发布全量离线 Setup、安装库存和 Bing 示例，不生成增量更新包。
- 新增 RFC TOTP、密文往返/锁定、恢复码轮换、Agent MCP 工具、多页签快捷键与初始化恢复 UI 回归；1280×720 截图检查全部通过且浏览器控制台无 error/warning。

## [2.0.1] - 2026-08-05

### Windows 浏览器与 Agent

- `ctx.browser()` 改为连接工作区级持久 DrissionPage 会话：有界面与 headless 使用独立调试端口和持久 Profile，任务成功、失败或包内调用 `page.quit()` 后均保留 Chrome，供开发工作室和后续 RPAZ 任务复用。
- 每次连接后重新应用当前任务的 `output/downloads` 下载目录；嵌套 `ctx.invoke()` 与父包共享浏览器句柄登记，运行日志明确提示浏览器已保留。
- AI Agent 新增模式切换：内置 RPAZ Agent 继续使用 DRPA ToolRegistry 与分类权限，JCode 开发者 Agent 使用完整开发工具集并复用 OpenAI-compatible Provider、流式 Markdown 和 DRPA 会话。
- Windows 轻量更新和全量安装流水线固定打包经 SHA-256 校验的 JCode sidecar；密钥只通过子进程环境传递，不写入 JCode 配置文件。

## [2.0.0] - 2026-07-19

> 本次正式 Release 仅发布 Windows x64 全量离线安装包；Linux 发行资产继续保持在 `desktop-v1.0.0`。

### Local Dify 开发平台

- 基础设施新增“AI 应用”：本地管理 Chat/Completion 应用和 OpenAI-compatible Provider，支持 Prompt、模型参数、流式 Markdown 调试和 Provider 连接测试。
- Local Dify 源码按应用写入工作区文件，运行记录写入独立 SQLite；Provider API Key 与应用 API Token 从公开配置和 DSL 中分离。
- 新增本地 Dify Service API，支持 `/parameters`、`/chat-messages`、`/completion-messages`、`/workflows/run`、blocking 与 SSE streaming。
- 新增 Dify YAML DSL 导入、原始 DSL 保留、规范化导出和云端 Provider 映射检查。
- `dify-loves-hermes` 升级到 0.3，透传 Trace、Provider Route 和 Hop Count；Local Dify 检测重复 App 与四跳上限，支持本地/远程 Dify 套娃调试。
- 新增 Workflow / Chatflow 可视化设计器：节点库、拖放画布、连线、条件分支端口、属性面板、高级 JSON、缩放、适应画布、撤销重做、校验与快捷键。
- 新增版本化 Workflow IR 与旧应用迁移，支持 Start、LLM、Template、If/Else、HTTP、Python Code、Answer、End 本地执行、节点级流式事件和实时调试轨迹。
- Dify DSL 适配扩展到 `workflow.graph` 节点/连线/坐标导入导出；`/v1/workflows/run` 复用本地图执行器。

### Windows 开发体验与数据能力

- 运行记录改为 SQLite 持久化，支持应用重启后继续查看完整概览、事件时间线、参数、产物与错误回溯；异常退出时未结束任务标记为 interrupted。
- 开发工作室和 Notebook 代码单元接入标准 Jupyter `complete_request`，通过项目级 IPython/Jedi 命名空间提供离线 Python 补全。
- Runtime Context 新增 `ctx.sql`，支持 SQLite 参数化执行、批量写入、字典查询、标量读取、事务和嵌套 savepoint。
- 新增自有数据工作台：本地 SQLite 对象树、表结构检查、Monaco SQL 编辑、`Ctrl+Enter`、结果网格、TSV 复制和查询历史；DBX 仅作为交互研究参考，不引入其运行时代码。
- 数据工作台新增 PostgreSQL/MySQL 连接注册、会话级密码、TLS、连接测试、远程对象树与统一结果网格；连接文件不写入密码。
- 数据工作台新增 AI SQL 助手：使用当前连接 schema 和全局 OpenAI-compatible 配置，支持流式 Markdown、追加/替换编辑器且不自动执行。
- Jupyter 编辑体验新增标准 `inspect_request`，为 Python 文件与 Notebook 提供实时悬停文档和函数参数提示。
- 运行详情日志支持全文/级别筛选、复制，产物可复制完整路径。

### Linux UOS Desktop 20 兼容包

- 修复 UOS 中首次聚焦任意输入框后 WebView 页面完全失去点击响应的问题。UOS 包修订提升为 `1.0.0-2+uos20.3`，launcher 固定 GTK 使用 XIM 桥，避免私有 Ubuntu GTK 在 UOS 上自动连接不匹配的 IBus/Fcitx D-Bus IM 模块；继续继承系统 `XMODIFIERS` 以使用 DDE/Fcitx 的中文输入服务。
- 新增真实输入交互门禁：Xvfb 中由 `xdotool` 打开命令面板、物理点击输入框、逐键输入 sentinel、关闭面板并点击侧栏按钮；前端只有在输入事件与后续按钮事件均完成、延迟计时器仍运行且 Tauri IPC 往返成功后才写出 marker。
- 撤销“run 29551221688 的 Deepin 截图已证明 UI 可读”的结论：该图顶部文字区深色像素比例为 `0`，只能证明 React/WebKit 绘制了无文字的页面结构。字体已在真实 UOS 上由用户确认正常，因此不增加包内字体；Debian 10 测试必须通过 Noto CJK 可见文字与顶部连通组件门禁，固定 Deepin 20.8 镜像若没有可用系统字体，则单独要求非白屏布局和真实输入/后续页面响应，并在指标中记录 `font_available=0`。
- 修复 UOS/Fantasy II-M 上 `WebKitWebProcess` 因 `MESA-LOADER: failed to open swrast`、`EGL_NOT_INITIALIZED` 退出后主窗口永久白屏的问题。UOS 包修订提升为 `1.0.0-2+uos20.2`，私有携带 GLVND、Mesa EGL、GLX、glapi、swrast/kms_swrast 与 llvmpipe 的完整 `DT_NEEDED` 闭包，并强制 WebKitGTK 使用隔离的软件渲染路径。
- 原“Xvfb 运行 20 秒不退出”门禁被认定不足并替换：前端必须在 workspace snapshot 成功后完成两帧绘制并通过 Tauri IPC 写入就绪标记；Debian 10 与 Deepin 20.8 用户态还会检查 `WebKitWebProcess` 持续存在、扫描 EGL/swrast 致命日志并验证实际截图。存在系统字体时要求灰度标准差不低于 0.03 并检查文字区组件；无字体的最小镜像仍要求至少 32 色、灰度标准差不低于 0.01，以及输入后页面与 IPC 继续响应。截图、进程树、窗口树和日志始终作为 Actions 诊断资产保存。
- 新增 `drpa-next-1.0.0-linux-x86_64-uos20.deb`，最低系统基线为 x86_64、glibc 2.28，覆盖 UOS Desktop 20 Professional（eagle）与同代 Debian 10 用户态。
- UOS 包固定安装到 `/opt/drpa-next-uos20`，随包携带 glibc 2.35 动态加载器、libc/NSS、libstdc++ 与 libgcc；构建器为桌面 Host、WebKit helper、Python、uv、Chrome 等全部动态 ELF 写入私有解释器和传递型 `DT_RPATH`，解决现代 WebKitGTK 的 `GLIBC_2.35`、`GLIBCXX_3.4.30` 与 `CXXABI` 缺口。
- 启动器不全局导出 `LD_LIBRARY_PATH`，避免私有 libc 污染 `xdg-open` 等 UOS 系统程序；WebKitGTK 所需的 GBM、通用 libdrm、GLVND、Mesa EGL/GL 与软件 DRI 进入私有层，只有内核与 X11 server 继续由目标系统提供。
- 新增机器可读 UOS deb manifest、私有运行库来源/散列库存和 ELF 解释器/RPATH 校验。构建器递归解析 `DT_NEEDED`，把 Fribidi、X11/XCB、ALSA、字体、NSS、Mesa/LLVM 等用户态依赖补入私有层；包的 `Depends` 不含系统 WebKitGTK、C++ runtime、GBM/libdrm、EGL 或 GL，只保留 glibc 2.28 基线与 `xdg-utils`。
- UOS 固定根启动器按 AppImage `AppRun` 语义切换到包内 `usr/` 工作目录，使生产版 WebKitGTK 重定位后的 Network/Web helper 与 injected bundle 相对路径正确解析，并禁用旧 Mesa 不可靠的 DMABUF renderer；helper 仍使用包内解释器和传递型私有 RPATH。
- Linux 发布门禁新增 Debian 10 与 Deepin 20.8/glibc 2.28 容器：真实安装 UOS deb，在 Debian 基线未安装 `libwebkit2gtk-4.1-0`、并在启动前屏蔽系统 Mesa DRI 路径的条件下完成离线 runtime bootstrap、Jupyter/NumPy/Pandas/debugpy 等原生扩展导入、Chrome headless、React/IPC/像素级 UI、卸载和用户数据保留验证。
- run `29551221688` 证明了私有 Mesa 修复白屏和 WebKitWebProcess 退出，但其 Deepin 20.8 PNG 没有任何文字，不能作为完整 UI 验收；对应 `uos20.2` 包及 SHA-256 仅保留为历史诊断记录，不再视为当前可用发布。
- 最终 `uos20.3` 由 [Linux run 29564213354](https://github.com/EthanBird/drpa-client/actions/runs/29564213354) 构建并发布：Debian 10 截图为 2103 色、标准差 `0.0480815`、12 个标题字形组件；Debian 与 Deepin 的 `nativeInputTyped`、`postInputClick`、`ipcRoundTrip` 均为 `true`。UOS deb 为 340,090,572 字节，SHA-256 `ecefbe51b1375bab2f89e567c1eab121bcf5c3fba98da5f4ab30d8d4c380e59c`。
- 新增 `docs/UOS20_PACKAGING.md`，集中记录目标系统、依赖分层、私有 ELF 运行层、可复现构建、静态/容器门禁、已解决故障和 UOS 4.19 实体机回归清单。

## [1.0.0] - 2026-07-15

### Linux x86_64 发布

- deb 包修订为 `1.0.0-2`：不再使用 Tauri 默认的系统 WebKitGTK 依赖布局，而是从已验证 AppImage AppDir 生成 `/opt/drpa-next` 私有运行时，内含 WebKitGTK 4.1、JavaScriptCoreGTK、GTK、GStreamer、NSS、Soup 与 WebKit helper process。
- 现代 Linux deb 的发布门禁会在打包后卸载 Runner 的 `libwebkit2gtk-4.1-0`，确认 `Depends` 只保留 glibc/libgcc/libstdc++ 与驱动相关 EGL/GL/GBM 基础运行库、安装不会重新拉取 WebKitGTK，并在该状态下完成 X11 启动与卸载数据保留测试；UOS 专用 deb 另行验证私有 GBM。
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
