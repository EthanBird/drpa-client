# DRPA Client 开发文档

本文档面向 DRPA Client 的开发者，说明当前项目的产品目标、代码结构、脚本包协议、运行时机制、GUI 设计、跨平台注意事项和后续开发路线。

DRPA Client 当前定位为一款轻量级 Python RPA 桌面客户端：

- 用户安装一次桌面 GUI。
- GUI 内置或管理 Python 运行环境。
- 开发者分发 `.rpaz` 脚本包。
- 用户在 GUI 中导入脚本包、填写参数、点击运行。
- 脚本可以使用 DrissionPage 完成 Web 自动化，也可以继续扩展其他 Python 自动化能力。

项目不是一开始就做完整低代码 RPA 平台，而是先实现一个稳定、可扩展、易安装的 Python RPA Worker。

## 1. 当前能力概览

首版已具备以下基础能力：

| 能力 | 状态 | 说明 |
| --- | --- | --- |
| PySide6 GUI | 已实现 | 首页、脚本包页、运行页、设置页和暗色主题 |
| 脚本包导入 | 已实现 | 支持 `.rpaz` 和 `.zip` |
| manifest 解析 | 已实现 | 使用 `manifest.yaml` 描述包信息、参数和依赖 |
| 每包 venv | 已实现 | 每个脚本包可拥有独立 Python 虚拟环境 |
| 离线 wheels | 已预留并实现基础安装 | 支持 `wheels/common`、`wheels/windows`、`wheels/linux` |
| 子进程运行 | 已实现 | GUI 不直接 import 用户脚本 |
| JSON Lines 事件 | 已实现 | 日志、进度、产物、状态通过 stdout 传回 GUI |
| SDK Context | 已实现 | 提供 `ctx.log`、`ctx.params`、`ctx.progress()`、`ctx.output_file()`、`ctx.browser()` |
| DrissionPage 集成 | 已预留并实现入口 | `ctx.browser()` 创建 DrissionPage `ChromiumPage` |
| 示例脚本包 | 已实现 | `examples/hello_web_bot` |
| SQLite 运行历史 | 已实现 | `RunStore` 持久化任务状态、日志和输出路径 |
| 包管理操作 | 已实现 | 支持卸载脚本包和重建脚本包 venv |
| 进程树停止 | 已实现 | 使用 psutil 终止任务进程及其子进程 |
| Monaco 代码编辑 | 已实现入口 | 基于 QtWebEngine 承载 Monaco Editor，参考 VS Code |
| 浏览器录制器 | 规划中 | 详见 `docs/BROWSER_RECORDER_DESIGN.md` |

## 2. 项目结构

```text
.
├── README.md
├── docs/
│   └── DEVELOPMENT.md
├── examples/
│   └── hello_web_bot/
│       ├── README.md
│       ├── main.py
│       ├── manifest.yaml
│       └── wheels/
│           └── common/
├── pyproject.toml
├── src/
│   └── drpa_client/
│       ├── app/
│       │   ├── main.py
│       │   └── ui/
│       │       ├── main_window.py
│       │       └── themes/
│       │           └── dark.qss
│       ├── core/
│       │   ├── database.py
│       │   ├── manifest.py
│       │   ├── models.py
│       │   ├── package_manager.py
│       │   ├── paths.py
│       │   ├── runtime_manager.py
│       │   └── task_runner.py
│       ├── runtime/
│       │   └── bootstrap.py
│       ├── resources/
│       │   └── default_packages/
│       │       └── bing_daily_image.rpaz
│       │   └── editor/
│       │       └── monaco.html
│       └── sdk/
│           └── context.py
├── tools/
│   └── build_default_packages.py
└── tests/
    └── test_manifest.py
```

### 2.1 `app`

`app` 层负责桌面应用入口和 PySide6 UI。

当前入口：

```text
src/drpa_client/app/main.py
```

主窗口：

```text
src/drpa_client/app/ui/main_window.py
```

主题：

```text
src/drpa_client/app/ui/themes/dark.qss
```

