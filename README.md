# DRPA Client

DRPA Client 是一个轻量级 Python RPA 桌面客户端原型，目标是让用户安装一次 GUI 程序后，可以直接导入并运行 Python 脚本包。

文档：

- [功能设计文档](docs/FUNCTIONAL_DESIGN.md)
- [开发文档](docs/DEVELOPMENT.md)
- [浏览器录制与脚本包生成设计规范](docs/BROWSER_RECORDER_DESIGN.md)

当前首版聚焦：

- Windows / Linux 跨平台桌面客户端
- PySide6 美观 UI，支持暗色/亮色 QSS 主题切换
- `.rpaz` 脚本包安装
- 单个 `.py` 文件直接导入为脚本包
- `manifest.yaml` 参数定义
- 每个脚本包独立 venv
- 安装时可选 Runtime 策略：共享当前 Python、使用已有 venv、新建 venv
- 支持脚本包内置 wheels 离线依赖
- 仓库内置 Python 3.11/cp311 的 Windows/Linux wheelhouse：DrissionPage、requests、pandas、openpyxl 及完整依赖
- 子进程运行脚本
- JSON Lines 实时任务日志
- SQLite 运行历史
- 脚本包卸载和环境重建
- 列表式任务运行界面
- Monaco Web 代码编辑器入口
- examples 示例脚本包：Hello、Bing 每日一图、Bilibili 搜索、营销销户派工
- 浏览器录制 MVP：录制事件模型、JS Agent、草稿 `.rpaz` 生成器；作为高级功能默认隐藏，可在设置中启用
- AI skill：构建 `.rpaz` 代码包，指定 Python 3.11.9
- 面向 DrissionPage 的 SDK 入口

## 快速运行

```bash
python -m pip install -e .
drpa-client
```

如果只想做语法验证：

```bash
python -m compileall src tests
```

## 脚本包格式

脚本包使用 `.rpaz` 后缀，本质是 zip 文件：

```text
my_bot.rpaz
  manifest.yaml
  main.py
  requirements.txt
  wheels/
    common/
    windows/
    linux/
  assets/
```

`manifest.yaml` 示例：

```yaml
id: invoice_downloader
name: 发票下载机器人
version: 1.0.0
entry: main.py

runtime:
  python: ">=3.11"
  isolation: venv

dependencies:
  strategy: offline-first
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

  - name: password
    label: 密码
    type: password
    required: true
```

## 离线依赖策略

脚本包可以携带 wheel 文件：

- `wheels/common/*.whl`：所有平台通用
- `wheels/windows/*.whl`：仅 Windows 安装
- `wheels/linux/*.whl`：仅 Linux 安装

安装依赖时一律禁用联网索引，pip 始终使用 `--no-index`。manifest 中的 `strategy` 只保留兼容语义，不允许联网安装。

仓库还提供全局 wheelhouse：

```text
wheelhouse/linux-x86_64/
wheelhouse/windows-amd64/
```

安装依赖时，DRPA Client 会根据当前平台自动把对应目录加入 pip `--find-links`，因此常用依赖可以直接复用仓库里的完整 wheels。

查找优先级：

1. 安装/运行目录下的全局 `wheelhouse`
2. `.rpaz` 包内的 `wheels/common`、`wheels/windows`、`wheels/linux`

全局 wheelhouse 还会生成 constraints 文件，确保已有安装目录 wheels 的版本优先于脚本包内同名 wheels。

## 参数表与运行表单

脚本包的 `manifest.yaml` 中 `params` 字段会自动生成两部分 UI：

- 参数表：展示参数名、类型、必填、默认值和说明。
- 运行表单：根据参数类型生成输入控件，例如文本框、密码框、数字框、日期框、复选框。

因此每个任务的可配置参数不需要在 GUI 里写死，只需要维护脚本包自己的 manifest。

## 示例脚本包

仓库内置示例源码和可直接安装的 `.rpaz`：

```text
examples/hello_web_bot.rpaz
examples/bing_daily_image/
examples/bing_daily_image.rpaz
examples/bilibili_search/
examples/bilibili_search.rpaz
examples/marketing_cancel_order_dispatch/
examples/marketing_cancel_order_dispatch.rpaz
```

在 GUI 的“脚本包”页面点击“安装脚本包”，默认会打开当前运行/安装目录，方便直接选择 `examples/*.rpaz`。

其中：

- `bing_daily_image`：访问 Bing 每日一图接口，下载图片和 JSON 元数据。
- `bilibili_search`：使用 DrissionPage 打开 Bilibili 搜索页，按关键词导出搜索结果。
- `marketing_cancel_order_dispatch`：整理自营销销户派工脚本，使用参数表配置人员、计划时间和业务字段。

如需重新生成 examples 下的 `.rpaz`：

```bash
python3 tools/build_example_packages.py
```

## 脚本 SDK

脚本入口需要定义 `main(ctx)`：

```python
def main(ctx):
    ctx.log.info("任务启动")
    username = ctx.params["username"]
    page = ctx.browser(headless=True)
    page.get("https://example.com")
    ctx.progress(50, "已打开页面")
    ctx.output_file("result.xlsx")
```

`ctx.browser()` 默认创建 DrissionPage `ChromiumPage`，并把下载目录指向本次任务的输出目录。

## 目录结构

```text
src/drpa_client/
  app/          PySide6 GUI
  core/         脚本包安装、runtime、任务运行
  runtime/      子进程 bootstrap
  sdk/          用户脚本上下文 API
examples/      示例脚本包
```
