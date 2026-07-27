# AI Agent、持久 Jupyter 与 DrissionPage 浏览器调试设计

## 1. 结论

DRPA 不应限制一段对话只能进行固定轮数。当前实现允许每个本地会话保留最近 120 条
消息；单次用户请求内部的“模型 → 工具 → 模型”循环现已改为默认 64 轮，并允许用户
配置 1–256 轮。这个限制不是整段对话的消息数限制。

建议分三层改造：

1. 以当前可配置的默认 64 轮为基础继续完善执行预算，同时由工具调用数、
   运行时间、上下文余量、重复调用和用户取消共同约束。
2. 把现有 Studio Jupyter bridge 抽成可复用的 Host Kernel Service，为每个 Agent 会话
   提供独立、持久、可中断的 Python 内核。
3. 在持久内核之上提供 DrissionPage 调试会话和紧凑页面快照；Skill 只负责编排方法，
   内核、浏览器、超时、路径和进程生命周期必须由 Rust Host 控制。

当前 64 轮配置解除了最直接的功能瓶颈；后续仍需补齐取消、逐轮上下文核算、软停止和
浏览器清理，否则更长的循环会放大这些问题。

## 2. 当前实现事实

### 2.1 会话与单次执行是两个概念

- React 本地最多保存 50 个 Agent 会话。
- 每个会话最多保存最近 120 条 `user/assistant` 消息。
- `maxRounds` 默认 64、Host 允许 1–256，只约束 `run_agent_turn()` 内部的一次工具循环。
- 工具时间线保存在 assistant 消息的 `tools` 字段中，但下一次请求只回传
  `role/content`，详细工具结果不会重新进入上下文。

因此需要分别命名：

- **Conversation turn**：用户发一条消息并得到一条最终回复。
- **Model round**：同一个 conversation turn 内的一次模型请求。
- **Tool call**：某个 model round 返回的一个 function call；一轮可以包含多个。

产品文案和遥测都应使用这三个术语，不能再用含义模糊的“轮”。

### 2.2 当前循环的主要风险

1. 每轮开始前不重新计算新增 tool result 对上下文的占用。
2. 达到第 8 轮后直接返回错误，没有最后一次关闭工具的总结机会。
3. 没有 `cancel_agent_run`，模型请求、Python 和后续循环不能统一取消。
4. 没有 wall-clock、总工具调用数和重复调用熔断。
5. 运行状态只在当前调用和 UI 内存中，异常退出后不能从 checkpoint 恢复。
6. `rpaz_python` 是 30 秒一次性进程，不能保存 import、变量或浏览器对象。

### 2.3 已有 Jupyter 基础可以复用

项目已经包含：

- `StudioKernelManager`：按 Studio 项目持有 bridge 进程。
- `drpa_runner.kernel`：使用 `jupyter_client`、IPython kernel 和 ZMQ。
- execute、complete、inspect、变量预览和标准 Jupyter MIME 输出。
- sealed Python、DrissionPage 和 Chrome for Testing。
- `DRPA_BROWSER_PATH` 注入和项目根目录作为工作目录。

Hermes 的 Jupyter skill 值得借鉴的是“用多次紧凑 execute 保持 Python 状态”，而不是
照搬它启动无鉴权 JupyterLab HTTP 服务的部署方式。DRPA 已经有更窄的 Host ↔ JSONL
bridge，不需要额外开放 8888 端口、REST session 和 WebSocket。

## 3. 目标与非目标

### 3.1 目标

- 长任务在合理预算内持续工作，而不是在第 8 个 model round 突然失败。
- 用户随时可以停止，停止后仍能看到已完成步骤和最后状态。
- Agent 可以在同一 Python 内核中反复试验、观察变量、修正代码。
- Agent 可以保持一个 DrissionPage 页面对象，完成“观察 → 操作 → 再观察”的调试闭环。
- 调试成功后把逻辑收敛到项目 `.py` 文件，再执行 `rpaz_validate` 和 `rpaz_build`。
- 所有进程、路径、输出、浏览器 profile 和下载均受 Host 生命周期管理。

