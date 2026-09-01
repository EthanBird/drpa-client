# SQL 与数据工作台

DRPA 为所有 RPAZ 提供一个由 Host 管理的本地 SQLite 数据库。脚本通过 `ctx.sql` 写入和查询，用户通过“数据工作台”浏览表结构、执行 SQL、查看结果与查询历史。两条入口使用同一个 `workspace.sqlite3`，因此自动化任务产生的数据可以立即在界面中检查。

## 1. 适用场景

工作区数据库适合：

- 跨运行保存抓取游标、去重键和同步状态；
- 保存结构化采集结果，供下一次任务增量处理；
- 在多个本地 RPAZ 之间共享业务字典；
- 保存适合 SQL 分析的中小型数据集；
- 让 AI Agent 基于明确表结构生成查询，再由用户检查和执行。

一次运行独有的图片、报表、压缩包等文件仍应写入 `ctx.output_file()`。SQLite 是结构化状态层，不替代运行产物目录。

## 2. 文件位置与生命周期

数据库逻辑名称是“工作区数据库”，引擎为 SQLite，文件由 Host 定位：

```text
<DRPA 数据目录>/databases/workspace.sqlite3
```

脚本收到的是已解析的 `database_path`，并由 Runtime 构造 `ctx.sql`。不要从 `ctx.output_dir` 向上猜路径，也不要写死盘符。数据库：

- 跨运行保留；
- 不属于某个 RPAZ 安装包；
- 不进入 Windows 热更新清单；
- 与项目源码、运行输出和 Host 内部运行记录库分离；
- 可以从数据工作台使用“打开目录”定位和备份。

Host 自身的运行审计使用 `system/drpa.sqlite3`。脚本只访问 `databases/workspace.sqlite3`，避免业务表与平台内部迁移相互影响。

## 3. 创建表

入口函数应使用幂等 DDL：

```python
def ensure_schema(ctx):
    ctx.sql.execute(
        """
        CREATE TABLE IF NOT EXISTS image_records (
            id INTEGER PRIMARY KEY,
            source_key TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            source_url TEXT NOT NULL,
            local_file TEXT,
            captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    ctx.sql.execute(
        "CREATE INDEX IF NOT EXISTS idx_image_records_captured_at "
        "ON image_records(captured_at)"
    )
```

建议：

1. 主键优先使用 `INTEGER PRIMARY KEY`。
2. 业务去重字段增加 `UNIQUE`。
3. 时间统一保存 ISO 8601 文本或 Unix 整数，并在项目内保持一致。
4. 常用过滤和关联字段建立索引。
5. 表名加项目或业务前缀，避免不同脚本偶然使用同名表。

## 4. 参数化写入

`execute()` 返回影响行数：

```python
affected = ctx.sql.execute(
    """
    INSERT INTO image_records(source_key, title, source_url, local_file)
    VALUES (?, ?, ?, ?)
    ON CONFLICT(source_key) DO UPDATE SET
        title = excluded.title,
        source_url = excluded.source_url,
        local_file = excluded.local_file,
        captured_at = CURRENT_TIMESTAMP
    """,
    (source_key, title, source_url, str(output_file)),
)
ctx.log.info("数据库写入完成 · affected=%s · key=%s", affected, source_key)
```

命名参数也可使用：

```python
ctx.sql.execute(
    "INSERT INTO settings(name, value) VALUES (:name, :value)",
    {"name": "market", "value": market},
)
```

外部数据只能作为参数值绑定。表名和列名不能通过 `?` 绑定；需要动态标识符时应使用代码内白名单，而不是直接采用用户输入。

## 5. 批量写入

```python
rows = [
    (item["key"], item["title"], item["url"])
    for item in items
]
with ctx.sql.transaction():
    count = ctx.sql.executemany(
        """
        INSERT INTO image_records(source_key, title, source_url)
        VALUES (?, ?, ?)
        ON CONFLICT(source_key) DO NOTHING
        """,
        rows,
    )
ctx.log.info("批量处理完成 · input=%s · inserted=%s", len(rows), count)
```

大批数据应分批处理并报告进度，例如每 500 条一个事务。不要为每行输出一条 INFO 日志；记录批次、总数和异常摘要即可。

## 6. 查询与标量

`query()` 返回 `list[dict[str, Any]]`：

