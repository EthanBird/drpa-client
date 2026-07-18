# Local Dify 工作流设计器

DRPA 在 Local Dify Studio 中内置了离线优先的 Workflow / Chatflow 设计器。它不是对 Dify Web 页面进行嵌套，而是维护一套稳定、可迁移的本地 Workflow IR，再在导入导出边界转换为 Dify DSL。

## 1. 设计目标

- 本地完成画布编排、配置、校验、调试和运行记录查看。
- 用户离线时仍可编辑模板、条件、代码与流程结构。
- Provider 继续复用 Local Dify 的 OpenAI 兼容连接。
- 导入 Dify Workflow / Chatflow YAML 后保留节点配置和画布坐标。
- 导出时生成 Dify `workflow.graph.nodes` / `workflow.graph.edges` 结构。
- `/v1/workflows/run` 与桌面调试使用同一个执行器，避免两套语义。

参考实现与字段以 [Dify 官方仓库](https://github.com/langgenius/dify)中的 Workflow 转换和 DSL 服务为准：

- [`api/services/workflow/workflow_converter.py`](https://github.com/langgenius/dify/blob/main/api/services/workflow/workflow_converter.py)
- [`api/services/app_dsl_service.py`](https://github.com/langgenius/dify/blob/main/api/services/app_dsl_service.py)

## 2. 界面组成

进入“AI 应用”，创建 `Workflow` 或 `Chatflow` 应用后会出现“工作流”标签。

### 2.1 节点库

节点库位于左侧，支持点击添加或拖放到画布：

| 节点 | 本地执行 | Dify DSL | 用途 |
| --- | --- | --- | --- |
| Start | 是 | 是 | 声明输入变量并写入运行上下文 |
| LLM | 是 | 是 | 调用当前应用绑定的 OpenAI 兼容 Provider |
| Template Transform | 是 | 是 | 使用变量生成文本 |
| If / Else | 是 | 是 | 根据字符串、空值或数值条件选择分支 |
| HTTP Request | 是 | 是 | 发送 GET/POST/PUT/PATCH/DELETE 请求 |
| Code | Python 3 | 是 | 使用封装 Python 运行时执行 `main(...)` |
| Answer | 是 | 是 | Chatflow 的最终回复节点 |
| End | 是 | 是 | Workflow 的结构化输出节点 |

导入的其他 Dify 节点会保留在 IR 和导出 DSL 中，校验器会标记本地执行支持状态。

### 2.2 画布

画布支持：

- 节点拖动；
- 点击节点输出端口，再点击目标节点完成连线；
- If / Else 独立 TRUE、FALSE 输出端口；
- 连线选择、标签编辑与删除；
- 鼠标滚轮缩放、缩放按钮和适应画布；
- `Delete` 删除选中节点或连线；
- `Ctrl+Z` / `Ctrl+Shift+Z` 撤销与重做；
- `Ctrl+S` 校验并保存。

Start 节点是工作流入口，画布中只允许一个。Workflow 至少需要一个 End；Chatflow 至少需要一个 Answer。

### 2.3 属性面板

右侧属性面板提供常用字段的结构化编辑，同时保留“高级 JSON 配置”。高级配置直接对应节点 `data` 中除 `title` 和 `type` 以外的字段，适合粘贴 Dify 的高级节点参数。

变量引用使用 Dify 形式：

```text
{{#start.query#}}
{{#llm.text#}}
{{#http_node.body#}}
```

模板转换节点也接受简写：

```text
{{ input }}
```

## 3. Workflow IR

应用数据位于：

```text
<workspace>/local-dify/apps/<app-id>/app.json
```

`LocalDifyApp.workflow` 的核心结构：

```json
{
  "schema": 1,
  "viewport": { "x": 80, "y": 120, "zoom": 1 },
  "nodes": [
    {
      "id": "start",
      "kind": "start",
      "title": "开始",
      "x": 80,
      "y": 210,
      "width": 220,
      "height": 84,
      "config": {
        "variables": [
          { "label": "query", "variable": "query", "type": "paragraph", "required": true }
        ]
      }
    }
  ],
  "edges": [
    {
      "id": "edge-start-llm",
      "source": "start",
      "target": "llm",
      "sourceHandle": "source",
      "targetHandle": "target",
      "label": "",
      "data": {}
    }
  ]
}
```

IR 与 Dify DSL 分离的原因：

1. DRPA 可以独立升级本地运行状态、调试数据和画布交互字段。
2. Dify DSL 版本变化集中在转换边界处理。
3. 不依赖 Dify 前端包，减少桌面安装体积与升级耦合。

旧版 Local Dify 应用没有 `workflow` 字段时会自动迁移。Workflow / Chatflow 应用会生成 Start → LLM → End/Answer 默认图。

## 4. 校验规则

保存、运行、发布和导出前执行后端校验：

- 节点 ID 与连线 ID 唯一；
- 恰好一个 Start；
- 存在符合应用模式的输出节点；
- 连线源节点与目标节点存在；
- 禁止自连接；
- 输出节点可由 Start 到达；
- 基础图不存在环；
- 最多 256 个节点、1024 条连线；
- 未连接或不可达节点产生 warning；
- 本地执行器未实现的导入节点产生 warning，并继续保留到 DSL。

循环需求后续由显式 Iteration / Loop 容器节点承载，而不是普通边构成的隐式环。

## 5. 本地执行模型

执行器从 Start 开始按有向无环图推进。节点输出保存在：

```text
outputs[node_id][output_name]
```

每个节点发送桌面流式事件：

```text
started
nodeStarted
delta               # LLM 文本增量
nodeCompleted
nodeFailed
completed
```

调试抽屉会实时显示节点状态、耗时和最终 Markdown 输出。完整运行结果继续写入：

```text
<workspace>/local-dify/runtime.sqlite3
```

### 5.1 LLM

- 使用应用选择的 Provider、Temperature 和最大输出 Token。
- 读取节点 `prompt_template`。
- Provider 开启 Streaming 时透传增量文本。
- 多个 LLM 节点的 Token 用量累加到一次工作流运行记录。

### 5.2 条件分支

本地执行支持：

- 等于、不等于；
- 包含、不包含；
- 开头、结尾；
- 为空、不为空；
- 大于、小于、大于等于、小于等于。

命中条件后选择 `sourceHandle=true`；否则选择 `sourceHandle=false`。

### 5.3 HTTP

URL、Header 和 Body 在发送前进行变量替换。输出包含：

```json
{
  "status_code": 200,
  "body": "response text",
  "json": {}
}
```

### 5.4 Python Code

代码节点使用 DRPA 安装包的 Python 运行时，要求定义：

```python
def main(input: str):
    return {"result": input}
```

输入来自 `variables[].value_selector`。没有声明变量时提供 `input=<query>`。返回值应为字典，运行超时为 30 秒。

## 6. Dify DSL 互操作

导入时读取：

```text
app.mode
workflow.graph.viewport
workflow.graph.nodes
workflow.graph.edges
workflow.features.opening_statement
```

节点的 `data.type` 转为 `kind`，`data.title` 转为 `title`，其余字段进入 `config`。导出时执行反向转换，并把 Local Dify Provider 的云端映射写入所有 LLM 节点。

发布前的兼容性检查会同时报告：

- 图结构错误；
- 未连接节点；
- 本地 Provider URL；
- Dify 云端 Provider / Model 映射缺失；
- 本地执行器尚未覆盖的导入节点。

## 7. Local Dify API

发布 Workflow 后可调用：

```http
POST /v1/workflows/run
Authorization: Bearer <APP_TOKEN>
Content-Type: application/json

{
  "inputs": { "query": "生成日报" },
  "response_mode": "blocking",
  "user": "local-user"
}
```

API 服务与桌面画布共用 Workflow IR、Provider 路由、循环检测和运行记录。

## 8. 后续扩展点

- Iteration / Loop 容器与子图调试；
- Tool、Knowledge Retrieval、Agent 和 Parameter Extractor 节点；
- 多分支并行调度与汇聚状态；
- 单节点运行、从节点运行和断点恢复；
- 节点模板、复制粘贴和 AI 生成工作流；
- Workflow 运行记录中的节点级输入输出持久化。