### 3.2 非目标

- 不直接控制用户日常 Chrome profile。
- 不把任意 shell 暴露给 Agent。
- 不让 Skill 自己启动常驻进程或绕过 Host 路径校验。
- 不把完整 HTML、截图 base64 或无限 stdout 填入模型上下文。
- 不保证 Notebook 内存状态就是可交付的 RPAZ；最终项目必须能在新进程中运行。

## 4. 总体架构

```text
React Agent UI
  ├─ conversation / run / cancel / resume
  ├─ model round、tool call、预算与浏览器状态
  └─ 调试产物、截图和 notebook trace
          │
          ▼
Rust AgentRunManager
  ├─ ExecutionPolicy / BudgetGovernor
  ├─ OpenAI-compatible ProviderAdapter
  ├─ ToolRegistry
  ├─ AgentKernelManager
  └─ Run checkpoint / event stream
          │
          ├──────────────► 普通 Host tools
          │
          ▼
drpa_runner.kernel (每个 Agent session + project 独立)
  ├─ IPython / jupyter_client / ZMQ
  ├─ 持久 namespace
  ├─ DRPA browser helper
  └─ DrissionPage → bundled Chrome for Testing
```

Studio Kernel 与 Agent Kernel 默认隔离。这样 Agent 的试验不会污染用户正在手工执行的
Notebook，也不会与 Studio UI 抢同一条同步 JSONL 通道。以后可以增加显式的
“附加到 Studio Kernel”模式，但不能作为默认行为。

## 5. 用执行预算代替固定 8 轮

### 5.1 请求策略

在 `AgentTurnRequest` 中增加：

```ts
interface AgentExecutionPolicy {
  maxModelRounds: number;       // 默认 64，UI 范围 1..256
  maxToolCalls: number;         // 默认 64，Host 硬上限 256
  maxWallTimeMs: number;        // 默认 600_000，Host 硬上限 1_800_000
  maxRepeatedCall: number;      // 相同 name + canonical args 默认最多连续 3 次
  contextReserveTokens: number; // 默认 max(8192, maxOutputTokens * 2)
}
```

Host 必须继续设置不可由 WebView 放大的硬上限。UI 的“快速 / 标准 / 深度”可以映射到
策略预设，同时保留高级设置：

| 预设 | model rounds | tool calls | wall time |
| --- | ---: | ---: | ---: |
| 快速 | 16 | 32 | 2 分钟 |
| 标准 | 64 | 128 | 10 分钟 |
| 深度 | 128 | 256 | 20 分钟 |

默认使用“标准”。SQL 助手无工具，不受这组循环预算影响。

### 5.2 每轮执行顺序

每个 model round 开始前按以下顺序检查：

1. 用户是否取消。
2. wall-clock 是否耗尽。
3. model round 和 tool call 预算是否耗尽。
4. 是否连续重复同一工具及参数。
5. 当前 system、history、assistant tool calls、tool results 是否仍能放入上下文。
6. 必要时压缩旧 tool result，再构造 provider payload。

达到软限制时，不直接报错。应关闭 tools，再做一次不计入工具预算的 finalization call，
要求模型说明：

- 已完成什么；
- 哪一步未完成；
- 当前文件、浏览器或内核状态；
- 用户继续时可以从哪里恢复。

只有 provider 失败、Host 不变量被破坏或强制终止时才返回错误。

### 5.3 停止原因与结果

`AgentTurnResult` 增加：

```ts
type AgentStopReason =
  | "completed"
  | "cancelled"
  | "round_budget"
  | "tool_budget"
  | "time_budget"
  | "context_budget"
  | "repeated_tool_call"
  | "provider_error"
  | "tool_error";
```

