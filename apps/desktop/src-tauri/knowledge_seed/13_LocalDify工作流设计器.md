# Local Dify 工作流设计器

DRPA 的 Local Dify Studio 内置 Workflow / Chatflow 可视化设计器。它使用本地 Workflow IR 保存节点、连线、配置与画布位置，并可导入、导出 Dify YAML DSL。

## 创建工作流

1. 打开 **AI 应用**。
2. 选择 **新建应用**。
3. 选择 **Workflow** 或 **Chatflow**。
4. 打开 **工作流** 标签。
5. 从左侧节点库点击或拖入节点。
6. 点击节点右侧端口，再点击目标节点完成连接。
7. 在右侧属性面板配置节点。
8. 选择 **校验**、**保存工作流**或**测试运行**。

默认 Workflow：

```text
Start → LLM → End
```

默认 Chatflow：

```text
Start → LLM → Answer
```

## 支持的节点

| 节点 | 作用 |
| --- | --- |
| Start | 输入变量 |
| LLM | 调用当前 OpenAI 兼容 Provider |
| Template Transform | 文本与变量模板 |
| If / Else | TRUE / FALSE 条件分支 |
| HTTP Request | HTTP API 调用 |
| Code | 内置 Python 3 代码 |
| Answer | Chatflow 回复 |
| End | Workflow 结构化输出 |

## 变量引用

```text
{{#start.query#}}
{{#llm.text#}}
{{#template.output#}}
{{#http_node.body#}}
```

End 节点通过 `value_selector` 读取上游输出，例如：

```json
{
  "outputs": [
    { "variable": "answer", "value_selector": ["llm", "text"] }
  ]
}
```

## 快捷键

- `Ctrl+S`：校验并保存；
- `Ctrl+Z`：撤销；
- `Ctrl+Shift+Z`：重做；
- `Delete`：删除选中的节点或连线；
- 鼠标滚轮：缩放画布。

## 调试

测试运行会自动保存当前工作流，然后实时展示：

- 节点开始；
- 节点完成或失败；
- 节点耗时；
- LLM 流式文本；
- 最终 Markdown 输出。

运行记录保存在工作区 `local-dify/runtime.sqlite3`。

## Python 节点

Python 节点使用应用附带的运行环境：

```python
def main(input: str):
    return {"result": input.strip()}
```

定义 `variables` 时，运行器按 `value_selector` 组装 `main(...)` 的关键字参数。返回值应为字典。

## Dify DSL

导入会读取 `workflow.graph.nodes` 和 `workflow.graph.edges`。导出会恢复 Dify 节点结构，并为 LLM 节点写入 Provider 云端映射。导入的扩展节点即使尚未由本地执行器覆盖，也会保留在 IR 与导出文件中。

完整开发说明参见仓库 `docs/LOCAL_DIFY_WORKFLOW.md`。
