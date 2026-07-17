# `ctx` 上下文与默认配置详解

Python RPAZ 的入口是 manifest 指定的可调用对象，通常写成：

```python
def main(ctx):
    ...
```

Host 启动 sealed Python worker 后，通过 JSONL 运行协议发送执行请求。Runtime 创建一个 `RuntimeContext`，再动态加载 `entrypoint.module` 并调用 `entrypoint.callable(ctx)`。本章逐项说明当前实现真正提供的字段、方法、默认行为和边界。

## 1. Context 构造来源

执行请求包含：

```text
protocol
run_id
package_id
package_dir
output_dir
entrypoint
callable
parameters
database_path
```

Runtime 要求协议版本为 `1`。请求中的 `parameters` 缺失或为空时转换为空字典。`package_dir` 和 `output_dir` 都会解析为绝对规范路径；输出目录在入口调用前创建。

## 2. `ctx` 字段总表

| 字段 | Python 类型 | 默认 / 来源 | 典型用途 |
|---|---|---|---|
| `ctx.run_id` | `str` | Host 为本次运行生成 | 日志关联、临时标识 |
| `ctx.package_id` | `str` | `manifest.id` | 输出元数据、业务分支 |
| `ctx.params` | `dict[str, Any]` | 工作台提交；缺失时 `{}` | 读取任务参数 |
| `ctx.package_dir` | `pathlib.Path` | 当前项目/安装版本根目录，已 `resolve()` | 读取包内资源 |
| `ctx.output_dir` | `pathlib.Path` | 本次运行输出目录，已创建并 `resolve()` | 输出根目录 |
| `ctx.log` | `logging.Logger` | INFO 级别、独立 Runtime handler | 输出结构化运行日志 |
| `ctx.sql` | `SqlClient` | Host 管理的 `workspace.sqlite3` | 持久化结构化数据、事务和查询 |

内部字段 `ctx._events` 是运行协议写入器，不属于包 SDK。包代码不要直接调用或替换它。

## 3. `ctx.params`

`ctx.params` 是普通 Python 字典，值来自工作台提交的 JSON。它不会自动变成属性对象，也不会自动替你做业务范围校验。

```python
def main(ctx):
    market = str(ctx.params.get("market") or "zh-CN")
    count = int(ctx.params.get("count") or 1)
    headless = bool(ctx.params.get("headless", False))
```

### 三层默认保持一致

manifest：

```yaml
parameters:
  - id: market
    type: string
    required: false
    default: zh-CN
  - id: count
    type: number
    required: false
    default: 1
  - id: headless
    type: boolean
    required: false
    default: false
```

Python：

```python
market = str(ctx.params.get("market") or "zh-CN")
count = int(ctx.params.get("count") or 1)
headless = bool(ctx.params.get("headless", False))
```

业务校验：

```python
if market not in {"zh-CN", "en-US", "ja-JP"}:
    raise ValueError(f"不支持的 market：{market}")
count = max(1, min(count, 8))
```

manifest 默认保证 UI 首次可运行，Python 回退兼容缺字段调用，业务校验保证值可用。三者职责不同。

### 布尔值注意事项

JSON boolean 进入 Python 后是 `bool`。如果外部数据可能传字符串，不要直接写：

```python
# 错误示例：bool("false") 是 True
headless = bool(raw_value)
```

可以使用严格解析：

```python
def parse_bool(value, default=False):
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        normalized = value.strip().lower()
        if normalized in {"true", "1", "yes", "on"}:
            return True
        if normalized in {"false", "0", "no", "off"}:
            return False
    raise ValueError(f"无效布尔值：{value!r}")
```

## 4. `ctx.log`

`ctx.log` 是标准库 `logging.Logger`：

```python
ctx.log.debug("这条默认不会显示")
ctx.log.info("正在处理 %s 条记录", count)
ctx.log.warning("页面没有可选字段，继续使用回退值")
ctx.log.error("任务失败前的业务上下文：step=%s", step)
ctx.log.exception("捕获异常时附带 traceback")
```

当前 logger：

- 名称为 `drpa.runtime.{run_id}`。
- 级别为 `logging.INFO`。
- 不向根 logger 传播。
- 每次构造会清空旧 handler，再添加 Runtime handler。
- 日志事件固定 scope 为 `package`。
- Python level 转成小写，例如 `INFO → info`。
- 参数插值由 `record.getMessage()` 完成。

