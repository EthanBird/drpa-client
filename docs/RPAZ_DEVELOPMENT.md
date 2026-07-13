# RPaz 脚本包开发

RPaz 是一个根目录包含 `manifest.yaml` 的 ZIP 文件。DRPA Next 的开发工作室可以新建项目、编辑源码、校验清单、构建 `.rpaz` 并立即安装。

## 在开发工作室创建

1. 打开“开发工作室”。
2. 输入项目名称与反向域名 ID，例如 `com.example.my-task`。
3. 点击“新建项目”。
4. 编辑 `main.py` 和 `manifest.yaml`。
5. 点击“构建并安装”，随后在“运行工作台”填写参数并运行。

编辑器完全来自应用安装包，不会访问 CDN。项目源码、构建产物和运行输出都保存在本地工作区。

## 最小目录

```text
my-task/
├── manifest.yaml
└── main.py
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
- `ctx.browser(headless=True)`：使用封装的 Chrome for Testing 与 DrissionPage。

脚本不应写死工作目录、解释器路径或浏览器路径，也不应自行调用 pip。离线依赖只能来自平台 sealed runtime 或包内经过锁定和校验的 wheel 集合。

## 手工构建

压缩时 `manifest.yaml` 必须位于归档根目录，不能多套一层项目文件夹：

```bash
cd my-task
python -m zipfile -c ../my-task.rpaz manifest.yaml main.py
```

安装器会拒绝父目录跳转、绝对路径、符号链接、重复路径、压缩炸弹和不受支持的 schema。

## Bing 示例

仓库中的 `examples/bing_daily_image.rpaz` 是一个只依赖 Python 标准库的完整测试包。安装后使用 `market=zh-CN`、`image_count=1` 运行；成功时会生成一张图片与 `bing-daily-images.json`，日志中会显示实际产物路径。
