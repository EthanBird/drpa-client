# 数据工作台与 `ctx.sql`

DRPA Next 内置自有数据工作台，交互布局参考通用数据库客户端与 DBX 的对象树、SQL 编辑器、结果网格分区，但不嵌入或复制 DBX 运行时代码。工作区 SQLite 由 Host 管理，并与平台内部运行记录库隔离；同一界面也可保存并连接 PostgreSQL、MySQL、外部 SQLite，以及只读的 XLS/XLSX/XLSB/ODS 工作簿。

DBX 参考基线为 `t8y2/dbx@5206750a5303f7c91cbd76d52d4ba38d9a675cb1`。DRPA 只借鉴其“连接注册表 → schema 浏览 → 查询会话 → 受预算结果集”和编辑器内 AI 的产品分层；实现继续使用本项目自己的 Tauri commands、DTO、React 页面和运行数据目录，不引入 DBX crate、前端包或运行时。

## 存储布局

```text
<workspace>/
├── system/drpa.sqlite3             # Host 内部：运行、事件、产物
└── databases/
    ├── workspace.sqlite3           # 用户/RPAZ 数据：ctx.sql 与数据工作台
    └── connections.json            # 远程连接元数据，不包含密码
```

`data/` 不进入 Windows 更新清单。工作区数据库会跨运行保留，可从数据工作台打开所在目录后备份。前端不直接打开 SQLite；所有操作都通过 typed gateway 调用 Rust command。

## Host API

Tauri commands：

- `get_workspace_database_info`
- `list_database_tables`
- `describe_database_table`
- `execute_database_sql`
- `get_database_schema_context`
- `open_workspace_database_directory`
- `list/save/delete_remote_database_profile`
- `test_remote_database_connection`
- `list/describe/execute_remote_database_*`
- `get_remote_database_schema_context`

Rust 使用 bundled `rusqlite`，启用 WAL、foreign keys 和 30 秒 busy timeout。查询结果最多返回 1000 行、8 MiB，每个 TEXT/BLOB 单元最多读取 256 KiB；超出任一预算时返回 `truncated=true`。BLOB 在 UI 协议中编码为 `0x` 十六进制文本。

WebView 只提交 SQL 文本和表名；数据库路径固定由 `AppPaths.workspace_root` 生成。结构查询使用参数化 `pragma_table_info(?1)`，不拼接表名。

远程连接由 Rust `sqlx::Any` 适配 PostgreSQL、MySQL，并使用系统原生根证书完成可选 TLS。`connections.json` 只保存名称、类型、主机、端口、数据库、用户名和 TLS 模式。密码只在当前数据工作台组件内存中保留，退出应用后消失，不进入 Zustand 持久化、连接文件、查询历史或日志。远程查询沿用本地结果预算：最多 1000 行、8 MiB、单元格 256 KiB。

文件数据源同样保存在 `connections.json`，但只记录文件路径，不记录文件内容。外部 SQLite 使用现有数据库文件并支持读写；Excel 工作簿在每次连接时映射到内存 SQLite，每个非空 sheet 对应一张表、首行作为字段名，只允许只读查询。单个工作簿限制为 100 MiB、100 个 sheet、每个 sheet 100,000 行和 512 列，避免异常文件拖垮桌面 Host。

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

- 左侧显示本地及远程连接、文件数据源、端点、表和视图；可新建、编辑、测试、删除 PostgreSQL/MySQL、外部 SQLite 和 Excel 连接。
- 单击表加载列定义；双击生成受引用的 `SELECT * ... LIMIT 200`。
- Monaco SQL 编辑器使用 `Ctrl+Enter` 执行选区或全文。
- 结果区显示列、行号、耗时、影响行数、截断状态，支持复制 TSV。
- 右侧显示字段类型、主键、NOT NULL、默认值和最近 30 条本地查询历史。
- DDL/DML 成功后刷新对象树。
- “AI 写 SQL”读取当前连接的结构，复用设置中的 OpenAI-compatible 模型参数并流式渲染 Markdown。模型只生成代码，不自动执行；用户选择追加或替换编辑器后再手动运行。

## 扩展约定

后续增加 SQL Server、Oracle 等驱动时继续经过 Host connection registry，并保持当前 `connectionId`、列名/二维值、截断标记协议，避免 UI 与驱动耦合。密码目前是会话级内存值；需要跨启动保存时应接入操作系统凭据保险箱，只在连接配置中保存 secret reference。
