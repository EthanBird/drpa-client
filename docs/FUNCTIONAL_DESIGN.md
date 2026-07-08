# DRPA Client 功能设计文档

本文档描述 DRPA Client 的产品功能设计。`DEVELOPMENT.md` 更偏工程实现，本文更偏产品模块、用户流程、能力边界和后续演进。

## 1. 产品定位

DRPA Client 是一款轻量级 Python RPA 桌面运行器，目标用户包括：

- 会写 Python 的自动化开发者。
- 需要运行自动化脚本但不懂命令行的业务用户。
- 希望用脚本包方式分发内部自动化能力的小团队。

核心目标：

```text
安装一次 GUI -> 导入脚本包 -> 填写参数 -> 点击运行 -> 查看日志和产物
```

第一阶段不做完整低代码流程设计器，不做大型 Commander，而是把单机 Worker 做扎实。

## 2. 产品原则

### 2.1 轻量

- 不强制部署服务端。
- 默认本地运行。
- 脚本包是 zip 兼容格式。
- 运行历史和配置优先使用 SQLite 和本地文件。

### 2.2 可分发

- GUI 安装后，不需要用户再手动安装 Python。
- 自动化能力通过 `.rpaz` 脚本包分发。
- 脚本包可以带 `requirements.txt` 和 wheels。
- 脚本包未来可以从本地、内网仓库或远程仓库安装。

### 2.3 可扩展

- 默认支持 DrissionPage。
- SDK 不绑定单一自动化引擎。
- 后续可以扩展桌面自动化、OCR、Excel、邮件、文件监听等能力。

### 2.4 可维护

- 每个脚本包独立 venv。
- 任务通过子进程运行。
- 安装、运行、日志、产物有清晰目录结构。
- 失败可诊断，环境可重建。

## 3. 用户角色

### 3.1 自动化开发者

职责：

- 编写 Python RPA 脚本。
- 编写 `manifest.yaml`。
- 准备依赖和 wheels。
- 打包 `.rpaz`。
- 分发给业务用户。

关注点：

- 包格式简单。
- 本地调试方便。
- 能声明参数。
- 能带依赖。
- 能查看日志和异常。

### 3.2 业务用户

职责：

- 安装 DRPA Client。
- 导入脚本包。
- 填写账号、日期、文件路径等参数。
- 运行任务。
- 查看结果文件。

关注点：

- 不接触命令行。
- 界面美观清晰。
- 安装失败有提示。
- 运行失败能把日志发给开发者。

### 3.3 管理者

后续扩展角色。

职责：

- 管理脚本包来源。
- 管理权限和凭据。
- 查看运行审计。
- 配置统一脚本仓库。

## 4. 功能模块总览

```text
DRPA Client
├── 首页
│   ├── 脚本包数量
│   ├── 运行次数
│   └── 成功/失败统计
├── 脚本包管理
│   ├── 安装脚本包
│   ├── 卸载脚本包
│   ├── 重建虚拟环境
│   └── 查看安装日志
├── 任务运行
│   ├── 选择脚本包
│   ├── 自动生成参数表单
│   ├── 参数校验
│   ├── 启动任务
│   ├── 停止任务
│   └── 实时日志
├── 运行历史
│   ├── 运行状态
│   ├── 开始/结束时间
│   ├── 退出码
│   ├── 输出目录
│   └── 日志文件
└── 设置
    ├── 数据目录
    ├── 浏览器路径
    ├── 主题
    └── 脚本仓库配置
```

## 5. 当前已实现功能

### 5.1 首页

展示：

- 已安装脚本包数量。
- 历史运行次数。
- 成功/失败次数。

目的：

- 让用户快速知道当前客户端状态。
- 为后续增加健康检查、环境诊断预留空间。

### 5.2 脚本包管理

当前支持：

- 导入 `.rpaz` 或 `.zip`。
- 读取 `manifest.yaml`。
- 安装到数据目录。
- 创建独立 venv。
- 安装依赖。
- 使用包内 wheels。
- 覆盖安装相同版本。
- 卸载脚本包。
- 重建脚本包虚拟环境。
- 安装/重建时显示阶段日志和 pip 输出。

脚本包表格字段：

- 名称
- ID
- 版本
- Runtime
- 目录

### 5.3 任务运行

当前支持：

- 从已安装脚本包中选择。
- 根据 manifest 参数生成表单。
- 支持基础参数类型：
  - string
  - password
  - boolean
  - integer
  - number
  - date
  - file/directory 暂用文本框
- 校验必填参数。
- 子进程启动脚本。
- 实时展示日志、进度、产物、状态、错误。
- 停止任务时清理进程树。

### 5.4 运行历史

当前支持：

- SQLite 持久化任务运行记录。
- 展示最近运行记录。
- 显示状态、退出码、输出目录、日志文件。

当前状态值：

| 状态 | 含义 |
| --- | --- |
| `running` | 正在运行 |
| `success` | 成功结束 |
| `failed` | 失败结束 |
| `cancelled` | 被取消 |

