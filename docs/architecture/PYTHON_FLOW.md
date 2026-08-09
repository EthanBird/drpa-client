# Python Flow 架构与低代码约定

Python Flow 是开发工作室中 Python 源码的可视化视图。它服务于 RPAZ 开发、AI 结对编程和低代码编排，但不引入第二套运行时：**可执行的 `.py` 文件始终是唯一事实源**，流程图是由 AST 生成、可校验并可写回源码的投影视图。

## 设计目标

1. 已有 RPAZ 项目无需迁移即可打开流程视图。
2. 图形编辑产生普通 Python，继续走 Studio 保存、直接运行、运行记录和 RPAZ 导出链路。
3. `ctx`、DrissionPage 和 RPA for Python 都是一等调用节点，同时保留任意 Python 的表达能力。
4. 转换过程只解析文本，不导入、执行或读取用户代码引用的路径。
5. 不理解的 Python 语句完整保留为 `raw-code`，不会因为切换视图而丢失代码。
6. 结构、画布布局和运行语义解耦，后续可增加调试断点、节点模板与 AI 生成而不改动 RPAZ 执行协议。

## 模块边界

```text
Studio Monaco buffer (.py, canonical)
        │ inline source
        ▼
Rust Host python_flow adapter
        │ bounded JSON stdin/stdout
        ▼
sealed Python / drpa_runner.python_flow
        │ ast.parse, validate, render
        ▼
PythonFlowGraph v1
        │
        ├─ React visual designer / layout / inspector
        └─ flow_to_python ──> Monaco buffer ──> existing save/run/export
```

- **Python converter**：负责 AST、源码跨度、稳定 ID、结构校验和 Python 生成。这是一个较深的 Module，隐藏 AST 版本差异。
- **Rust adapter**：只接受内联 JSON，限制输入输出尺寸，使用 sealed runtime 执行转换器；它不接触项目文件。
- **React designer**：负责节点呈现、选择、拖拽、自动布局、撤销/重做和属性编辑，不承担 Python 语义解析。
- **Studio document model**：仍持有脏状态、页签、保存和冲突判断，Python Flow 通过 `onChange(source)` 写回同一缓冲区。
- **Run Manager**：完全不认识流程图；它执行写回后的 `main.py`，因此运行记录、日志和产物行为保持一致。

这种边界避免将 UI 图模型、RPAZ 归档格式和 Python 执行器紧耦合。转换器还可被 AI Agent、命令行工具和未来的批量迁移器复用。

## PythonFlowGraph v1

顶层对象：

```json
{
  "schemaVersion": 1,
  "kind": "drpa.python-flow",
  "source": { "name": "main.py", "sha256": "..." },
  "entrypoint": "main",
  "nodes": [],
  "edges": [],
  "metadata": {
    "body": [],
    "moduleBefore": "import rpa as r\n",
    "moduleAfter": "",
    "functionHeader": "def main(ctx):",
    "indent": "    "
  }
}
```

节点公共字段：

- `id`：由节点类型、源码跨度和语句内容派生的稳定 ID；相同源码重复转换得到相同 ID。
- `type`：节点类型。
- `label`：面向画布的短标题。
- `code`：该节点对应的完整语句文本，是无损降级和属性编辑的基础。
- `span`：`startLine/startColumn/endLine/endColumn`，用于跳转和后续调试映射。
- `data`：节点类型专属的结构化字段。
- `position`：可选画布坐标；它只影响视图，不影响执行顺序。

首批节点类型：

| 类型 | 语义 | 典型例子 |
| --- | --- | --- |
| `start` / `end` | 函数边界 | `main(ctx)` |
| `assign` | 赋值 | `name = ctx.params["name"]` |
| `call` | 普通 Python 调用 | `notify(result)` |
| `ctx-call` | DRPA Runtime Context 调用 | `ctx.progress(50)`、`ctx.sql.query(...)` |
| `rpa-call` | RPA for Python 调用 | `r.click("登录")` |
| `if` | 条件与分支体 | `if enabled:` |
| `for` / `while` | 循环及退出 | `for item in rows:` |
| `try` | `try/except/else/finally` | 事务或恢复逻辑 |
| `return` | 返回并连接结束节点 | `return result` |
| `raw-code` | 无损保留尚未结构化的语句 | `with`、`match`、类定义等 |

边使用 `next/true/false/body/exit/except/finally/return` 等显式语义。画布坐标不决定运行顺序；结构化子节点和边必须通过校验器保持一致。

## 往返与冲突规则

### 打开流程视图

Studio 将当前 Monaco 内存缓冲区连同文件名传给 `python-to-flow`。转换器执行 `ast.parse`，优先投影 `main(ctx)` 的函数体；import、常量和辅助函数保存在 preamble/原始节点中。语法错误返回行列，Studio 保持代码视图并定位问题。

### 写回代码

