# AI Agent：Chrome Bridge 与 DRPA Host 工具

## 目标

DRPA 的两种 Agent 模式共用同一套本地能力：

- **RPAZ Agent**：工具直接注册到内置 OpenAI-compatible tool loop。
- **JCode 开发者 Agent**：DRPA 在每次运行前生成 `JCODE_HOME/mcp.json`，通过 stdio MCP 暴露同名工具。
- **RPAZ 任务**：`ctx.browser()`、RPAZ Agent、JCode 都连接固定调试端口与持久化用户目录。

整个 Bridge 随安装包离线提供，不执行浏览器组件下载，也不依赖 Firefox 扩展。JCode 自带的 `browser` 工具会被隐藏，避免触发 `browser setup`。

## Chrome 会话模型

默认可见浏览器使用：

| 配置 | 默认值 |
| --- | --- |
| 调试端口 | `9222` |
| 用户目录 | `<workspace>/browser/drissionpage/visible` |
| Chrome | 离线 runtime manifest 中的 `browser_executable` |

浏览器进程不会在 Agent 回合或 RPAZ 任务结束时退出。这样可以保留失败现场、登录态和已打开页面，也允许多个任务复用浏览器启动成本。

## 浏览器工具

- `browser_open(url)`：在内置 Chrome 打开页面。
- `browser_snapshot(maxChars?)`：返回标题、URL、可见文本及交互元素 `ref`。
- `browser_click(ref)`：点击快照中的元素。
- `browser_type(ref, text, submit?)`：输入文本，可选提交表单。
- `browser_wait(seconds?, text?)`：按时间或页面文本等待。
- `browser_screenshot(fullPage?)`：写入当前 Agent 会话的浏览器产物目录。
- `browser_status()`：读取端口、profile、URL 与标题。

页面元素引用通过 `data-drpa-agent-ref` 注入当前 DOM；同一页面内保持稳定，页面跳转后应重新调用 `browser_snapshot`。

## RPAZ 与运行记录工具

- `rpaz_list_packages()`：列出已安装 RPAZ 包、参数和任务配置。
- `rpaz_run_package(packageId, profileId?, parameters?)`：调用桌面 Host 的正式运行入口。
- `run_list(packageId?, status?, limit?)`：读取运行记录摘要。
- `run_get_detail(runId)`：读取完整结构化事件、日志、进度、错误和产物。

`rpaz_run_package` 不会绕过 Host 直接执行 Python。它使用与运行工作台相同的 `prepare_run` 和后台执行路径，因此任务会实时出现在运行记录中。

## JCode MCP 与 Host Bridge

JCode 是外部进程，不能直接持有 Tauri `State`。DRPA 在回合期间启动随机端口的 loopback Host Bridge，并把随机 token、地址和 Chrome 配置写入 MCP 子进程环境。MCP 的 RPAZ/运行记录工具通过 Bridge 回到主进程；浏览器工具直接连接持久化 Chrome。

Host Bridge 只监听 `127.0.0.1`，随 JCode 回合关闭；token 每次随机生成。API Key 仍只通过环境变量传给 JCode，不写入 `config.toml` 或 `mcp.json`。

## 故障定位

1. `browser_status` 检查内置 Chrome 路径、端口和 profile。
2. `rpaz_list_packages` 确认包 ID 与 profile ID。
3. `rpaz_run_package` 获取 `runId`。
4. 运行中重复调用 `run_get_detail(runId)`，查看结构化 runtime 事件和 debug 日志。
5. 页面交互失败时重新 `browser_snapshot`，不要复用跳转前的元素引用。
