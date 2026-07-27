# 轻量 RPAZ AI Agent

DRPA Next 在“基础设施 → AI Agent”提供面向 RPAZ 开发的单 Orchestrator 对话入口。ProviderAdapter 负责模型连接，ToolRegistry 统一发现内置工具、Skills 2.0 工具和插件工具；插件服务由 Rust PluginSupervisor 管理。

## 配置与会话

- `OpenAI 兼容 URL`：支持标准 `/v1` 根地址，也支持已经包含 `/chat/completions` 的地址。
- `Model`：由用户填写 provider 提供的模型名。
- `API Key`：可选，只存放在当前 React/Tauri 会话内，不进入 localStorage、项目文件、日志或命令行。本地无鉴权 provider 可留空。
- `开发项目`：可选。绑定后工具被限定在该 Studio 项目目录；绑定关系按对话分别保存。
- URL、model、配置面板显示状态、对话列表和消息记录写入本地 UI 偏好。最多保留 50 个对话，每个对话保留最近 120 条消息；支持新建、切换、自动命名、重命名、清空和删除。
- API key 与正在执行的请求状态不进入持久化数据。Agent 执行期间锁定会话切换，避免响应写入错误会话。

离线机器可连接预先部署在内网或 loopback 的 OpenAI-compatible provider。桌面安装包不携带模型，也不执行在线模型或 Python 包下载。

## 执行链路

```text
React Agent UI
  → run_agent_turn (async Tauri command)
  → AgentOrchestrator
      ├─ OpenAiCompatibleAdapter
      └─ ToolRegistry
          ├─ Built-in tools
          ├─ Skills 2.0 executable tools
          └─ Plugin tools
  → OpenAI-compatible /chat/completions 或本地 Plugin Provider
  ↔ 默认最多 64 轮、可配置 1–256 轮 function tool calls
  → assistant message + tool timeline + token/duration metadata
```

工具结果以 `role=tool` 和原始 `tool_call_id` 回送模型。Host 最多读取最近 120 条历史，并按配置的上下文窗口估算和裁剪；单消息上限 100 KiB、单工具回送模型的输出上限 20 KiB。模型请求与本地工具执行都离开 Tauri UI 线程。

## Skills 与插件

- Skills 2.0 是包含 `skill.yaml`、`instructions.md`、工作流、资源、代码和测试的能力包。
- Skill 可以直接声明 sealed Python 工具、插件内命令工具以及需要加入模块搜索路径的代码库。
- 插件可以导出 Tool、ProviderAdapter 或长期运行的本地服务。
- Agent 请求携带稳定的本地 Session ID，Dify 等会话型 Provider 可以据此维护远端 `conversation_id`。
- 详细清单和执行约定见 [Skills 2.0 与插件系统](SKILLS_AND_PLUGINS.md)。

## 内置工具

| 工具 | 行为 |
| --- | --- |
| `knowledge_list_documents` | 列出本地 Markdown 知识目录和文档，无需绑定项目 |
| `knowledge_read_document` | 读取知识库 UTF-8 Markdown，单篇最大 8 MiB |
| `knowledge_write_document` | 原子创建/覆盖 Markdown，并按安全相对路径创建父目录 |
| `rpaz_list_files` | 列出项目文件，忽略缓存、字节码与符号链接 |
| `rpaz_read_file` | 读取项目内 UTF-8 文本，单文件最大 2 MiB |
| `rpaz_write_file` | 在项目内创建或覆盖 UTF-8 文件，单文件最大 2 MiB |
| `rpaz_validate` | 使用 `drpa-package` 校验 schema 2 manifest 与入口文件 |
| `rpaz_build` | 校验后在工作区 `build/` 生成 `.rpaz` |
| `rpaz_python` | 使用 sealed Python 在项目目录执行辅助代码，30 秒超时 |

路径先经过 `safe_relative_path`，再校验目录链和符号链接。知识工具限定在数据目录 `knowledge/`；RPAZ 工具限定在绑定项目。工具不读取凭据保险箱、不访问安装包目录、不暴露任意 Tauri command，也没有通用 shell。

## 产品参考与取舍

- [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling)：采用“模型请求 → 工具调用 → 应用执行 → 工具结果 → 最终消息”的标准循环。
- [OpenAI Chat Completions API](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)：作为兼容 provider 的最小协议面。
- [OpenCode](https://github.com/anomalyco/opencode)：参考对话、项目上下文与工具事件的开发者工作流。
- [Hermes Agent tools](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/tools.md)：参考 toolset 与本地代码执行的可见性；DRPA 只保留 RPAZ 所需窄工具。
- [Harness](https://github.com/harness/harness)：参考基础设施产品的信息组织方式，不引入其平台服务或部署依赖。

## 后续迭代

1. 写入前 diff/checkpoint、逐次撤销、会话导入导出与跨设备同步。
2. 复用 Notebook 执行、运行日志和产物只读工具。
3. Provider profile、模型能力探测、代理与自定义 header。
4. 长期服务的双向 `Content-Length` JSON-RPC 与取消/进度通知。
5. 固定夹具 Skill/Agent evaluation，覆盖 manifest 修复、参数生成、插件崩溃和 Dify 工具桥。

Agent 执行预算、持久 Jupyter Kernel 与 DrissionPage 浏览器调试的具体改造方案见
[`AI_AGENT_JUPYTER_BROWSER_DESIGN.md`](AI_AGENT_JUPYTER_BROWSER_DESIGN.md)。
