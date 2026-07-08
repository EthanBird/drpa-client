# DRPA Client

DRPA Client 是一个轻量级 Python RPA 桌面客户端原型，目标是让用户安装一次 GUI 程序后，可以直接导入并运行 Python 脚本包。

文档：

- [功能设计文档](docs/FUNCTIONAL_DESIGN.md)
- [开发文档](docs/DEVELOPMENT.md)

当前首版聚焦：

- Windows / Linux 跨平台桌面客户端
- PySide6 美观暗色 UI
- `.rpaz` 脚本包安装
- `manifest.yaml` 参数定义
- 每个脚本包独立 venv
- 支持脚本包内置 wheels 离线依赖
- 子进程运行脚本
- JSON Lines 实时任务日志
- SQLite 运行历史
- 脚本包卸载和环境重建
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

安装时会按 manifest 中的 `dependencies.local` 收集这些目录，并传给 pip 的 `--find-links`。如果 `strategy` 为 `offline-only`，会附加 `--no-index`，确保只从脚本包资源中安装。

## 示例脚本包

仓库内置一个示例：

```bash
cd examples/hello_web_bot
zip -r ../hello_web_bot.rpaz .
```

然后在 GUI 的“脚本包”页面导入 `examples/hello_web_bot.rpaz`。

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