同时返回 `modelRounds`、`toolCalls` 和 `checkpointId`。UI footer 应显示当前运行的实际
预算和消耗，例如“模型 7/24 · 工具 13/64 · 01:42”，而不是固定宣传 8 轮。

### 5.4 上下文和历史

每轮都必须重新估算上下文，而不只是进入循环前估算一次。压缩顺序建议为：

1. 保留 system、最新用户请求和最近未完成的 tool call/result。
2. 旧工具输出替换为结构化摘要：工具名、状态、关键结果、修改文件和产物路径。
3. 更早的 conversation turns 使用滚动摘要。
4. 始终保留最近一次用户和 assistant 的原文。

不要在下一轮对话中回放所有原始 stdout 或 HTML。应该回传一个小型 working state：

```json
{
  "changedFiles": ["main.py", "manifest.yaml"],
  "artifacts": ["agent/.../page.png"],
  "browser": {"url": "...", "title": "...", "open": true},
  "kernel": {"executionCount": 12, "variables": ["page", "rows"]},
  "lastValidation": {"ok": true}
}
```

## 6. Agent Kernel Service

### 6.1 先抽公共服务，再接 Agent

把目前 `lib.rs` 中的 `StudioKernelManager`、spawn、JSONL request/response 和清理逻辑
移动到独立模块，例如：

```text
apps/desktop/src-tauri/src/kernel_service.rs
runtime/python/src/drpa_runner/kernel.py
```

Rust 侧使用通用 key：

```rust
enum KernelOwner {
    Studio { project_id: String },
    Agent { session_id: String, project_id: String },
}
```

一个 kernel 同一时刻只执行一个请求。不要在持有全局 sessions mutex 时等待整个
Jupyter 执行；应先取得对应 session 的独立锁，否则一个长单元会阻塞所有项目和 Agent。

### 6.2 内置工具

第一阶段只需要四个 Host 工具：

| 工具 | 用途 |
| --- | --- |
| `jupyter_execute` | 在当前 Agent kernel 执行代码，状态跨调用保留 |
| `jupyter_variables` | 获取名称、类型和短预览，不执行用户自定义 `repr` 的无限输出 |
| `jupyter_interrupt` | 中断当前执行但保留 kernel |
| `jupyter_restart` | 结束并重建 kernel，清空状态 |

`jupyter_execute` 参数建议：

```json
{
  "code": "string",
  "timeoutSeconds": 30,
  "purpose": "简短说明本次试验要验证什么"
}
```

timeout 可配置为 1..120 秒。超时先发送 Jupyter interrupt，等待宽限期后再终止 bridge
和 kernel。当前 `kernel.py` 的 IOPub 循环缺少整体 deadline，这一点必须先修复。

结果必须紧凑：

- stdout 最多 16 KiB；
- stderr 最多 8 KiB；
- text/plain 最多 16 KiB；
- traceback 最多 30 行；
- 变量最多 100 个，每个 preview 最多 240 字符；
- image/HTML 等 MIME 保存为本地 artifact，模型只得到路径、类型、尺寸和短说明。

### 6.3 Notebook trace

不要求运行 JupyterLab server，但应把每次 `jupyter_execute` 的 code、紧凑输出、时间和
执行计数追加到 Agent run checkpoint。用户选择“保存调试 Notebook”时，再转换成标准
nbformat 文件。默认存放在数据目录：

```text
agent/sessions/<session-id>/
  runs/<run-id>.jsonl
  artifacts/
  browser-profile/
  downloads/
  scratch.ipynb        # 用户显式保存后生成
```

这些文件不能混入 RPAZ 构建归档。

## 7. DrissionPage 调试层

### 7.1 浏览器对象必须留在持久内核

一次性 `rpaz_python` 无法可靠持有 `ChromiumPage`。Agent kernel 启动时继续注入
`DRPA_BROWSER_PATH`，并提供一个小型 helper：

```python
browser = drpa.start_browser(headless=False)
page = browser.page
```

helper 负责：

