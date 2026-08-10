# RPaz 脚本包开发

RPaz 是一个根目录包含 `manifest.yaml` 的 ZIP 文件。DRPA Next 的开发工作室可以新建项目、编辑源码和 Notebook、直接运行当前源码、校验清单并导出 `.rpaz`。

## 在开发工作室创建

1. 打开“开发工作室”。
2. 只输入项目名称。内部项目 ID 与清单 package ID 会自动由名称和随机盐计算，不需要手工遵守包名格式。
3. 点击“新建项目”。
4. 先阅读自动生成的 `README.md`，再编辑 `main.py`、`manifest.yaml` 或 Jupyter 兼容的 `notebook.ipynb`。README 包含面向 AI Agent 的入口顺序、RPAZ 约束、`ctx` 能力、离线依赖和测试约定。
5. 展开“运行参数”，填写 JSON 后点击“直接运行”；这会运行当前工作副本，不需要先构建或安装。
6. 需要分发时点击“导出 RPAZ”。

“打开已安装包”会把已安装版本复制为新的可编辑工作副本，绝不会直接修改安装区。修改后可以直接运行或另行导出。

编辑器和 Notebook Kernel 完全来自应用安装包，不会访问 CDN。Python 文件和 Notebook 代码单元使用 Jupyter `complete_request` + IPython/Jedi 提供离线补全，补全读取当前代码和项目级持久命名空间。Notebook 支持逐单元格/全部运行、执行计数、标准输出、异常、变量浏览和重启 Kernel。项目源码、构建产物和运行输出都保存在本地工作区。

## 最小目录

```text
my-task/
├── README.md
├── manifest.yaml
├── main.py
└── notebook.ipynb
```

`manifest.yaml`：

```yaml
schema: 2
id: com.example.my-task
name: 我的任务
version: 0.1.0
entrypoint:
  runtime: python
  module: main.py
  callable: main
runtime:
  python: "3.11.*"
capabilities:
  network:
    allow: []
  filesystem:
    read: []
    write: ["$outputs"]
parameters:
  - id: message
    type: string
    required: true
```

`main.py`：

```python
def main(ctx):
    message = str(ctx.params["message"])
    ctx.log.info("开始处理：%s", message)
    result = ctx.output_file("result.txt", "运行结果")
    result.write_text(message, encoding="utf-8")
    ctx.progress(100, "完成")
```

## Runtime Context

- `ctx.params`：工作台提交的参数字典。
- `ctx.log`：标准 Python logger，日志会显示在工作区。
- `ctx.progress(value, message)`：报告 0 到 100 的进度。
- `ctx.output_file(relative_path, label)`：获得隔离输出路径并登记产物。
- `ctx.open_output_directory()`：请求 Host 打开本次运行输出目录。
- `ctx.sql`：连接工作区 SQLite；提供 `execute`、`executemany`、`query`、`scalar` 和事务。
- `ctx.browser(headless=True)`：连接封装的 Chrome for Testing 与 DrissionPage 持久会话。Windows 任务结束和 `page.quit()` 不终止浏览器，后续任务继续复用同一 Profile 与调试端口。

`ctx.sql` 示例：

```python
def main(ctx):
    ctx.sql.execute("CREATE TABLE IF NOT EXISTS counters (name TEXT PRIMARY KEY, value INTEGER NOT NULL)")
    ctx.sql.execute(
        "INSERT INTO counters(name, value) VALUES (?, 1) "
        "ON CONFLICT(name) DO UPDATE SET value = value + 1",
        ("runs",),
    )
    ctx.log.info("累计运行：%s", ctx.sql.scalar("SELECT value FROM counters WHERE name=?", ("runs",)))
```

完整说明见 [数据工作台与 `ctx.sql`](DATA_WORKBENCH.md)。

脚本不应写死工作目录、解释器路径或浏览器路径，也不应自行调用 pip。schema 2 项目只使用平台 sealed runtime 中经过锁定和验证的 wheelhouse；增加平台级依赖需要更新离线运行时构建锁并重新发布全量运行时。

## RPA for Python

sealed runtime 提供 [RPA for Python](https://github.com/tebelorg/RPA-Python)，项目使用官方导入方式：

```python
import rpa as r


def main(ctx):
    target = str(ctx.params.get("target", "https://example.test"))
    r.init()
    try:
        r.url(target)
        ctx.log.info("已打开：%s", target)
        ctx.progress(100, "RPA for Python 流程完成")
    finally:
        r.close()
```

`rpa`/TagUI 与 `ctx.browser()`/DrissionPage 是两套并列的自动化 Adapter。前者适合采用 `click/type/read/snap` 等简洁动作的既有脚本，后者使用 DRPA 管理的持久 Chrome 会话。一个任务优先选择同一浏览器 Adapter，数据、参数、日志、进度和产物仍通过 `ctx` 进入 Host。

最终用户环境按离线方式运行。项目代码不执行 `pip`、`r.pack()` 或在线 bootstrap；依赖和平台资产由 DRPA 全量 sealed runtime 的构建锁统一管理。

## Python Flow（Beta）

在开发工作室打开任意 `.py` 文件后，编辑区右上角固定显示“代码 / 流程图”切换器；点击带有 `Python Flow · Beta` 标记的“流程图”即可生成当前代码的可视化视图。该入口独立于可横向滚动的文件页签，多文件同时打开时仍保持可见。Flow 使用 sealed Python 的 `ast` 静态解析当前 Monaco 内存缓冲区，识别：

- 赋值、普通函数调用和返回；
- `ctx.*`、`ctx.sql.*` 与 `ctx.browser(...)`；
- `import rpa as r` 后的 `r.*` 调用；
- `if`、`for`、`while`、`try/except/finally`；
- 其他 Python 语句的原始代码节点。

流程图支持节点拖拽、连线、自动布局、属性 JSON、撤销/重做、结构校验和源码预览。点击应用后先校验 Flow，再生成普通 Python 写回当前页签；使用 `Ctrl+S` 保存，随后继续“直接运行”或“导出 RPAZ”。源码是执行行为的唯一事实源，流程图不会建立另一套解释器。

若在生成流程图后又修改了源码，界面会标记漂移；先从代码刷新流程，避免用旧图覆盖新代码。语法错误会返回文件名和行列。`with`、`match`、装饰器和其他首期未结构化语句作为原始代码整体保留。详细边界见 [Python Flow 架构](architecture/PYTHON_FLOW.md)。

## 手工构建

压缩时 `manifest.yaml` 必须位于归档根目录，不能多套一层项目文件夹：

```bash
cd my-task
python -m zipfile -c ../my-task.rpaz manifest.yaml main.py
```

安装器会拒绝父目录跳转、绝对路径、符号链接、重复路径、压缩炸弹和不受支持的 schema。

## Bing 示例

仓库中的 `examples/bing_daily_image.rpaz` 是一个只依赖 Python 标准库的完整测试包。安装后使用 `market=zh-CN`、`image_count=1` 运行；成功时会生成一张图片与 `bing-daily-images.json`，日志中会显示实际产物路径。