推荐占位符参数日志，而不是提前拼接超大对象：

```python
ctx.log.info("下载开始 · url=%s · target=%s", url, target.name)
```

## 5. `ctx.progress(value, message="")`

进度 API：

```python
ctx.progress(25, "已完成登录")
ctx.progress(80.5, "正在写入结果")
ctx.progress(100, "任务完成")
```

当前默认行为：

- 接受整数或浮点数。
- 先转换为 `float`。
- 小于 `0` 的值归一为 `0.0`。
- 大于 `100` 的值归一为 `100.0`。
- `message` 为空字符串时，事件中的 message 为 `null`。
- API 不保证单调；脚本应自行保证进度不倒退。

循环进度模板：

```python
total = len(items)
for index, item in enumerate(items, start=1):
    process(item)
    ctx.progress(index / total * 90, f"已处理 {index}/{total}")
finalize()
ctx.progress(100, "全部完成")
```

空列表要先处理，避免除零：

```python
if not items:
    ctx.log.warning("没有需要处理的数据")
    ctx.progress(100, "没有数据")
    return
```

## 6. `ctx.output_file(relative_path, label="")`

这是首选产物 API：

```python
json_path = ctx.output_file("reports/result.json", "JSON 结果")
json_path.write_text(payload, encoding="utf-8")
```

当前行为顺序：

1. 将 `relative_path` 作为相对路径解析到 `ctx.output_dir` 内。
2. 拒绝绝对路径。
3. 解析 `..` 后再次验证最终路径仍在输出根目录内。
4. 创建父目录。
5. 立即发送 artifact 事件。
6. 返回 `pathlib.Path`，由脚本实际写入。

`label` 为空时默认使用文件名；`media_type` 当前由 Context 发送为 `None`。

> 产物事件在文件写入前登记。因此拿到路径后必须完成写入；如果后续失败，日志应明确说明，避免用户看到空文件或缺失文件。

正确写法：

```python
target = ctx.output_file("data/items.json", "采集结果")
target.write_text(json.dumps(items, ensure_ascii=False, indent=2), encoding="utf-8")
ctx.log.info("产物写入完成 · name=%s · bytes=%s", target.name, target.stat().st_size)
```

二进制写法：

```python
target = ctx.output_file("images/cover.jpg", "封面图片")
with target.open("wb") as stream:
    stream.write(image_bytes)
```

### `ctx.open_output_directory()`：打开本次输出目录

需要在任务完成后向用户展示下载文件时，可以请求桌面 Host 在 Windows 资源管理器中打开本次运行的 `output_dir`：

```python
def main(ctx):
    report = ctx.output_file("report.json", "结果")
    report.write_text("{}", encoding="utf-8")

    if bool(ctx.params.get("open_output_directory", False)):
        ctx.open_output_directory()
```

该方法只允许打开当前运行的输出目录，实际桌面操作由 Host 执行。建议把它放在所有文件写入完成之后，并通过 `boolean` 参数让任务配置决定是否自动打开。

## 7. `ctx.sql`

`ctx.sql` 连接 Host 管理的工作区 SQLite 文件。数据库跨任务运行持久存在，默认位于工作区的 `databases/workspace.sqlite3`；脚本不应自行推导或写死这个路径。

```python
def main(ctx):
    ctx.sql.execute(
        """CREATE TABLE IF NOT EXISTS downloads (
               id INTEGER PRIMARY KEY,
               url TEXT NOT NULL,
               saved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
           )"""
    )
    ctx.sql.execute(
        "INSERT INTO downloads(url) VALUES (?)",
        ("https://example.com/file",),
    )
    total = ctx.sql.scalar("SELECT COUNT(*) FROM downloads", default=0)
    rows = ctx.sql.query(
        "SELECT id, url, saved_at FROM downloads ORDER BY id DESC",
        limit=100,
    )
    ctx.log.info("累计记录=%s，本次读取=%s", total, len(rows))
```

可用方法：

- `execute(sql, parameters=()) -> int`：执行一个语句并返回影响行数。
- `executemany(sql, rows) -> int`：批量执行参数化语句。
- `query(sql, parameters=(), limit=None) -> list[dict]`：返回字典行。
- `scalar(sql, parameters=(), default=None) -> Any`：返回第一行第一列。
- `transaction()`：事务上下文；异常时回滚，支持嵌套 savepoint。

