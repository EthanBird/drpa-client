# DRPA Local Dify 开发平台

DRPA Local Dify 是面向离线开发与测试的轻量 AI 应用平台。它复用 DRPA 的 Tauri Host、SQLite、插件、Skills、Python Runtime 和日志基础设施，提供 Dify 常用的应用开发闭环，而不是在桌面安装目录内复制一套 Docker 服务栈。

## 当前能力

- Chat 与 Completion 应用创建、编辑、复制式本地开发。
- OpenAI Chat Completions 兼容 Provider。
- Provider URL、Model、上下文、最大输出、流式能力、超时及 Dify 云端映射。
- API Key 与应用 API Token 分离存储，不写入导出的 DSL。
- 实时 SSE 调试、Markdown 渲染、耗时与 Token 摘要。
- 独立 SQLite 运行历史和完整输入输出。
- Dify YAML DSL 导入、原始文件保留、规范化编辑与重新导出。
- Dify Service API：`parameters`、`chat-messages`、`completion-messages` 与 `workflows/run`。
- 本地发布版本与每个应用独立的 Bearer Token。
- 可直接使用插件页导出的 Dify2API 或其他 OpenAI 兼容 Provider。
- Workflow / Chatflow 可视化画布、节点属性、连线、缩放、校验、撤销重做和调试抽屉。
- Start、LLM、Template、If/Else、HTTP、Python Code、Answer、End 本地执行器。
- Dify `workflow.graph` 导入导出和 `/v1/workflows/run` 原生执行。

工作流设计器的节点、IR、执行语义和快捷键见 [Local Dify 工作流设计器](LOCAL_DIFY_WORKFLOW.md)。

## 数据目录

Local Dify 使用“文件保存源码，SQLite 保存运行状态”的结构：

```text
<workspace>/local-dify/
├── apps/
│   └── app-<id>/
│       ├── app.json
│       └── source.dify.yml
├── providers.json
├── secrets.json
└── runtime.sqlite3
```

- `app.json` 是本地规范化应用模型。
- `source.dify.yml` 只在导入 DSL 时生成，用于保留上游原稿和排查兼容问题。
- `providers.json` 不包含 API Key。
- `secrets.json` 保存 Provider Key 与本地 App Token；前端列表只得到 `hasApiKey`。
- `runtime.sqlite3` 保存成功和失败的运行记录。

后续接入凭据保险箱时，`secrets.json` 会迁移为 Secret ID 引用，应用与 Provider DTO 保持稳定。

## Provider 配置

每个 Provider 包含：

```text
id / name
baseUrl / model
contextWindow / maxOutputTokens / temperature
streaming / supportsTools / supportsJson / supportsVision
timeoutSeconds / customHeaders
difyProvider / difyModel
```

`baseUrl` 接受以下形式：

```text
https://api.example.com
https://api.example.com/v1
https://api.example.com/v1/chat/completions
```

Host 会统一解析为 Chat Completions Endpoint。自定义 Header 禁止覆盖 `Authorization` 和 DRPA 路由追踪 Header。

`difyProvider` 与 `difyModel` 只用于云端 DSL 映射。例如本地可以调用某个 OpenAI 兼容网关，而导出时映射成云端已经安装的 Dify Provider 与 Model。

## 调试执行

调试请求经由 Tauri Command 进入 Rust Host：

```text
React Preview
  → run_local_dify_app
  → LocalDifyApp + Provider + Secret
  → OpenAI-compatible /chat/completions
  → local-dify-stream-<requestId>
  → Markdown 实时渲染
  → runtime.sqlite3
```

流式请求会发送 `stream_options.include_usage=true`。Provider 返回标准 OpenAI SSE 时，Host 合并 `choices[0].delta.content` 和最终 usage；阻塞请求读取 `choices[0].message.content`。

每次运行至少记录：

- Run ID、App ID、应用名称。
- Provider ID 与 Model。
- 输入、输出或错误。
- Prompt/Completion Tokens。
- 总耗时和创建时间。

## 本地 Dify Service API

在“API 与导出”页启动本地服务，默认 Endpoint：

```text
http://127.0.0.1:34130/v1
```

服务只监听 `127.0.0.1`。发布应用后生成独立 Token：

```bash
curl http://127.0.0.1:34130/v1/chat-messages \
  -H "Authorization: Bearer APP_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "inputs": {},
    "query": "测试本地应用",
    "response_mode": "blocking",
    "user": "developer"
  }'
```

支持接口：

```text
GET  /v1/health
GET  /v1/parameters
POST /v1/chat-messages
POST /v1/completion-messages
POST /v1/workflows/run
```

`streaming` 响应使用 Dify 风格的 SSE `message` 与 `message_end` 事件。`workflows/run` 执行已发布应用的 Workflow IR，并把最终结果写入 `data.outputs.answer`。

## Dify2API Provider

插件页内置的 `dify2api` 能力包可把 Dify Agent 转换为标准 OpenAI
Chat Completions Provider。启动后，从插件的 Provider 卡片选择“用于 AI
Agent”即可应用 Endpoint、模型名和独立的本地代理 Key。

Local Dify 也可以把该 Endpoint 作为普通 OpenAI Provider 使用，但不要让
Local Dify 应用经 Dify2API 再指回同一个 Local Dify Service；Dify2API
不会复用 Dify `conversation_id`，也不承担工作流路由或递归调用检测。

## DSL 导入导出

导入器检查：

- YAML 根对象。
- `kind: app`。
- `app.name` 与 `app.mode`。
- `model_config.model.provider/name`。
- `pre_prompt`、completion params 与首个输入变量。
- `workflow.graph.viewport/nodes/edges` 与工作流开场白。

导入后保存原始 YAML，同时生成本地规范化应用。若已经存在相同的 `difyProvider + difyModel`，自动绑定该 Provider；否则应用保持“待选择 Provider”。

导出器生成：

```text
version: 0.3.1
kind: app
app: ...
dependencies: []
model_config:
  model: ...
  pre_prompt: ...
  user_input_form: ...
```

Workflow / Chatflow 则生成 `workflow.graph`，节点 `config` 恢复为 Dify `data`，并为每个 LLM 节点注入 Provider 云端映射。

导出前执行兼容性检查：

- 缺少 Provider：error。
- Workflow 图结构、入口、输出路径或悬空连线错误：error。
- 未连接节点或本地执行器尚未覆盖的导入节点：warning。
- 未配置云端 Provider 映射：warning。
- Provider 指向 localhost：warning。

warning 允许导出，上传云端后需要重新选择对应 Provider；error 阻止发布和导出。

## 后端模块

主要实现：

```text
apps/desktop/src-tauri/src/local_dify.rs
apps/desktop/src-tauri/src/local_dify_workflow.rs
```

它同时负责：

- 文件与 Secret 分层。
- SQLite migration 与运行记录。
- OpenAI Provider 调用和 SSE 解析。
- Tauri IPC。
- DSL adapter。
- 本地 HTTP Service 生命周期。
- Provider 路由循环检测。

前端入口：

```text
apps/desktop/src/pages/LocalDifyPage.tsx
apps/desktop/src/components/LocalDifyWorkflowDesigner.tsx
apps/desktop/src/styles/local-dify.css
apps/desktop/src/styles/local-dify-workflow.css
```

页面通过 lazy import 加载，避免影响 DRPA 首屏包体积。

## 后续工作流增强

Workflow IR 已作为执行模型落地，Dify YAML 继续作为导入导出协议。后续增强聚焦 Iteration/Loop 容器、Tool/Knowledge/Agent 节点、并行汇聚、单节点运行、节点级持久化 Trace 和 AI 生成工作流。
