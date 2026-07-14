# DRPA Next 功能扩展路线

本文档只描述未来方向、架构约束和验收门槛，不代表对应功能已经实现。优先级遵循：先保证离线运行可靠，再建设自动化调度，最后引入受控的 AI Agent。

## 1. 长期产品模型

DRPA Next 的目标不是单纯的 Python 启动器，而是本地自动化控制面：

```text
Package registry / local import
              │
              ▼
RPAZ + capability + immutable dependency set
              │
      ┌───────┴────────┐
      ▼                ▼
Manual run        Scheduled / triggered run
      │                │
      └───────┬────────┘
              ▼
Host policy → Runtime adapter → Worker
              │
              ▼
Logs / artifacts / audit / retry state
              ▲
              │ approved tools only
         AI development agent
```

四个稳定扩展点：

1. `RuntimeAdapter`：Python 之外可增加 Node、native command、WASI。
2. `TriggerProvider`：手动、时间、文件、Webhook、事件源。
3. `PackageCapability`：浏览器、文件、网络、桌面、模型等权限。
4. `AgentTool`：把受控的开发和运行操作暴露给 AI，而不是暴露任意 shell。

任何扩展都必须有版本化协议、离线交付方式、失败恢复和审计事件。

## 2. 基础环境与离线能力

### 2.1 当前基线

Windows x64 sealed runtime 已包含固定 CPython、uv、完整 wheel closure、Chrome、Jupyter 依赖和逐文件 SHA-256。最终 venv 在安装位置离线创建，应用不会调用系统 Python 或在线 pip。

### 2.2 下一阶段：能力扩展包

不应把 OCR、桌面自动化、AI 推理等所有依赖塞入基础安装包。建议增加独立 `.drpa-runtime-pack`：

```text
pack.json
files/
licenses/
SBOM.spdx.json
SIGNATURE
```

建议首批能力包：

- `windows-desktop`：UI Automation、截图、剪贴板和受控输入。
- `document`：PDF、Office/OpenXML、图像处理。
- `ocr`：OCR runtime、语言数据和模型。
- `data-science`：NumPy/Pandas 扩展和可选 notebook renderer。
- `local-ai`：可选本地推理 runtime 与量化模型，不进入默认包。

安装键应包含 `runtime version + platform + architecture + lock digest`。相同依赖集合可复用，不同集合不能互相修改。

### 2.3 依赖与供应链

- 所有直接和传递依赖精确锁定，并保存目标平台 wheel。
- CI 使用空缓存、`--offline --no-index` 完成安装和 import smoke。
- 每个包发布 SHA-256、依赖许可证和 SBOM；稳定版再要求签名。
- 不接受运行时临时下载、Git URL、editable 依赖或未固定版本。
- 缺依赖时更新 lock、重新生成完整 Windows bundle、上传 GitHub Release；不能只把单个 wheel 手工拷到用户机器。
- 未来维护内网/离线补丁目录时，目录本身也需要签名索引、回滚版本和过期策略。

### 2.4 热更新演进

当前 `.drpa-update` 已采用安装库存清单和逐文件差量：Release 保留完整 `install-manifest.json`，构建时比较上一版本，只传输变化文件并声明删除项；WebView2 与 Chrome 不再在日常更新中重复。后续扩展：

1. 更新通道清单：`stable`、`preview`、`offline-media`，支持手工导入和可选 HTTPS feed。
2. Ed25519 签名：Host 内置可信公钥，签名覆盖版本、目标、文件散列和最低 Host 版本。
3. 内容寻址块：在“逐文件差量”基础上复用大文件内部未变化块；仍保留完整安装包作为恢复介质。

更新必须支持断电恢复、磁盘空间预检、失败回滚和“永不修改 `data/`”。

## 3. 自动化任务

### 3.1 领域模型

建议新增以下实体，不直接把 cron 字符串塞入现有 run 表：

```text
TaskDefinition
  id, name, package_version, profile_id, enabled
  concurrency_policy, timeout, retry_policy, retention

Trigger
  type, timezone, configuration, next_fire_at

TaskLease
  task_id, owner, acquired_at, expires_at

RunAttempt
  run_id, task_id, trigger_event_id, attempt, reason
```

参数配置引用 package manifest 的 parameter schema；secret 只保存引用，不复制值。

### 3.2 触发器顺序

按风险从低到高实现：

1. 一次性和 interval。
2. 每日/每周及标准 cron，必须显式保存 IANA 时区。
3. 文件到达/目录变化，包含去抖和稳定文件判定。
4. 本地 Webhook，默认只监听 loopback。
5. 系统启动、用户登录和外部消息网关，单独做权限设计。

第一版调度器由 Rust Host 持有，只保证应用运行期间执行。需要“应用未启动也执行”时，应增加独立 Windows service/worker，而不是暗中注册系统任务；安装与启用必须由用户明确选择，并重新审视“无注册表、无管理员权限”承诺。

### 3.3 执行语义

- 并发策略：`allow`、`forbid`、`replace`、`queue-one`。
- misfire 策略：跳过、立即补跑一次、按上限补跑；禁止无限追赶。
- 重试：指数退避、最大次数、可重试错误分类和抖动。
- 超时：先请求优雅取消，再终止完整 Windows Job Object 进程树。
- 幂等：为触发事件生成稳定 idempotency key，重复事件不能重复入队。
- 时间：内部 UTC，展示使用任务时区；覆盖 DST 跳跃与重复小时测试。
- 可观察性：下一次运行、排队原因、尝试次数、触发来源、取消者和产物均可追踪。

### 3.4 UI