设计原则：

- GUI 只负责展示和交互。
- 不在窗口类中写复杂业务逻辑。
- 脚本安装、运行、依赖处理等逻辑放在 `core` 层。
- 长耗时操作必须放到线程或子进程，避免卡住主 UI。

### 2.2 `core`

`core` 是桌面客户端的业务核心。

| 文件 | 职责 |
| --- | --- |
| `models.py` | 共享数据模型，例如 `PackageManifest`、`InstalledPackage`、`TaskEvent` |
| `database.py` | SQLite 运行历史存储，例如 `RunStore` |
| `manifest.py` | 读取和校验 `manifest.yaml` |
| `package_manager.py` | 安装、列出、查找、卸载脚本包，重建包环境 |
| `runtime_manager.py` | 创建 venv、安装依赖、处理平台化 wheels |
| `task_runner.py` | 启动用户脚本子进程，接收结构化事件，更新运行历史 |
| `paths.py` | 管理跨平台数据目录 |

后续浏览器录制器建议新增：

| 模块 | 职责 |
| --- | --- |
| `core/recorder/session.py` | 启动和管理 DrissionPage 录制会话 |
| `core/recorder/events.py` | 定义录制事件模型和 schema |
| `core/recorder/generator.py` | 将录制文件生成 `.rpaz` 草稿包 |
| `app/ui/pages/recorder.py` | 录制器 GUI 页面 |

完整规范见：

```text
docs/BROWSER_RECORDER_DESIGN.md
```

### 2.3 `runtime`

`runtime` 目录包含用户脚本运行时辅助代码。

当前核心文件：

```text
src/drpa_client/runtime/bootstrap.py
```

它在子进程中执行，职责是：

1. 读取 GUI 生成的任务配置 JSON。
2. 校验入口脚本路径。
3. 加载用户脚本。
4. 构造 `Context`。
5. 调用用户脚本的 `main(ctx)`。
6. 捕获异常并输出结构化错误事件。

### 2.4 `sdk`

`sdk` 是用户脚本可以依赖的轻量 API。

当前核心类：

```python
from drpa_client.sdk import Context
```

用户脚本入口：

```python
def main(ctx):
    ctx.log.info("任务开始")
```

SDK 的目标是让业务脚本不需要知道 GUI、数据库、子进程协议等内部实现。

### 2.5 `resources`

`resources` 保存应用内置资源。

当前内置默认脚本包：

```text
src/drpa_client/resources/default_packages/bing_daily_image.rpaz
```

该包源码位于：

```text
examples/bing_daily_image/
```

重新构建默认包：

```bash
python3 tools/build_default_packages.py
```

代码编辑器资源：

```text
src/drpa_client/resources/editor/monaco.html
```

该页面使用 Monaco Editor CDN 资源，产品化安装包后续应考虑内置 Monaco 静态文件以支持离线编辑。

## 2.6 AI Skills

仓库包含给 AI 开发使用的 skill：

```text
.cursor/skills/build-rpaz-package/SKILL.md
```

该 skill 明确使用 **Python 3.11.9** 构建和验证 `.rpaz` 脚本包，并定义 manifest、入口函数、构建命令和验证清单。

## 3. 脚本包协议

脚本包使用 `.rpaz` 后缀，本质是 zip 压缩包。

推荐结构：

```text
my_bot.rpaz
├── manifest.yaml
├── main.py
├── requirements.txt
├── wheels/
│   ├── common/
│   ├── windows/
│   └── linux/
├── assets/
└── README.md
```

### 3.1 `manifest.yaml`

`manifest.yaml` 是脚本包的元数据文件。

完整示例：