- 使用 bundled Chrome for Testing；
- 为 Agent session 创建独立 profile 和下载目录；
- 默认 headed，便于用户观察调试过程；
- 只创建一个受管主页面，重复 start 返回当前会话；
- 在 restart、cancel、删除会话和应用退出时关闭浏览器；
- 对截图、下载和导出文件执行 containment check。

不要默认连接用户的日常 Chrome profile。未来如支持外部 CDP，应作为明确的高级模式，
并显示正在接管哪个 endpoint/profile。

### 7.2 两层工具面

保留 `jupyter_execute` 作为低层逃生口，同时增加面向 LLM 的紧凑浏览器工具：

| 工具 | 行为 |
| --- | --- |
| `browser_open` | 创建/复用受管浏览器并导航到 URL |
| `browser_snapshot` | 返回标题、URL、可见文本和带 ref 的交互元素 |
| `browser_act` | 对 ref 执行 click/input/select/hover/press |
| `browser_screenshot` | 保存截图为 artifact，返回路径和尺寸 |
| `browser_close` | 关闭当前受管浏览器 |

DrissionPage 没有直接等同于浏览器 accessibility tree 的标准快照，因此
`browser_snapshot` 可以在页面中执行受控 JS，收集可见的 button、a、input、select、
textarea、role、label、placeholder 和邻近文本，并给本次快照分配 `@e1`、`@e2`。
ref 到 locator/element 的映射保存在内核 helper 中；导航或 DOM 大幅变化后要求重新
snapshot，避免使用过期元素。

完整 HTML 仅在明确调试选择器时由 `jupyter_execute` 小范围读取。默认快照要限制元素
数量和文本长度，并优先返回交互元素。

### 7.3 推荐调试循环

Skill 应指导 Agent 使用固定闭环：

1. `browser_open` 打开目标页。
2. `browser_snapshot` 观察当前状态。
3. `browser_act` 完成一个小动作。
4. 再次 snapshot，验证页面确实变化。
5. 需要复杂解析、监听网络或试验 DrissionPage API 时使用 `jupyter_execute`。
6. 把验证过的定位器和纯函数写入项目 `.py`，不要只留在内核变量中。
7. 重启 kernel 或关闭 browser 后，用 RPAZ 直接运行验证可重现性。
8. 调用 `rpaz_validate`；需要交付时再调用 `rpaz_build`。

连续多个动作不经观察直接执行，会放大页面漂移和错误定位，应由 Skill 明确禁止。

## 8. Skill 的职责边界

建议新增内置 Skill `drissionpage-browser-debug`，使用当前 Skills 2.0 结构：

```text
agent/skills/drissionpage-browser-debug/
  skill.yaml
  instructions.md
  references/
    drissionpage-patterns.md
```

Skill 的 description 应覆盖“网页自动化、页面交互调试、选择器试验、表单流程、网络监听、
截图验证和把 Notebook 探索收敛为 RPAZ”等触发词。

`instructions.md` 只保留上述调试闭环、浏览器清理、重启验证和敏感信息规则。详细的
DrissionPage API 示例放入 reference，按需读取，避免把整本手册放进 system prompt。

`jupyter_*` 和 `browser_*` 必须是 Host 内置工具，不能声明为普通 Skill Python tool。
现有 Skill tool 每次通过独立 sealed Python 入口执行，天然无法保存 kernel 和
DrissionPage 对象，也无法可靠响应统一取消。

## 9. 安全与授权

### 9.1 URL 和网络

- 只允许 `http`、`https`。
- 默认拒绝 `file:`、`javascript:`、`data:`、`chrome:` 等协议。
- 公网、loopback、局域网和企业内网应有可见策略；RPA 场景不能简单永久禁止内网。
- 可配置域名 allowlist，跨域导航在事件流中显示。

### 9.2 敏感操作

以下动作应成为可识别事件，并允许产品层增加确认：

- 上传本地文件；
- 下载并执行文件；
- 提交包含密码、token、支付或个人信息的表单；
- 删除、发布、发送消息等不可逆网页动作；
- 连接用户已有 CDP/profile。