属性编辑或节点操作先更新图，在应用到代码前运行 `validate-flow`，再调用 `flow-to-python`。生成器保持四空格缩进和可解析 Python，并把结果写回当前页签；此时页签进入普通的未保存状态。

### 防止覆盖并发编辑

生成图时记录 `sourceHash`。若 Monaco 内容已变而流程视图仍基于旧哈希，界面标记“源码已变化”，先刷新流程图，不直接覆盖缓冲区。撤销/重做属于画布会话；写回后仍可使用 Monaco 的源码撤销栈。

### 无损原则

首期不对表达式建立细粒度节点。复杂表达式保留在 `data` 和 `code`；`with`、`match`、装饰器、异步语句、类和未知 AST 使用 `raw-code`。设计器允许编辑原始代码，并在提交前重新解析。增加新节点类型时，旧版本仍可把它显示为原始代码节点。

## RPAZ 规范补强

现有 schema 2 能描述入口、Python 版本、基础网络/文件能力和运行参数，但对自动化开发仍有四处较浅的接口：

1. **依赖只写 Python 版本**：没有声明平台预置能力。Python Flow 首期通过 sealed runtime 提供 `drpa_runner`、DrissionPage、`rpa`；后续 schema 应增加可选的 `runtime.features`，例如 `browser.drissionpage`、`automation.rpa-python`、`data.sqlite`，由 Host 在预检时解析，而不是让项目执行 pip。
2. **浏览器所有权不明确**：DrissionPage 使用工作区持久 Chrome，RPA for Python 的 TagUI 浏览器则是另一实现。未来的 capability 应声明 `browser.provider` 和 `browser.sessionPolicy`，防止两个后端争用生命周期。
3. **缺少源码到运行事件的映射**：当前运行记录按日志/进度呈现。Python Flow 的 `span` 与稳定节点 ID 为未来 `node_started/node_finished/node_failed` 事件预留映射，但首期不修改稳定运行协议。
4. **可视化元数据不应污染运行清单**：布局、折叠、颜色和注释属于开发态 sidecar。后续若持久化，使用 `.drpa/python-flow/<relative-file>.json`，RPAZ 构建器可选择携带；运行器忽略该目录。

在 schema 正式扩展前，新项目 README 会明确平台预置依赖与离线边界。项目执行仍只依赖合法的 schema 2 清单，从而保持旧客户端兼容。

## RPA for Python 的边界

平台内置 PyPI 包 `rpa`（导入方式 `import rpa as r`）及其 `tagui` Python 适配层，使补全、Notebook 和 RPAZ 源码可以稳定导入。TagUI 的平台引擎与视觉依赖体积较大，必须由离线运行时构建阶段预置和锁定；运行时不执行联网 bootstrap。

Python Flow 将 `r.*` 调用识别为 `rpa-call`，常用动作可进入节点模板：

```python
import rpa as r

def main(ctx):
    r.init()
    r.url("https://example.test")
    r.type("//input[@name='q']", ctx.params["query"])
    r.click("//button[@type='submit']")
    ctx.progress(100, "完成")
```

RPA for Python 和 `ctx.browser()` 是并列 Adapter，而非互相包装。新任务优先按目标选择一个浏览器后端；数据、日志、参数和产物继续通过 `ctx` 进入 DRPA Host。

## 校验与安全属性

- 转换器只接受 `source` 文本和安全的显示文件名，不接受任意磁盘路径。
- AST 转换不 import、不 eval、不 exec。
- dangling edge、重复 ID、未知节点类型、缺少 start/end、结构字段错误在写回前终止。
- Rust Host 对源码和 JSON 响应设置上限，并隔离用户 site packages。
- 生成代码必须再次通过 `ast.parse`；RPAZ 自身仍经过 manifest、归档路径和尺寸校验。

## 分阶段演进

1. **v1 投影视图**：AST 往返、核心节点、画布、属性、自动布局、代码/流程切换。
2. **节点模板库**：`ctx`、DrissionPage、RPA for Python、SQLite、文件、控制流模板；模板最终生成普通 Python。
3. **开发态 sidecar**：保存布局与注释，使用源码哈希检测漂移。
4. **节点级调试**：将稳定 ID 注入运行事件，运行记录可从异常跳回源码/节点。
5. **可组合子流程**：以普通 Python 函数作为子流程 Interface，支持参数和返回值，不发明私有执行语言。
6. **AI 协作**：Agent 读取图和源码，提交结构化 patch；Host 校验后再写回，保留可审阅 diff。

## 验收基线

- 相同源码生成稳定节点 ID 和源码跨度。
- `ctx`、`rpa`、普通调用分类正确。
- 条件、循环、异常和返回形成可校验边。
- 未支持语句往返后仍可由 `ast.parse` 解析，关键源码保持。
- 转换恶意文本不会执行任何语句或访问传入路径。
- 流程编辑写回同一 Studio 页签，可保存、直接运行并在运行记录查看结果。
- Windows 与 UOS 使用同一 Python Flow 协议，复杂 UI 操作不引入同步 IPC 循环。