```yaml
id: invoice_downloader
name: 发票下载机器人
version: 1.0.0
entry: main.py
description: 自动登录业务系统并下载发票
author: example-team

runtime:
  python: ">=3.11,<3.13"
  isolation: venv

dependencies:
  strategy: offline-first
  requirements: requirements.txt
  pip:
    - DrissionPage
    - openpyxl
  local:
    common:
      - wheels/common/*.whl
    windows:
      - wheels/windows/*.whl
    linux:
      - wheels/linux/*.whl

params:
  - name: username
    label: 用户名
    type: string
    required: true
    description: 业务系统登录账号

  - name: password
    label: 密码
    type: password
    required: true

  - name: headless
    label: 无头浏览器
    type: boolean
    default: false
```

### 3.2 必填字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | 包唯一标识，建议使用小写字母、数字和下划线 |
| `name` | string | GUI 展示名称 |
| `version` | string | 包版本 |
| `entry` | string | 入口脚本路径，相对于包根目录 |

### 3.3 `runtime`

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `python` | `>=3.11` | Python 版本约束，使用 PEP 440 specifier |
| `isolation` | `venv` | 当前支持 `venv` 和 `shared`，推荐 `venv` |

当前实现优先支持每包独立 venv。这样可以避免不同脚本包之间依赖冲突。

### 3.4 `dependencies`

| 字段 | 说明 |
| --- | --- |
| `strategy` | 依赖安装策略 |
| `requirements` | requirements 文件路径 |
| `pip` | 需要通过 pip 安装的依赖列表 |
| `local.common` | 所有平台都可用的本地 wheel 匹配规则 |
| `local.windows` | Windows 平台专用 wheel 匹配规则 |
| `local.linux` | Linux 平台专用 wheel 匹配规则 |

`strategy` 当前支持：

| 值 | 行为 |
| --- | --- |
| `offline-first` | 优先使用包内 wheels，同时允许从默认 pip index 下载缺失依赖 |
| `offline-only` | 使用 `--no-index`，只从包内 wheels 安装 |
| `online` | 允许从默认 pip index 安装，仍可传入 `--find-links` |

注意：当前代码中 `online` 和 `offline-first` 都不会附加 `--no-index`，差异主要是语义预留。后续可以进一步扩展为更严格的安装策略。

### 3.5 参数类型

当前模型支持以下参数类型：

| 类型 | GUI 控件 |
| --- | --- |
| `string` | `QLineEdit` |
| `password` | 密码模式 `QLineEdit` |
| `boolean` | `QCheckBox` |
| `integer` | 当前暂用 `QLineEdit`，后续可改为 `QSpinBox` |
| `number` | 当前暂用 `QLineEdit`，后续可改为 `QDoubleSpinBox` |
| `date` | 当前暂用 `QLineEdit`，后续可改为 `QDateEdit` |
| `file` | 当前暂用 `QLineEdit`，后续可加文件选择按钮 |
| `directory` | 当前暂用 `QLineEdit`，后续可加目录选择按钮 |

后续开发应优先完善这些参数类型的专用控件和校验逻辑。

## 4. 脚本包安装流程

入口：

```python
PackageManager.install_archive(path)
```

流程：

```text
选择 .rpaz/.zip
  -> 解压到临时 staging 目录
  -> 防 Zip Slip 安全校验
  -> 读取 manifest.yaml
  -> 复制到 data/packages/<id>/<version>/package
  -> 根据 runtime.isolation 创建 venv
  -> 根据 dependencies 安装依赖
  -> 写入 install.lock
  -> GUI 刷新脚本包列表
```

### 4.1 安装目录

数据目录由 `platformdirs` 决定，也可以通过环境变量覆盖：

```bash
DRPA_DATA_DIR=/tmp/drpa-client-data drpa-client
```

典型安装结构：

```text
data/
├── packages/
│   └── hello_web_bot/
│       └── 0.1.0/
│           ├── package/
│           ├── venv/
│           └── install.lock
├── logs/
├── outputs/
└── cache/
```

### 4.2 安全解压

`package_manager.py` 中的 `_safe_extract()` 会检查 zip 内每个成员的解压目标路径，防止如下恶意路径：

```text
../../evil.py
/absolute/path/evil.py
```

后续可以继续增强：

- 限制单包大小。
- 限制文件数量。
- 拒绝软链接。
- 做 SHA256 校验。
- 支持包签名。