截图、DOM 和日志中发现 password input、Authorization、cookie 等内容时应脱敏。模型
不应收到浏览器 profile 路径中的凭据文件或原始 cookie。

### 9.3 进程和资源

- Agent session 删除、kernel restart、应用退出和超时必须回收 kernel/Chrome 子进程。
- 取消应先 interrupt，再按宽限期 terminate，最后强制 kill。
- 每个 session 限制一个浏览器和有限 tab 数。
- downloads、profile、artifact 必须在该 session 数据目录内。

## 10. UI 建议

1. 把 footer 改为“本次预算：标准 · 最多 24 次模型循环 / 64 次工具调用 / 10 分钟”。
2. 运行中展示 Stop 按钮，而不是锁死整个页面且无法取消。
3. timeline 区分 model round 与 tool call，并显示当前预算。
4. 增加 Kernel 状态：未启动、就绪、执行中、中断、已重启。
5. 增加 Browser 状态：URL、标题、headed/headless、打开 tab 数。
6. 提供“重启调试环境”“关闭浏览器”“保存为 Notebook”。
7. 达到预算时显示“已暂停并总结”，提供“继续此任务”，不要显示成普通失败。

## 11. 实施顺序

### Phase 0：解除不合理限制

- 增加 `AgentExecutionPolicy` 和 Host 硬上限。
- 默认 64 model rounds，并继续增加 tool calls / wall-clock 预算。
- 达到软限制后执行无工具 finalization。
- 增加 `cancel_agent_run`。
- 每轮重新计算上下文预算。
- 修正文案和 telemetry。

### Phase 1：持久 Python

- 抽取 `kernel_service.rs`。
- Studio 与 Agent 使用独立 owner/key。
- 增加 execute、variables、interrupt、restart。
- 修复 Jupyter 整体 timeout 和全局 mutex 阻塞。
- 增加紧凑输出、artifact 和 run checkpoint。

### Phase 2：浏览器调试

- 增加受管 DrissionPage helper 和 session profile。
- 实现 open、snapshot、act、screenshot、close。
- 新增 `drissionpage-browser-debug` Skill。
- UI 展示浏览器和 kernel 状态。
- 增加应用退出、取消和异常后的 Chrome 回收测试。

### Phase 3：长期任务质量

- 滚动 conversation summary 和 working state。
- resume from checkpoint。
- 对固定 RPAZ/浏览器任务建立评测集。
- 按完成率、平均 model rounds、重复调用率、取消延迟、遗留进程数调节默认预算。

## 12. 最小验收标准

1. 同一对话可持续超过 8 个 conversation turns。
2. 单次复杂任务可完成至少 12 个 model rounds，不因固定 8 轮失败。
3. 达到预算时得到可读总结和可继续 checkpoint。
4. Stop 在 2 秒内触发取消状态；长 Python 在宽限期后被中断或终止。
5. 两次 `jupyter_execute` 之间变量和 DrissionPage page 对象保持。
6. snapshot → act → snapshot 能稳定验证页面变化。
7. Agent 和 Studio 默认使用不同 kernel，互不污染。
8. kernel/browser 重启后，生成的 RPAZ 在全新进程直接运行通过。
9. 取消、删除会话和应用退出后没有遗留 kernel 或 Chrome。
10. 模型上下文中没有完整 screenshot base64、cookie、password 或无限 HTML/stdout。

## 13. 参考

- Hermes Agent Jupyter skill：
  <https://hermes-agent.nousresearch.com/docs/user-guide/skills/optional/data-science/data-science-jupyter-notebook>
- Hermes Agent browser automation：
  <https://hermes-agent.nousresearch.com/docs/user-guide/features/browser>
- hamelnb：
  <https://github.com/hamelsmu/hamelnb>
- DrissionPage 4 文档：
  <https://www.drissionpage.cn/dp40docs/>