始终使用参数绑定，不要把外部输入拼接进 SQL：

```python
title = str(ctx.params.get("title") or "")
ctx.sql.execute("INSERT INTO notes(title) VALUES (?)", (title,))
```

多个写操作需要一起成功时使用事务：

```python
with ctx.sql.transaction():
    ctx.sql.execute("INSERT INTO batches(name) VALUES (?)", (batch_name,))
    batch_id = ctx.sql.scalar("SELECT last_insert_rowid()")
    ctx.sql.executemany(
        "INSERT INTO batch_items(batch_id, value) VALUES (?, ?)",
        [(batch_id, value) for value in values],
    )
```

数据库启用 WAL、外键和 30 秒 busy timeout。结构和数据可在 DRPA 的“数据工作台”中查看；`ctx.sql` 与工作台指向同一文件。

更完整的建模、并发和工作台说明见 [SQL 与数据工作台](./11_SQL与数据工作台.md)。

## 8. `ctx.browser(headless=None)`

创建 DrissionPage `ChromiumPage`：

```python
page = ctx.browser()
# 或
page = ctx.browser(headless=True)
```

当前默认配置和执行过程：

1. 导入 `DrissionPage.ChromiumOptions` 与 `ChromiumPage`。
2. 如果依赖缺失，抛出 `RuntimeError`，说明 browser capability 需要 DrissionPage feature。
3. `headless is None` 时读取 `bool(ctx.params.get("headless", False))`。
4. 创建 `ChromiumOptions()`。
5. 如果环境变量 `DRPA_BROWSER_PATH` 存在，调用 `options.set_browser_path(path)`。
6. 调用 `options.headless(headless)`。
7. 创建 `ctx.output_dir / "downloads"`。
8. 调用 `options.set_download_path(...)`。
9. 返回 `ChromiumPage(options)`。

所以浏览器默认是**有界面模式**。若任务应默认后台运行，需要在 manifest 中显式加入：

```yaml
- id: headless
  type: boolean
  required: false
  default: true
```

并保持 Python 默认一致：

```python
page = ctx.browser(headless=bool(ctx.params.get("headless", True)))
```

下载目录由 Context 自动设置，不要写死用户下载目录。浏览器结束时应主动关闭：

```python
page = ctx.browser()
try:
    page.get("https://example.com")
    ...
finally:
    page.quit()
```

## 9. `ctx.package_dir`

读取包内静态资源：

```python
template = ctx.package_dir / "assets" / "template.json"
data = json.loads(template.read_text(encoding="utf-8"))
```

不要修改安装包目录。开发态目录可能可写，但安装区应视为只读；运行产生的所有内容都进入 output。

需要验证路径未越界时，可以自己使用 `Path.resolve()` 并检查父子关系，或只拼接固定的代码内相对路径。

## 10. `ctx.output_dir`

可以用于创建暂时不希望登记为产物的内部文件，但最终交付文件仍应走 `output_file()`：

```python
scratch = ctx.output_dir / ".scratch"
scratch.mkdir(exist_ok=True)

final = ctx.output_file("final.csv", "最终 CSV")
build_csv(scratch, final)
```

不要假设不同运行共享同一个 output；每次运行应彼此隔离。需要跨运行缓存时，应等待平台提供明确缓存能力，而不是猜测父目录结构。

## 11. 完整 Context 模板

```python
from __future__ import annotations

import json


def main(ctx):
    limit = int(ctx.params.get("limit") or 10)
    limit = max(1, min(limit, 100))
    headless = bool(ctx.params.get("headless", False))

    ctx.log.info(
        "任务开始 · package=%s · run=%s · limit=%s · headless=%s",
        ctx.package_id,
        ctx.run_id,
        limit,
        headless,
    )

    page = ctx.browser(headless=headless)
    try:
        page.get("https://example.com")
        ctx.progress(30, "页面已打开")
        records = collect_records(page, limit)
        ctx.progress(80, f"已采集 {len(records)} 条")
    finally:
        page.quit()

    target = ctx.output_file("records.json", "采集记录")
    target.write_text(
        json.dumps(records, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    ctx.log.info("任务完成 · records=%s · output=%s", len(records), target.name)
    ctx.progress(100, "任务完成")
```

下一章：[参数、输出与产物](./04_参数输出与产物.md)。