## 5. 依赖安装机制

入口：

```python
RuntimeManager.ensure_environment(...)
```

### 5.1 venv 创建

当前默认每个脚本包创建独立 venv。

为了兼容部分 Linux 环境缺少 `ensurepip` 的情况，代码会先判断脚本包是否真的需要 pip：

- 如果没有 `requirements.txt`
- 且 `dependencies.pip` 为空

则创建不带 pip 的 venv：

```python
venv.EnvBuilder(with_pip=False)
```

如果脚本包需要安装依赖，则创建带 pip 的 venv：

```python
venv.EnvBuilder(with_pip=True)
```

如果 Linux 系统缺少 `python3-venv/ensurepip`，需要在产品安装包或环境准备阶段解决。

### 5.2 wheels 搜索

根据当前平台选择 `manifest.yaml` 中的本地 wheels：

```text
common + windows
common + linux
```

然后转换为 pip 参数：

```bash
python -m pip install --find-links <dir1> --find-links <dir2> ...
```

如果策略是 `offline-only`，追加：

```bash
--no-index
```

### 5.3 后续建议

依赖安装后续应增强：

1. GUI 展示详细安装进度。
2. 单独保存 pip 安装日志。
3. 支持依赖缓存目录。
4. 支持重建 venv。
5. 支持每个脚本包锁定依赖版本。
6. 支持脚本包安装前的依赖预检。
7. 支持嵌入式 Python runtime，不依赖系统 Python。

## 6. 任务运行机制

入口：

```python
TaskRunner.start(package, params, on_event)
```

运行链路：

```text
GUI
  -> TaskRunner
    -> 生成临时任务配置 JSON
    -> 选择 package venv 中的 python
    -> 启动 runtime/bootstrap.py 子进程
      -> 加载用户 main.py
      -> 构造 ctx
      -> 调用 main(ctx)
      -> stdout 输出 JSON Lines
    -> GUI 解析事件并刷新日志
```

### 6.1 为什么用子进程

不要在 GUI 主进程中直接 import 用户脚本，原因：

- 用户脚本异常不会拖垮 GUI。
- 可以终止任务。
- 可以隔离 stdout/stderr。
- 后续可加入资源限制和超时控制。
- 不同脚本包可以使用不同 venv。

### 6.2 任务配置 JSON

`TaskRunner` 会为每次运行生成临时 JSON：

```json
{
  "run_id": "20260708000000000000",
  "package_id": "hello_web_bot",
  "package_name": "Hello Web Bot",
  "package_dir": ".../package",
  "entry": "main.py",
  "params": {
    "username": "demo",
    "headless": true
  },
  "output_dir": ".../outputs/hello_web_bot/<run_id>",
  "log_file": ".../logs/hello_web_bot/<run_id>.log"
}
```

### 6.3 JSON Lines 事件协议

子进程通过 stdout 输出 JSON Lines。

日志事件：

```json
{"type":"log","level":"info","message":"任务开始"}
```

进度事件：

```json
{"type":"progress","value":50,"message":"已打开页面"}
```

产物事件：

```json
{"type":"artifact","path":"/path/to/result.xlsx","label":"结果文件"}
```

状态事件：

```json
{"type":"status","value":"success","message":"任务执行完成"}
```

错误事件：

```json
{"type":"error","message":"错误信息","traceback":"..."}
```

结束事件由父进程 `TaskRunner` 追加：

```json
{"type":"finished","exit_code":0}
```

### 6.4 停止任务

当前 `RunningTask.stop()` 使用：

```python
process.terminate()
```

后续应增强：

- Windows 下清理子进程树。
- Linux 下使用进程组。
- 通知脚本优雅退出。
- 增加强制 kill 超时。
- 清理浏览器进程。

## 7. 脚本 SDK

脚本入口文件必须提供：

```python
def main(ctx):
    ...
```

### 7.1 基础用法

