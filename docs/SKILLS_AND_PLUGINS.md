# Skills 2.0 与插件系统

DRPA 将 Agent 扩展能力拆为三个层次：

```text
Skill      声明任务匹配、提示、流程、资源、代码和工具
Tool       向模型暴露带 JSON Schema 的可调用能力
Plugin     提供独立工具、Provider 适配器或受 Host 管理的后台服务
```

## Skills 2.0 包结构

每个 Skill 位于工作区 `agent/skills/<id>/`：

```text
example-skill/
├─ skill.yaml
├─ instructions.md
├─ tools/
│  └─ example.py
├─ lib/
├─ workflows/
├─ resources/
└─ tests/
```

`skill.yaml` 的 schema 为 2：

```yaml
schema: 2
id: example-skill
name: Example Skill
version: 1.0.0
description: 说明能力包适用的任务。
activation:
  intents: [分析项目]
  file_patterns: ['*.json']
permissions:
  workspace_read: true
  workspace_write: false
  network: false
tools:
  - name: summarize_json
    description: 汇总 JSON 文件中的记录
    runtime: python
    entry: tools/summarize.py:run
    timeout_seconds: 30
    parameters:
      type: object
      properties:
        path: { type: string }
      required: [path]
      additionalProperties: false
libraries:
  - path: lib
    language: python
```

Agent ToolRegistry 将该工具注册为：

```text
skill_example-skill__summarize_json
```

### Python 工具约定

Python 工具使用随 DRPA 安装的 sealed Python，不下载运行时依赖。入口函数支持以下两种签名：

```python
def run(arguments):
    return {"value": arguments}


def run(arguments, context):
    return {
        "project": context.get("projectRoot"),
        "arguments": arguments,
    }
```

`context` 包含：

- `skillRoot`
- `workspaceRoot`
- `projectRoot`
- `permissions`

`libraries` 中声明的目录或模块会加入该工具进程的 `sys.path`。工具返回值必须可 JSON 序列化。Host 负责参数传递、超时、输出上限、进程退出和错误归一化。

`permissions` 同时作为能力声明和运行上下文传入工具，便于工具自行收敛行为；当前版本没有提供操作系统级进程隔离，因此可执行 Skill 按本地可信代码管理。

### Command 工具约定

`runtime: command` 的 `entry` 必须位于 Skill 目录内。Host 通过 stdin 传入：

```json
{
  "arguments": {},
  "context": {
    "skillRoot": "...",
    "workspaceRoot": "...",
    "projectRoot": "..."
  }
}
```

程序通过 stdout 返回一个 JSON 对象。stderr 作为诊断信息记录；超时后终止子进程。

### 旧版迁移

工作区中的 `SKILL.md` 会在初始化时迁移为 `skill.yaml + instructions.md`。旧文件保留，避免覆盖用户内容。设置页内置 Skills 能力工作区：用目录树浏览能力包，以 Monaco 编辑 Python/YAML/JSON/Markdown，并支持创建、重命名、删除文件与子目录。`skill.yaml` 和 `instructions.md` 是受保护的包根文件。

## 插件包

插件安装目录为 `plugins/<id>/`，离线安装文件扩展名为 `.drpa-plugin`，内容是根目录带 `plugin.yaml` 的 ZIP：

```text
plugin.yaml
config.schema.json
service/
tools/
bin/
resources/
```

清单 schema 为 1。新清单按“能力”组合，不要求插件只能是某一种固定模板：

```yaml
schema: 1
id: example-provider
name: Example Provider
version: 1.0.0
description: 带工具和调试器的本地模型适配服务
types: [provider-adapter, service, tool-provider, debugger]
config_schema: config.schema.json
services:
  - id: gateway
    title: OpenAI Gateway
    primary: true
    runtime: executable
    entry_by_platform:
      windows: service/gateway.exe
      linux: service/gateway
    args: ["--config", "{state}"]
    transport: http
    endpoint: http://{config.listen_addr}/v1
    health:
      kind: http
      endpoint: http://{config.listen_addr}/healthz
      startup_timeout_seconds: 15
providers:
  - id: openai
    protocol: openai
    service_id: gateway
    endpoint: http://{config.listen_addr}/v1
    model_config_key: model
    api_key_config_key: proxy_api_key
tool_providers:
  - id: local-tools
    runtime: bundled-python
    entry: tools/provider.py
    tools:
      - name: lookup
        description: 查询本地数据
        callable: lookup
        parameters: { type: object, properties: {} }
debugger:
  endpoints:
    - id: health
      title: 服务健康
      method: GET
      endpoint: http://{config.listen_addr}/healthz
  panels:
    - id: traffic
      title: 请求链路
      kind: structured-log
default_config:
  listen_addr: 127.0.0.1:34122
  model: local-model
  proxy_api_key: ""
```

同一个插件可以自由组合：

- `provider-adapter`
- `service`
- `tool-provider`
- `debugger`
- 后续扩展的 importer、exporter 和安全 UI panel

服务 runtime 支持：

- `bundled-python`：使用 sealed Python，以 `-I` 模式运行入口脚本。
- `executable`：执行插件自带的当前平台程序。

一个插件可以声明多个服务；Host 按清单顺序事务式启动，任一服务失败会停止本次已经启动的全部服务。插件进程使用隐藏窗口启动。声明 `health` 后，Host 会在返回“运行中”前等待服务真正就绪；超时或提前退出会终止进程并呈现启动错误。`entry_by_platform` 用于选择 Windows、Linux 或 macOS 入口。