- 当前分支已提供自动化计划只读预览页和快照协议字段，用于验证任务列表、下一次运行、最近结果和健康状态的布局；真正的 Host Scheduler、持久化和触发执行仍属于本节后续实现。
- “自动化”页面展示任务、下一次运行、最近结果和健康状态。
- 创建向导分为“选择包/版本 → 参数配置 → 触发器 → 并发/重试 → 权限确认”。
- 右键菜单提供立即运行、暂停、复制、查看历史和删除。
- 失败任务显示可操作诊断，不用只有红色状态点。

### 3.5 验收门槛

- 测试时钟覆盖重启、休眠唤醒、DST、系统时间回拨和连续 misfire。
- 调度数据库崩溃恢复后不重复执行已确认事件。
- 禁用/卸载包会安全暂停引用它的任务。
- 离线机器不依赖外部时间服务或远程队列。

## 4. AI Agent 辅助开发

### 4.1 参考与取舍

[Hermes Desktop](https://github.com/fathah/hermes-desktop) 展示了桌面端统一管理本地/远程 Agent、会话、profiles、memory、skills/tools、计划任务和工具进度的产品形态。DRPA 可以借鉴这些边界与可见性，但 Hermes Desktop 是第三方参考，不是 DRPA 依赖，也不应照搬其完整聊天或网关体系。

DRPA Agent 的首要目标是 **辅助开发和诊断 RPAZ**：生成项目、解释 manifest、运行 notebook、分析失败日志、建议 selector，并在用户批准后执行受控操作。它不是默认获得桌面控制权的通用个人助手。

### 4.2 模块建议

```text
Agent UI
  ├── SessionStore          会话、上下文摘要、成本/用量
  ├── ProviderAdapter       云端或本地 OpenAI-compatible endpoint
  ├── ContextIndexer        项目文件、manifest、运行日志、文档
  ├── Planner               只产出结构化步骤和工具请求
  ├── ToolBroker            schema 校验、权限、批准、超时、审计
  └── Evaluation            回归任务与结果评分
                               │
                               ▼
                     Rust Host approved commands
```

Provider 与工具必须解耦。离线环境可以连接用户预先部署的 loopback 模型服务，或安装可选本地模型包；默认安装不应强塞大模型。

### 4.3 首批 Agent Tools

只开放窄接口，每个工具都有 JSON Schema、风险级别和审计结果：

- `project.list/read_file/write_patch`：写入只能使用可审查 patch，并限制在项目目录。
- `manifest.validate`：返回结构化诊断和修复建议。
- `notebook.execute_cell/restart_kernel`：执行前显示代码；状态与产出进入会话。
- `project.run/cancel`：复用现有 Host 运行链路。
- `run.read_logs/list_artifacts`：默认只读。
- `package.build/install_test_copy`：安装动作必须二次确认。
- `browser.inspect_snapshot`：未来 recorder 提供脱敏 DOM/截图，不直接给任意浏览器控制。

默认禁止：任意 shell、任意路径读写、读取 secret 明文、修改已安装包、安装在线依赖、无提示发送网络请求。

### 4.4 Agent 开发体验

- 项目侧栏提供“解释错误”“生成参数 schema”“为选中代码创建 notebook 实验”“将 notebook 结果整理为 `main.py`”。
- 工具调用以时间线显示输入摘要、权限、输出、耗时和失败原因。
- 支持 checkpoint：应用 patch 前保存项目快照，可逐步撤销。
- 会话绑定项目和 runtime digest，避免环境变化后误用旧结论。
- memory 分为用户偏好、项目事实和临时会话；项目事实必须带来源文件和更新时间。
- AI 生成的 RPAZ 在导出前仍需 manifest、离线依赖、单元测试和示例运行门禁。

### 4.5 安全与隐私

- 首次使用明确选择本地或远程 provider，并展示数据会离开机器的范围。
- 发送前按字段脱敏 secret、cookie、token 和用户定义敏感路径。
- 高风险工具逐次批准；低风险只读工具可按 session 授权。
- 所有工具由 Rust Host 执行，模型输出永远不能直接成为命令行。
- prompt injection 视为不可信输入；网页、日志和包 README 不能改变系统工具策略。
- 提供会话导出与完整删除；离线模式下不得发 telemetry。

### 4.6 验收门槛

- 对越权路径、符号链接、恶意 manifest、prompt injection 和 secret 泄漏做红队测试。
- Agent 生成的 patch 可预览、可撤销、可归因。
- 相同固定模型/参数和测试夹具能重复通过核心 RPAZ 开发任务。
- Provider 不可用时 Studio、Notebook 和手工运行仍完整可用。

## 5. 分阶段交付

| 阶段 | 目标 | 完成定义 |
| --- | --- | --- |
| P0 稳定基线 | Windows runtime、安装、更新、RPAZ、Jupyter | 独立测试机连续升级和恢复演练通过；关键 UI 无空操作 |
| P1 离线扩展 | 签名清单、SBOM、runtime packs | 基础包与扩展包可独立校验、安装、回滚 |
| P2 自动化任务 | 应用内调度、重试、并发、历史 | 时间/重启/misfire 测试通过，无重复或丢失事件 |
| P3 Agent MVP | 项目问答、patch、manifest、notebook、run tools | 权限与审计完备，离线本地 provider 可用 |
| P4 高级自动化 | recorder、Windows desktop、事件触发 | capability policy 与进程隔离经过安全验证 |
| P5 生态 | 私有包源、签名发布、团队策略 | 包来源、升级和撤销具备端到端信任链 |

每个阶段都应更新 `CHANGELOG.md`、本路线状态、用户文档和 CI 验收，不允许只交付 UI 占位。