```python
def main(ctx):
    username = ctx.params["username"]
    ctx.log.info("开始处理用户：%s", username)

    ctx.progress(10, "初始化完成")

    output = ctx.output_file("result.txt", "结果文件")
    output.write_text("hello\n", encoding="utf-8")

    ctx.progress(100, "完成")
```

### 7.2 DrissionPage 用法

```python
def main(ctx):
    page = ctx.browser(headless=ctx.params.get("headless", False))
    page.get("https://example.com")
    ctx.log.info("当前页面：%s", page.title)
```

`ctx.browser()` 当前会：

1. 创建 DrissionPage `ChromiumOptions`。
2. 设置 headless。
3. 将下载目录设置到本次任务输出目录下的 `downloads`。
4. 返回 `ChromiumPage`。

后续建议增加：

- 浏览器路径配置。
- 用户数据目录配置。
- 代理配置。
- 下载目录策略。
- 多标签页管理。
- 自动截图。
- 出错时自动保存页面 HTML。

### 7.3 日志

`ctx.log` 是标准 Python `logging.Logger`。

日志会同时写入：

- 子进程 stdout 的 JSON Lines 事件。
- 本地日志文件。

## 8. GUI 开发说明

当前 UI 使用 PySide6 + QSS。

### 8.1 页面结构

主窗口使用左侧导航 + 右侧 `QStackedWidget`：

```text
Sidebar
  - 首页
  - 脚本包
  - 运行任务
  - 设置

Stack
  - DashboardPage
  - PackagesPage
  - TasksPage
  - SettingsPage
```

任务运行页使用列表式布局，不使用下拉框：

```text
TasksPage
  ├── 左侧：PackageList
  │   ├── 已安装脚本包列表
  │   └── 刷新列表
  └── 右侧：RunDetail
      ├── 脚本包名称/描述/Runtime
      ├── manifest 参数表单
      ├── 运行/停止按钮
      └── 实时日志
```

这样做的原因：

- 下拉框只适合少量简单选项，不适合管理脚本包。
- 列表可以展示更多上下文，例如 ID、版本、最近运行状态。
- 后续可扩展搜索、收藏、分组、图标和脚本包状态。

### 8.2 主题

暗色主题位于：

```text
src/drpa_client/app/ui/themes/dark.qss
```

当前视觉方向：

- 深色背景。
- 卡片式布局。
- 蓝色主按钮。
- 圆角输入框和表格。
- 左侧导航高亮。

后续美化建议：

1. 引入图标。
2. 增加浅色主题。
3. 增加主题切换。
4. 增加 Toast 通知。
5. 增加空状态插画。
6. 增加安装进度弹窗。
7. 日志按 level 着色。
8. 表格增加状态 Badge。

### 8.3 线程规则

PySide6 中不能在后台线程直接修改 UI。

当前脚本包安装使用：

```python
PackageInstallWorker + QThread + Signal
```

任务运行事件使用：

```python
TaskEventBridge(QObject)
```

后续新增长耗时操作时，应保持：

- 后台执行耗时逻辑。
- 使用 Signal 回到主线程更新 UI。
- 不在子线程中直接访问 QWidget。

## 9. 跨平台开发注意事项

目标平台：

- Windows
- Linux

### 9.1 路径

统一使用：

```python
from pathlib import Path
```

不要硬编码：

```text
C:\...
/home/...
```

数据目录使用：

```python
platformdirs.user_data_dir()
```

### 9.2 Python 命令

开发环境中可能只有 `python3`，没有 `python`。

文档中面向用户可以写：

```bash
python -m pip install -e .
```

CI 或 Linux 验证脚本建议使用：

```bash
python3 -m compileall src tests examples
```

### 9.3 venv 和 ensurepip

Linux 最小环境可能缺少：

```text
python3-venv
ensurepip
```

当前代码对“无依赖脚本包”做了兼容，但需要安装依赖的脚本包仍要求可用 pip。

产品化安装包建议：

- 自带 Python runtime。
- 自带 pip。
- 不依赖系统 Python。

### 9.4 浏览器