清单字符串支持 `{state}`、`{pluginRoot}` 和 `{config.<key>}` 模板。Host 保存每项服务的状态、最近 1000 行 stdout/stderr、结构化事件与导出的 Endpoint；日志按原始字节容错解码，不因单行无效 UTF-8 终止服务管理。

## 插件工具

插件工具的声明和调用约定与 Skill 工具相同，工具名注册为：

```text
plugin_example-provider__tool_name
```

`tool_providers[]` 可以让一项进程入口导出多个工具；旧版平铺 `tools[]` 仍兼容。插件 Python 工具入口可以读取 `context.config`。插件禁用后，它导出的工具立即从 Agent ToolRegistry 消失。

## 声明式调试器

`debugger.endpoints[]` 声明调试器允许调用的 HTTP 请求。Host 只接受插件清单内的 loopback 地址、限制请求与响应大小，并在宿主端从配置注入 Bearer Key；前端不会拼接任意 URL。`debugger.panels[]` 决定插件页出现的通用流量、日志或结果面板。

配置 Schema 中标记为 `secret: true` 或 `format: password` 的字段不会随插件摘要发送到 WebView。页面只接收“是否已配置”，留空保存会保留宿主已有值，输入非空值才会替换。AI Agent 使用 Provider 时只保存 `pluginId + providerId` 的不透明引用；每次模型请求由 Rust Host 从当前工作区解析 Endpoint、模型与密钥，因此上游密钥和本地代理密钥都不会进入页面状态。

服务可以在 stdout 输出以下单行事件：

```json
DRPA_PLUGIN_EVENT {"kind":"request","req_id":"...","title":"OpenAI request","detail":"..."}
```

Host 会把 JSON 与普通日志分开保存，UI 可按 `req_id` 还原完整请求链路。

## 生命周期

插件页面支持：

- 安装本地 `.drpa-plugin`
- 启用、禁用
- 启动、停止
- 配置和自动启动
- 服务状态、Endpoint 和日志
- 一键设为 AI Agent Provider
- 卸载前二次确认

插件状态保存在自身目录的 `state.json`。自动启动只在 sealed runtime 已经就绪时执行，不触发运行环境重建。

自定义 iframe 面板仅允许应用自身或 `127.0.0.1` / `::1` 的 HTTP 来源，并运行在不含 `allow-same-origin` 的 sandbox 中；远程 URL、脚本 URL 和非 loopback 地址会被页面校验与 CSP 同时拦截。

## 插件开发工作台

插件页可以创建三类本地项目：

- `Tool Provider`：生成 `plugin.yaml`、JSON Schema 和可执行 Python tool。
- `Provider Service`：生成隐藏运行的 HTTP Service、健康检查、模型列表和配置 Schema。
- `组合能力包`：同时生成 Service、Provider、Tool Provider 与 Debugger，适合完整集成。

源码保存在工作区 `plugin-projects/<id>/`。构建前会校验清单、配置 Schema 和全部入口文件；成功后输出 `build/plugins/<id>-<version>.drpa-plugin`，并可直接安装到当前 DRPA。构建器排除 `state.json`、`__pycache__`、`.pyc` 和 Host 标记，确保开发状态不会进入发布包。

## 内置 Dify2API 网关

DRPA 首次初始化会提供默认禁用的 `dify2api` 能力包。它使用无界面
sidecar，把 Dify Agent API 转换为 OpenAI 兼容接口：

- `GET /v1/models`
- `POST /v1/chat/completions`，支持 blocking 与 SSE
- OpenAI `tools` 提示适配和标准 `tool_calls` 输出
- `GET /drpa/debug/upstream`，使用 Dify `/info` 验证真实上游
- OpenAI 请求、Dify 请求、SSE 事件和最终响应的结构化调试链路

它不会复用 Dify `conversation_id`；多轮历史由调用方的 `messages`
维护。当前网关面向 Dify Agent 的 `/chat-messages`，不把
Completion App 或 Workflow App 伪装成 Chat Completions。

服务默认只监听 `127.0.0.1`。DRPA 为每个工作区实例生成独立的
`proxy_api_key`；本地客户端不应直接使用上游 Dify App Key。插件页可把
Endpoint、模型名和本地代理 Key 一次性应用到 AI Agent。

该服务是独立静态可执行文件，启动链不依赖封装 Python。Linux/UOS Host
会把内置 sidecar 复制到当前登录会话的私有执行缓存后再启动，以兼容用户
数据分区带 `noexec` 的企业桌面策略；本地健康检查和调试请求固定直连
loopback，不继承系统 HTTP 代理。上游 Dify 请求仍按 sidecar 自身的代理
环境执行。

DRPA 自带的 Local Dify 开发平台见 [`LOCAL_DIFY.md`](LOCAL_DIFY.md)。

## 当前协议边界

当前版本已实现 HTTP Provider 服务与一次性 JSON-stdio 工具进程。长期服务的双向 `Content-Length` JSON-RPC 传输将沿用以下方法集合：

```text
initialize
tools/list
tools/call
providers/list
provider/chat
health
shutdown
```

服务通知使用 `log`、`progress`、`tool_event` 和 `provider_event`。HTTP 适合 OpenAI/Dify 兼容服务；stdio 适合无需端口的本地工具提供者。
