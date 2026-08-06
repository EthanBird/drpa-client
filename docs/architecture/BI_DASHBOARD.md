# BI 主页架构

> 交互更新：编辑模式采用“拖动预览、松手提交”的指针交互。放置时只置换实际碰撞的组件，未碰撞组件保持原坐标；额外碰撞项按从左到右、从上到下寻找空位。响应式投影按组件左右边界统一换算，并结合组件类型的最小可读宽度重新排布。位置提交使用 FLIP 动画，并遵循系统的减少动态效果设置。

## 1. 目标

DRPA 的总览页不是固定报表，而是工作区级低代码 BI 画布。它同时消费两类数据：

1. DRPA 内置数据集，例如工作区指标、运行记录、运行状态和 RPAZ 包；
2. 数据工作台管理的工作区 SQLite、外部 SQLite、Excel、PostgreSQL 和 MySQL 数据源。

仪表盘定义、数据访问与图形渲染相互独立。新增数据库驱动、图表类型或数据转换时，不需要改写整个主页。

## 2. 分层

```mermaid
flowchart LR
  D["DashboardDocument\n布局与可视化配置"] --> P["BI Page\n栅格编辑器"]
  P --> R["DashboardDataRuntime\n适配器注册表"]
  R --> B["Builtin Adapter\nDRPA Snapshot"]
  R --> Q["Database Adapter\n只读查询 API"]
  Q --> W["工作区 SQLite"]
  Q --> C["数据工作台连接"]
  P --> V["Widget Renderer\nMetric / Chart / Table / Markdown"]
  S["dashboard/home.json"] <--> D
```

### 2.1 定义层

`DashboardDocument` 只描述：

- 仪表盘列表与当前主页；
- 12 列等基础栅格参数；
- 组件类型和栅格位置；
- 数据源引用与只读 SQL；
- 字段映射、数字格式、颜色和刷新周期。

定义中不存查询结果，也不存数据库密码。每个工作区独立保存到 `dashboard/home.json`。Rust Host 对数量、字段长度、栅格边界、数据源和 SQL 大小进行验证，并使用临时文件与备份完成替换。

### 2.2 数据层

`DashboardDataRuntime` 是前端数据适配器注册表。适配器统一输出：

```ts
interface DashboardDataset {
  columns: string[];
  rows: Array<Record<string, unknown>>;
  durationMs: number;
  truncated: boolean;
}
```

当前适配器：

- `builtin`：把 `WorkspaceSnapshot` 转换为表格式记录；
- `database`：调用 `execute_dashboard_database_query`，复用数据工作台连接。

后续可注册 HTTP、Parquet、插件或 RPAZ 产物适配器。组件渲染器只依赖 `DashboardDataset`，不感知连接方式。

数据库 BI 查询强制使用 `database::agent_execute_read_only_query`。SQLite 同时检查 prepared statement 的 `readonly()`；PostgreSQL/MySQL 在只读事务中执行并回滚。远程密码只存在当前前端会话，不写入仪表盘定义。

### 2.3 展示层

组件渲染器按照 `kind` 分派：

- `metric`：单值指标；
- `line`、`bar`、`pie`：轻量 SVG/CSS 图表；
- `table`：滚动结果表；
- `markdown`：文字、口径和结论。

展示层不直接调用 Gateway。加载、错误、截断和刷新状态由 BI Page 统一管理。

## 3. 响应式栅格

持久化布局使用稳定的基础列数，默认 12 列。运行时根据画布宽度投影为：

- 宽屏：基础列数；
- 中等宽度：最多 8 列；
- 窄屏：最多 4 列。

投影只影响显示，不改写用户保存的基础布局。横向位置和宽度不再分别取整，而是投影 `left` / `right` 两条共享边界；相邻组件在不同断点下仍共享同一条边界，不会出现位置已经缩放、宽度仍停留在另一档的情况。

窄画布还会应用组件级最小可读宽度：指标、饼图和 Markdown 至少占 2 列，折线图、柱状图和表格占满 4 列画布。组件自身使用 CSS Container Queries 响应实际卡片宽度，而不是只依据应用窗口宽度；标题元数据、指标字号、饼图图例和图表说明会在卡片变窄时逐级简化。

编辑交互遵循以下不变量：

1. 指针移动期间只更新拖动卡片的浮动预览和目标占位，不持续改写文档布局；
2. 松手时执行一次布局提交，降低抖动和高频保存；
3. 没有与目标区域相交的组件保持原始 `x/y/w/h`；
4. 第一个碰撞组件优先进入拖动组件腾出的原位置，形成直观置换；
5. 原位置容纳不下时，才按行优先顺序寻找第一个空位；
6. 删除组件属于显式整理操作，可按行优先顺序重新填充空位；
7. 缩放同样先预览、后提交，碰撞处理与拖放共用一套规则。

响应式视图可能为了最小可读宽度把组件临时放到不同的显示坐标，因此拖动坐标使用“显示位置增量 → 基础栅格增量”换算，而不是把显示坐标直接写回基础布局。只按下再松开不会改写保存位置。

## 4. 扩展约定

### 新增数据适配器

1. 扩展 `DashboardDataSource` 联合类型；
2. 实现 `DashboardDataAdapter`；
3. 向 `dashboardRuntime` 注册；
4. 在 Rust 验证器中允许对应来源；
5. 增加适配器到统一 Dataset 的测试。

### 新增可视化组件

1. 扩展 `DashboardWidgetKind`；
2. 在 `DashboardWidgetView` 增加纯渲染分支；
3. 在组件面板和属性面板暴露配置；
4. 为默认尺寸、最小尺寸和字段映射增加测试。

## 5. 数据边界

- 仪表盘数据库查询只读；
- 查询仍受数据工作台的行数、单元格和总字节限制；
- 密码不持久化；
- Markdown 不执行 HTML 或脚本；
- 仪表盘配置按工作区隔离并参与工作区数据备份。