DrissionPage 控制 Chromium 内核浏览器。

第一版建议：

- 优先使用系统 Chrome / Edge / Chromium。
- 在设置页提供浏览器路径检测和手动选择。
- 不默认内置 Chromium，避免安装包过大和 Linux 依赖复杂。

后续可选：

- Windows 附带 portable Chromium。
- Linux 提供浏览器检测诊断。
- 让脚本包声明浏览器能力需求。

## 10. 本地开发

### 10.1 安装

```bash
python3 -m pip install -e .
```

如果需要开发工具：

```bash
python3 -m pip install -e ".[dev]"
```

### 10.2 启动 GUI

```bash
drpa-client
```

或：

```bash
python3 -m drpa_client.app.main
```

### 10.3 编译检查

```bash
python3 -m compileall src tests examples
```

### 10.4 单元测试

如果安装了 pytest：

```bash
python3 -m pytest
```

当前环境未必预装 pytest，可以直接执行测试函数：

```bash
PYTHONPATH=src:. python3 - <<'PY'
from tests.test_manifest import (
    test_parse_manifest_requires_entry,
    test_parse_manifest_with_offline_dependencies,
)

test_parse_manifest_with_offline_dependencies()
test_parse_manifest_requires_entry()
print("manifest-tests-ok")
PY
```

### 10.5 端到端验证示例包

```bash
PYTHONPATH=src python3 - <<'PY'
import json
import os
import shutil
import time
import zipfile
from pathlib import Path

shutil.rmtree("/tmp/drpa-client-test-data", ignore_errors=True)
archive = Path("examples/hello_web_bot.rpaz")
source = Path("examples/hello_web_bot")

if archive.exists():
    archive.unlink()

with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
    for path in source.rglob("*"):
        if path.is_file():
            zf.write(path, path.relative_to(source))

os.environ["DRPA_DATA_DIR"] = "/tmp/drpa-client-test-data"

from drpa_client.core.package_manager import PackageManager
from drpa_client.core.task_runner import TaskRunner

manager = PackageManager()
package = manager.install_archive(archive, install_dependencies=True)

events = []
runner = TaskRunner()
task = runner.start(package, {"username": "tester", "headless": True}, events.append)

while task.process.poll() is None:
    time.sleep(0.1)

time.sleep(0.2)
print(json.dumps([event.type for event in events], ensure_ascii=False))

if not any(event.type == "finished" and event.payload.get("exit_code") == 0 for event in events):
    raise SystemExit("task did not finish successfully")

archive.unlink(missing_ok=True)
PY
```

## 11. 打包设计建议

当前项目还没有正式打包脚本。后续建议按以下方向推进。

### 11.1 Windows

推荐结构：

```text
DRPA Client/
├── drpa-client.exe
├── runtime/
│   └── python/
├── app/
├── data/
└── resources/
```

可选工具：

- PyInstaller 或 Nuitka 打包 GUI。
- Windows embeddable Python 作为脚本运行 runtime。
- NSIS / Inno Setup 生成安装包。

注意：

- GUI exe 和脚本 runtime 最好分开。
- 不建议把所有用户脚本都打进 GUI exe。
- 需要保留可被 `TaskRunner` 调用的 Python 解释器。

### 11.2 Linux

推荐先支持：

- tar.gz portable 包。
- 后续再支持 AppImage。

Linux 需要重点处理：

- Python runtime。
- Qt platform plugin。
- 系统字体。
- Chromium/Chrome 检测。
- 沙盒和权限问题。

## 12. 数据与持久化

当前实现同时使用文件结构、`install.lock` 和 SQLite：

- 脚本包安装状态：仍使用 `install.lock`，方便包目录可迁移和排障。
- 任务运行历史：使用 `drpa-client.sqlite3`，方便按时间倒序查询和统计。

当前已实现表：

```text
task_runs
```

后续建议继续扩展 SQLite，管理：

```text
packages
package_versions
tasks
schedules
settings
credentials
```

当前 `task_runs` 表：

