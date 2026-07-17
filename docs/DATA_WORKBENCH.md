# 数据工作台与 `ctx.sql`

DRPA Next 内置自有数据工作台，交互布局参考通用数据库客户端与 DBX 的对象树、SQL 编辑器、结果网格分区，但不嵌入或复制 DBX 运行时代码。当前第一阶段使用 SQLite，数据库文件由 Host 管理，并与平台内部运行记录库隔离。

## 存储布局

```text
<workspace>/
├── system/drpa.sqlite3             # Host 内部：运行、事件、产物
└── databases/workspace.sqlite3     # 用户/RPAZ 数据：ctx.sql 与数据工作台
```

`data/` 不进入 Windows 更新清单。工作区数据库会跨运行保留，可从数据工作台打开所在目录后备份。前端不直接打开 SQLite；所有操作都通过 typed gateway 调用 Rust command。

## Host API

Tauri commands：

- `get_workspace_database_info`
- `list_database_tables`
- `describe_database_table`
- `execute_database_sql`
- `open_workspace_database_directory`

Rust 使用 bundled `rusqlite`，启用 WAL、foreign keys 和 30 秒 busy timeout。查询结果最多返回 1000 行、8 MiB，每个 TEXT/BLOB 单元最多读取 256 KiB；超出任一预算时返回 `truncated=true`。BLOB 在 UI 协议中编码为 `0x` 十六进制文本。

WebView 只提交 SQL 文本和表名；数据库路径固定由 `AppPaths.workspace_root` 生成。结构查询使用参数化 `pragma_table_info(?1)`，不拼接表名。

## Python API

每次 RPAZ 运行获得一个 `ctx.sql: SqlClient`：

```python
ctx.sql.execute(sql, parameters=())
ctx.sql.executemany(sql, rows)
ctx.sql.query(sql, parameters=(), limit=None)
ctx.sql.scalar(sql, parameters=(), default=None)

with ctx.sql.transaction():
    ...
```

独立语句自动提交；事务使用 `BEGIN IMMEDIATE`，嵌套事务使用 savepoint。worker 结束时关闭连接。参数接受 SQLite positional sequence 或 named mapping。

```python
def main(ctx):
    ctx.sql.execute(
        "CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT NOT NULL)"
    )
    ctx.sql.execute("INSERT INTO items(name) VALUES (?)", ("sample",))
    rows = ctx.sql.query("SELECT id, name FROM items ORDER BY id", limit=100)
    ctx.log.info("items=%s", len(rows))
```

## 前端行为

- 左侧显示本地连接、数据库路径、表和视图。
- 单击表加载列定义；双击生成受引用的 `SELECT * ... LIMIT 200`。
- Monaco SQL 编辑器使用 `Ctrl+Enter` 执行选区或全文。
- 结果区显示列、行号、耗时、影响行数、截断状态，支持复制 TSV。
- 右侧显示字段类型、主键、NOT NULL、默认值和最近 30 条本地查询历史。
- DDL/DML 成功后刷新对象树。

## 扩展约定

后续远程数据库连接必须继续经过 Host connection registry，不把凭据放入 React store 或查询历史。建议新增统一 `connectionId` DTO，再分别实现 SQLite、PostgreSQL、MySQL adapter；结果集保持当前列/二维值协议，避免 UI 与数据库驱动耦合。远程连接凭据应引用凭据保险箱条目，查询编辑和 schema 浏览继续复用现有页面。
