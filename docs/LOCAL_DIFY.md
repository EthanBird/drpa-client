# DRPA Local Dify 开发平台

DRPA Local Dify 是面向离线开发与测试的轻量 AI 应用平台。它复用 DRPA 的 Tauri Host、SQLite、插件、Skills、Python Runtime 和日志基础设施，提供 Dify 常用的应用开发闭环，而不是在桌面安装目录内复制一套 Docker 服务栈。

## 第一阶段能力

- Chat 与 Completion 应用创建、编辑、复制式本地开发。
- OpenAI Chat Completions 兼容 Provider。
- Provider URL、Model、上下文、最大输出、流式能力、超时及 Dify 云端映射。
- API Key 与应用 API Token 分离存储，不写入导出的 DSL。
- 实时 SSE 调试、Markdown 渲染、耗时与 Token 摘要。
- 独立 SQLite 运行历史和完整输入输出。
- Dify YAML DSL 导入、原始文件保留、规范化编辑与重新导出。
- Dify Service API：`parameters`、`chat-messages`、`completion-messages` 与 `workflows/run`。
- 本地发布版本与每个应用独立的 Bearer Token。
- Local Dify → Dify Loves Hermes → 远程 Dify 的 Provider 链路。

Chatflow 与 Workflow DSL 在第一阶段可以导入并保留；可视化画布和图执行器进入第二阶段。

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

`streaming` 响应使用 Dify 风格的 SSE `message` 与 `message_end` 事件。`workflows/run` 在第一阶段把 Chat/Completion 结果包装为 `data.outputs.answer`，方便 API 客户端和桥接插件进行开发测试。

## Dify Loves Hermes 套娃链路

`dify-loves-hermes` 0.3 会透传：

```text
X-DRPA-Trace-Id
X-DRPA-Provider-Route
X-DRPA-Hop-Count
```

因此可以配置：

```text
DRPA AI Agent
  → Dify Loves Hermes
  → DRPA Local Dify Service
  → OpenAI-compatible Provider
```

也可以配置：

```text
DRPA Local Dify
  → Dify Loves Hermes
  → Dify Cloud App API
```

Local Dify 在调用 Provider 前把当前 App ID 加入路由。重复 App ID 或超过四跳时终止执行并写入失败运行记录，避免配置形成无限调用环。

## DSL 导入导出

导入器检查：

- YAML 根对象。
- `kind: app`。
- `app.name` 与 `app.mode`。
- `model_config.model.provider/name`。
- `pre_prompt`、completion params 与首个输入变量。

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

导出前执行兼容性检查：

- 缺少 Provider：error。
- Chatflow/Workflow 尚未进入图执行器：error。
- 未配置云端 Provider 映射：warning。
- Provider 指向 localhost：warning。

warning 允许导出，上传云端后需要重新选择对应 Provider；error 阻止发布和导出。

## 后端模块

主要实现：

```text
apps/desktop/src-tauri/src/local_dify.rs
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
apps/desktop/src/styles/local-dify.css
```

页面通过 lazy import 加载，避免影响 DRPA 首屏包体积。

## 下一阶段

第二阶段在同一应用模型上增加 Workflow IR：

1. Start、LLM、Template、If/Else、Variable、Answer。
2. Python Code、HTTP、SQL 和 DRPA Tool 节点。
3. 节点画布、变量检查、单节点运行和完整 trace。
4. Dify Workflow Graph 导入导出 adapter。
5. Portable 与 DRPA Enhanced 两种兼容模式。

Workflow IR 作为执行模型，Dify YAML 作为导入导出协议，避免运行时与某一个 Dify DSL 版本强耦合。
