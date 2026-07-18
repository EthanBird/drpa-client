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

清单 schema 为 1：

```yaml
schema: 1
id: example-provider
name: Example Provider
version: 1.0.0
description: 本地模型适配服务
types: [provider-adapter, service, tool-provider]
service:
  runtime: bundled-python
  entry: service/main.py
  transport: http
  endpoint: http://127.0.0.1:{port}/v1
  healthcheck: http://127.0.0.1:{port}/v1/health
tools: []
default_config:
  port: 34122
```

支持的插件类型：

- `provider-adapter`
- `service`
- `tool-provider`
- 后续可扩展 importer、exporter 和 UI extension

服务 runtime 支持：

- `bundled-python`：使用 sealed Python，以 `-I` 模式运行入口脚本。
- `executable`：执行插件自带的当前平台程序。

插件进程使用隐藏窗口启动。声明 `healthcheck` 后，Host 会在返回“运行中”前等待 HTTP 服务真正就绪；超时或提前退出会终止进程并呈现启动错误。Host 保存运行状态、最近 1000 行 stdout/stderr 和导出的 Endpoint；日志按原始字节容错解码，不因单行无效 UTF-8 终止服务管理。

## 插件工具

插件工具的声明和调用约定与 Skill 工具相同，工具名注册为：

```text
plugin_example-provider__tool_name
```

插件 Python 工具入口可以读取 `context.config`。插件禁用后，它导出的工具立即从 Agent ToolRegistry 消失。

## Dify Loves Hermes

DRPA 首次初始化会提供一个默认禁用的 `dify-loves-hermes` 参考插件。它提供：

- `GET /v1/models`
- `POST /v1/chat/completions`
- OpenAI blocking 与 SSE 响应
- Dify Chat、Completion、Workflow 三种 App API 映射
- DRPA Session 与 Dify `conversation_id` 映射
- Dify task/错误信息归一化
- OpenAI `tools/tool_calls` 外部工具桥

工具桥流程：

```text
DRPA Agent → OpenAI tools + messages
           → Dify 结构化决策
           → Bridge 输出 OpenAI tool_calls
           → DRPA ToolRegistry 执行内置/Skill/Plugin 工具
           → role=tool 结果回送 Bridge
           → Dify 继续生成最终回答
```

插件仅在工具可用时要求 Dify 返回严格 JSON 决策；普通聊天继续逐段转发 Dify SSE。工具名称按当前请求白名单校验，参数必须为 JSON 对象，空调用列表和未知工具会作为明确错误返回。

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

## 插件开发工作台

插件页可以创建两类本地项目：

- `Tool Provider`：生成 `plugin.yaml`、JSON Schema 和可执行 Python tool。
- `Provider Service`：生成隐藏运行的 HTTP Service、健康检查、模型列表和配置 Schema。

源码保存在工作区 `plugin-projects/<id>/`。构建前会校验清单、配置 Schema 和全部入口文件；成功后输出 `build/plugins/<id>-<version>.drpa-plugin`，并可直接安装到当前 DRPA。构建器排除 `state.json`、`__pycache__`、`.pyc` 和 Host 标记，确保开发状态不会进入发布包。

## Dify 连接与会话

`dify-loves-hermes` 0.3 提供：

- `GET /v1/provider/test`：使用 Dify `/parameters` 检查 URL、API Key 和 App 参数，并返回耗时与输入字段摘要。
- `conversations.json`：原子持久化最近 1000 个 DRPA Session 到 Dify `conversation_id` 的映射，插件重启后继续同一会话。
- `input_key`：把 Completion/Workflow 的对话文本写入指定 `inputs` 变量，适配不同 Dify App 的输入表单。
- 插件页“测试 Dify 连接”按钮：区分 Bridge 已启动和上游 Dify App API 真正可用。
- 透传 `X-DRPA-Trace-Id`、`X-DRPA-Provider-Route` 与 `X-DRPA-Hop-Count`，支持连接 DRPA Local Dify 并由 Host 检测循环 Provider 链路。

DRPA 自带的 Local Dify 开发平台见 [`LOCAL_DIFY.md`](LOCAL_DIFY.md)。它可以把本地发布应用暴露为 Dify Service API；本插件再将该 API 转换为 OpenAI Chat Completions，因此本地应用既可供普通 Dify 客户端测试，也可直接作为 DRPA AI Agent Provider。

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