```sql
task_runs(
  id text primary key,
  package_id text not null,
  package_name text not null,
  package_version text not null,
  status text not null,
  params_json text,
  output_dir text,
  log_file text,
  started_at text,
  finished_at text,
  exit_code integer
);
```

## 13. 安全设计

Python 脚本包本质上可以执行任意代码，因此安全边界要明确。

当前已有：

- 安全解压，防 Zip Slip。
- 入口脚本路径越界检查。
- 每包独立 venv。

后续建议：

1. 安装前显示包信息、作者、版本、依赖。
2. 计算并展示 SHA256。
3. 支持包签名。
4. 支持可信发布者。
5. 密码参数加密存储。
6. 安装和运行审计日志。
7. 可选禁用网络、文件系统等权限的声明机制。
8. 企业版可接入私有脚本仓库。

## 14. 近期开发路线

建议按以下顺序推进：

### 阶段 1：打磨当前 MVP

- 完善参数控件。
- 安装依赖时显示实时 pip 日志。
- 任务运行日志按颜色高亮。
- 支持任务停止时清理进程树。
- 设置页增加浏览器路径配置。
- 增加运行历史。

### 阶段 2：脚本包安装体验

- 安装确认页。
- 展示 manifest 摘要。
- 展示依赖列表。
- 支持重建 venv。
- 支持卸载脚本包。
- 支持升级脚本包。

### 阶段 3：DrissionPage 能力增强

- 浏览器 profile 管理。
- 自动截图。
- 出错自动保存截图和 HTML。
- 下载文件记录。
- 代理配置。
- headless/headful 切换。

### 阶段 4：产品化打包

- Windows 安装包。
- Linux portable 包。
- 内置 Python runtime。
- 首次启动环境诊断。
- 自动更新预留接口。

### 阶段 5：小型 Commander 能力

- 定时任务。
- 文件夹触发。
- Webhook/API 触发。
- 多任务队列。
- 任务失败重试。
- 本地 Web 控制台。

## 15. 贡献约定

### 15.1 代码风格

- 使用 Python 3.11+。
- 优先使用 `pathlib.Path`。
- 业务逻辑放 `core`，GUI 只做展示。
- 用户脚本运行必须保持子进程隔离。
- 新增脚本包协议字段时，同时更新本文档。

### 15.2 提交前检查

至少运行：

```bash
python3 -m compileall src tests examples
```

如果安装了测试依赖：

```bash
python3 -m pytest
```

对涉及脚本包安装或任务运行的改动，建议执行端到端示例包验证。

### 15.3 不要提交的内容

`.gitignore` 已排除：

```text
__pycache__/
*.py[cod]
*.egg-info/
.pytest_cache/
.ruff_cache/
.venv/
venv/
dist/
build/
*.rpaz
```

不要提交本地生成的脚本包、虚拟环境、构建产物和缓存文件。

## 16. 常见问题

### 16.1 为什么不直接运行用户脚本？

因为用户脚本可能崩溃、阻塞、修改全局状态或依赖不同版本的第三方库。子进程运行更稳定，也更容易停止和记录日志。

### 16.2 为什么每个脚本包独立 venv？

RPA 脚本经常依赖不同版本的浏览器库、Excel 库、OCR 库等。每包 venv 能降低依赖冲突风险，便于卸载和重建。

### 16.3 为什么 `.rpaz` 本质还是 zip？

zip 易于生成、解压和检查，跨平台支持好。使用 `.rpaz` 后缀可以让 GUI 识别这是 DRPA 脚本包。

### 16.4 为什么 GUI 不直接做低代码流程编辑器？

当前产品目标是轻量 Python RPA Worker。先把脚本包安装、运行、依赖隔离和日志做好，比一开始做复杂低代码编辑器更容易形成稳定产品。

### 16.5 DrissionPage 是否是唯一自动化引擎？

不是。DrissionPage 是默认 Web-RPA 引擎。SDK 应保持开放，后续可以支持 `pyautogui`、`pywinauto`、OCR、Excel 等能力。