### 5.5 设置

当前支持：

- 展示数据目录。

后续需要补充：

- 浏览器路径。
- 默认下载目录。
- 主题切换。
- 脚本仓库配置。
- Python runtime 诊断。

## 6. 核心用户流程

### 6.1 安装脚本包

```text
用户打开“脚本包”
  -> 点击“安装脚本包”
  -> 选择 .rpaz
  -> GUI 解压并读取 manifest
  -> 创建/覆盖包目录
  -> 创建 venv
  -> 安装 requirements / pip 依赖
  -> 写入 install.lock
  -> 刷新脚本包列表
```

失败处理：

- manifest 缺字段：提示字段错误。
- venv 创建失败：提示 Python runtime / ensurepip 问题。
- pip 安装失败：显示 pip 输出。
- zip 路径越界：拒绝安装。

### 6.2 运行脚本

```text
用户打开“运行任务”
  -> 选择脚本包
  -> GUI 生成参数表单
  -> 用户填写参数
  -> 点击“运行”
  -> TaskRunner 创建运行记录
  -> 启动 bootstrap 子进程
  -> 用户脚本执行 main(ctx)
  -> GUI 实时显示事件
  -> 结束后更新 SQLite 运行记录
  -> 运行历史刷新
```

### 6.3 停止脚本

```text
用户点击“停止”
  -> RunningTask.stop()
  -> psutil 查找子进程树
  -> terminate 父子进程
  -> 超时后 kill
  -> TaskRunner 写入 finished 事件
  -> 运行历史状态变为 failed/cancelled
```

后续应将停止状态更准确地标记为 `cancelled`。

### 6.4 重建环境

```text
用户选择脚本包
  -> 点击“重建环境”
  -> 删除旧 venv
  -> 按 manifest 重新创建 venv
  -> 重新安装依赖
  -> 显示安装日志
```

适用场景：

- 依赖损坏。
- 更新 Python runtime 后。
- 替换 wheels 后。
- 开发者排查用户环境问题。

## 7. 脚本包能力模型

### 7.1 包元数据

脚本包必须包含：

```text
manifest.yaml
```

必填字段：

- `id`
- `name`
- `version`
- `entry`

### 7.2 参数表单

manifest 的 `params` 决定 GUI 表单。

示例：

```yaml
params:
  - name: start_date
    label: 开始日期
    type: date
    required: true

  - name: retry_count
    label: 重试次数
    type: integer
    default: 3
```

后续应支持：

- select
- multi-select
- textarea
- credential
- secret
- file
- directory
- table
- json

### 7.3 依赖来源

依赖可以来自：

1. GUI 基础 runtime。
2. 脚本包 `requirements.txt`。
3. manifest `dependencies.pip`。
4. 脚本包内置 wheels。
5. 后续脚本仓库依赖缓存。

推荐策略：

- 通用轻依赖可以声明在 `dependencies.pip`。
- 大型或内网环境依赖用 wheels 随包分发。
- 企业环境优先使用 `offline-only`。

## 8. 运行事件协议

用户脚本通过 SDK 或 stdout 产生事件。

### 8.1 日志

```json
{"type":"log","level":"info","message":"开始登录"}
```

### 8.2 进度

```json
{"type":"progress","value":60,"message":"下载完成"}
```

### 8.3 产物

```json
{"type":"artifact","path":"/path/result.xlsx","label":"结果表"}
```

### 8.4 状态

```json
{"type":"status","value":"success","message":"执行完成"}
```

### 8.5 错误

```json
{"type":"error","message":"登录失败","traceback":"..."}
```

## 9. 数据目录设计

```text
data/
├── drpa-client.sqlite3
├── packages/
│   └── <package_id>/
│       └── <version>/
│           ├── package/
│           ├── venv/
│           └── install.lock
├── logs/
│   └── <package_id>/
│       └── <run_id>.log
├── outputs/
│   └── <package_id>/
│       └── <run_id>/
└── cache/
```

## 10. 后续功能优先级

### P0：单机 Worker 必备

- 运行历史详情页。
- 打开输出目录按钮。
- 打开日志文件按钮。
- 更准确的取消状态。
- file/directory 参数选择器。
- pip 安装日志保存到文件。
- 浏览器路径设置。
- 运行失败自动截图。

### P1：脚本包产品化

- 包安装预览页。
- 包签名和 SHA256。
- 包升级和版本切换。
- 包 README 展示。
- 包图标。
- 包权限声明。
- 依赖缓存。

### P2：调度能力

- 定时任务。
- 文件夹触发。
- 手动任务队列。
- 失败重试。
- 并发限制。

### P3：小型 Commander

- 本地 Web API。
- 局域网 Worker 注册。
- 集中脚本仓库。
- 多用户权限。
- 审计日志。

## 11. 非目标

当前阶段不做：

- 完整低代码流程设计器。
- 大规模分布式调度。
- 云端账号系统。
- 企业权限中心。
- 全面桌面控件录制。

这些能力可以在 Worker 稳定后再逐步设计。