```python
recent = ctx.sql.query(
    """
    SELECT source_key, title, local_file, captured_at
    FROM image_records
    WHERE captured_at >= datetime('now', '-7 day')
    ORDER BY captured_at DESC
    """,
    limit=500,
)
```

`limit` 限制 Python 实际读取的行数，适合避免脚本无意加载过多数据。需要一个值时使用 `scalar()`：

```python
known = ctx.sql.scalar(
    "SELECT 1 FROM image_records WHERE source_key = ? LIMIT 1",
    (source_key,),
    default=0,
)
if known:
    ctx.log.info("记录已存在，跳过下载 · key=%s", source_key)
```

## 7. 事务与回滚

```python
with ctx.sql.transaction():
    ctx.sql.execute(
        "INSERT INTO sync_runs(run_id, state) VALUES (?, 'running')",
        (ctx.run_id,),
    )
    sync_all_items(ctx)
    ctx.sql.execute(
        "UPDATE sync_runs SET state='success' WHERE run_id=?",
        (ctx.run_id,),
    )
```

代码块正常结束时提交；抛出异常时回滚并继续向外传播异常。嵌套 `transaction()` 使用 savepoint：内层失败可以由业务代码捕获，同时保持外层事务结构明确。

事务中不要执行长时间网络请求或浏览器等待。推荐先采集到内存或临时文件，再用短事务写入，减少其他任务等待数据库锁的时间。

## 8. 并发行为

Runtime 默认设置：

- `PRAGMA journal_mode = WAL`；
- `PRAGMA foreign_keys = ON`；
- `PRAGMA busy_timeout = 30000`；
- 独立语句自动提交；
- 显式事务使用 `BEGIN IMMEDIATE`。

WAL 允许读写更好地并行，但 SQLite 同一时刻仍只有一个写入者。多个任务可能同时写数据库时：

- 保持事务短小；
- 使用唯一约束和 upsert 处理竞争；
- 不依赖“先查询、后插入”作为唯一并发保护；
- 为可重试操作记录稳定业务键；
- 遇到失败让异常进入运行记录，不要无限循环重试。

## 9. 数据工作台

打开侧栏“数据工作台”后，界面分为三部分：

1. **连接与对象树**：显示工作区 SQLite、文件大小、表和视图。单击表查看字段，双击生成 `SELECT ... LIMIT 200`。
2. **SQL 编辑器与结果网格**：Monaco 提供 SQL 高亮；选择部分文本后按 `Ctrl+Enter` 只执行选区，没有选区时执行全文。结果最多返回 1000 行、8 MiB，超长单元格按 256 KiB 截断，并明确显示截断状态。
3. **字段与查询历史**：显示列序号、类型、主键、非空和默认值；最近查询保存在本地 UI 偏好中，可以点击恢复到编辑器。

结果网格支持横纵滚动、固定表头、行号、单元格完整值提示和 TSV 复制。DDL/DML 执行后对象树自动刷新。每次执行只接受一个 SQLite statement；需要初始化多条语句时逐条执行，脚本端则把建表逻辑放在入口函数中。

## 10. 与 AI Agent 协作

让 Agent 写 SQL 前，应提供：

- 目标表及字段定义；
- 结果粒度和过滤条件；
- 时间字段的存储约定；
- 最大返回行数；
- 是否允许修改数据。

推荐流程：先让 Agent 生成只读 `SELECT`，在数据工作台检查结果，再决定是否执行写语句。复杂修改先备份 `workspace.sqlite3`，并使用事务包装。

## 11. 完整示例

```python
from __future__ import annotations


def main(ctx):
    ctx.sql.execute(
        """
        CREATE TABLE IF NOT EXISTS task_metrics (
            run_id TEXT PRIMARY KEY,
            package_id TEXT NOT NULL,
            item_count INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )

    items = collect_items(ctx)
    with ctx.sql.transaction():
        ctx.sql.execute(
            """
            INSERT INTO task_metrics(run_id, package_id, item_count)
            VALUES (?, ?, ?)
            """,
            (ctx.run_id, ctx.package_id, len(items)),
        )

    total = ctx.sql.scalar(
        "SELECT COALESCE(SUM(item_count), 0) FROM task_metrics "
        "WHERE package_id = ?",
        (ctx.package_id,),
        default=0,
    )
    ctx.log.info("本次=%s · 历史累计=%s", len(items), total)
    ctx.progress(100, "数据已持久化")
```

上一章：[AI Agent 协作约定](./10_AI_Agent协作约定.md)。
