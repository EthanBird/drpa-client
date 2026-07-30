# 从 Jupyter 探索代码到稳定 RPAZ

开发工作室内置真实 Jupyter 执行链路，适合逐段验证网页、数据转换和输出格式。Notebook 不是最终入口：导出的任务仍由 manifest 指向 Python 模块并调用 `main(ctx)`。本章说明怎样避免“单元格能跑，RPAZ 包不能跑”。

## 1. 工作室 Notebook 能力

新项目包含 `notebook.ipynb`。内置 Kernel 使用 sealed Python 3.11，支持：

- 多个 code / markdown 单元格。
- 单元格执行和全部运行。
- 持久变量、执行计数。
- stdout、stderr、异常 traceback。
- 常见 Jupyter MIME 输出。
- 变量浏览。
- 重启 Kernel。

首次执行会在后台准备 Kernel。大量单元格位于可滚动工作区中，不应依赖窗口高度显示全部内容。

## 2. Notebook 适合做什么

- 试验一个请求、定位器或数据清理函数。
- 观察少量响应结构。
- 迭代 CSV/JSON/Markdown 输出。
- 验证 sealed runtime 是否包含某个库。
- 构造固定夹具并运行纯函数。

不适合长期保留在单元格状态中的内容：

- 只执行一次的全局登录对象。
- 隐藏在上方单元格的凭据。
- 依赖执行顺序但没有显式输入的变量。
- 没有超时的无限轮询。

## 3. 保持可重启性

经常点击“重启 Kernel”，然后“全部运行”。如果只有依赖旧内存状态才能成功，就还没有形成可复现代码。

坏例子：

```python
# 第 1 格某次运行过，之后一直靠内存保留
page = ChromiumPage()
```

```python
# 第 8 格直接使用 page，没有声明来源
items = page.eles("css:.item")
```

改进：

```python
def collect_items(page, limit=10):
    elements = page.eles("css:.item", timeout=10)
    return [normalize(item) for item in elements[:limit]]
```

最终入口由 `ctx.browser()` 创建 page，并显式传入函数。

## 4. 把探索代码拆成纯函数

推荐层次：

```text
main(ctx)                 负责编排和 Context
parse_config(params)      参数解析与校验
fetch/collect(...)        外部访问
normalize(...)            纯数据转换
render/write(...)         输出
```

Notebook 中先测试纯函数：

```python
def normalize_title(value):
    return " ".join(str(value or "").split())

assert normalize_title("  A\n B  ") == "A B"
```

把固定响应保存为小型 dict/list 夹具，避免每次运行都访问网络。

## 5. 从单元格重构为入口函数

探索阶段：

```python
url = "https://example.com/api/items"
payload = json.loads(urlopen(url).read())
items = payload["items"]
Path("result.json").write_text(json.dumps(items))
```

RPAZ 阶段：

```python
from __future__ import annotations

import json
from urllib.request import Request, urlopen


def main(ctx):
    limit = max(1, min(int(ctx.params.get("limit") or 20), 100))
    ctx.log.info("获取数据 · limit=%s", limit)
    items = fetch_items(limit)
    ctx.progress(75, f"已获取 {len(items)} 条")
    target = ctx.output_file("items.json", "数据结果")
    target.write_text(json.dumps(items, ensure_ascii=False, indent=2), encoding="utf-8")
    ctx.progress(100, "完成")


def fetch_items(limit: int) -> list[dict]:
    request = Request("https://example.com/api/items", headers={"User-Agent": "DRPA-Client/0.3"})
    with urlopen(request, timeout=30) as response:
        payload = json.loads(response.read().decode("utf-8"))
    return list(payload.get("items") or [])[:limit]
```

变化包括：参数显式化、超时、编码、产物目录、日志和进度。

## 6. 在 Notebook 中模拟 Context

纯函数不需要 ctx。确实要试验入口编排时，可构造最小假的 Context，但不要把 mock 放进生产入口：

```python
import logging
from pathlib import Path
from tempfile import TemporaryDirectory


class NotebookContext:
    def __init__(self, output_dir):
        self.run_id = "notebook"
        self.package_id = "local.notebook"
        self.params = {"limit": 3}
        self.package_dir = Path.cwd()
        self.output_dir = Path(output_dir)
        self.log = logging.getLogger("notebook")

    def progress(self, value, message=""):
        print(f"[{value:5.1f}%] {message}")

    def output_file(self, relative_path, label=""):
        target = self.output_dir / relative_path
        target.parent.mkdir(parents=True, exist_ok=True)
        print("artifact", label or target.name, target)
        return target
```

```python
with TemporaryDirectory() as directory:
    main(NotebookContext(directory))
```

最终仍要点击工作室“直接运行”，因为真实 Context、协议、运行目录和错误处理只有直接运行能覆盖。

## 7. 导入项目代码

Notebook 应导入项目模块，而不是复制粘贴两份逻辑：

```python
from main import normalize_records, render_markdown
```

修改 `main.py` 后，Kernel 可能缓存旧模块。开发时可以：

```python
import importlib
import main
importlib.reload(main)
```

或直接重启 Kernel，后者最接近全新运行。

## 8. 处理工作目录差异

Notebook 当前目录通常是项目根目录，但生产代码不要依赖 `Path.cwd()`。

```python
# 开发 Notebook 临时读取
fixture = Path("tests/fixture.json")

# 生产入口读取包资源
fixture = ctx.package_dir / "tests" / "fixture.json"
```

输出同理，生产中使用 `ctx.output_file()`。

## 9. 浏览器对象生命周期

Notebook 探索时容易创建多个 Chrome：

```python
page = ctx.browser()  # Notebook 中通常没有真实 ctx
```

更常见是直接创建 DrissionPage 页面，但这绕过 Host 浏览器路径。推荐在 Notebook 中只验证选择器与解析函数；浏览器集成尽快转入 `main(ctx)` 直接运行。

若临时创建 page，务必：

```python
try:
    ...
finally:
    page.quit()
```

## 10. 输出与富媒体

Notebook 可以显示表格和图，但 RPAZ 运行输出需要落盘：

```python
# Notebook
display(df.head())

# RPAZ
target = ctx.output_file("data.csv", "数据 CSV")
df.to_csv(target, index=False, encoding="utf-8-sig")
```

使用第三方库前确认它在 sealed runtime 中。为了一个简单 CSV，不必引入 pandas；标准库 `csv` 更轻量。

## 11. Notebook 整理清单

导出前：

- [ ] 重启 Kernel 后全部运行通过。
- [ ] 没有凭据保存在输出或单元格源码。
- [ ] 删除超大输出、二进制 base64 和无关调试结果。
- [ ] 核心函数已经进入 `.py` 文件。
- [ ] Notebook 只保留说明、实验和固定夹具。
- [ ] `main(ctx)` 不依赖 Notebook 全局变量。
- [ ] 使用工作室直接运行至少一次。

## 12. 常见差异表

| Notebook 成功原因 | RPAZ 失败原因 | 修复 |
|---|---|---|
| Kernel 之前已 import | 模块缺依赖 | 在 sealed runtime 锁定依赖或改用标准库 |
| 当前目录恰好正确 | Host 使用独立目录 | 使用 `ctx.package_dir` / `ctx.output_file()` |
| 浏览器来自系统环境 | 离线机器没有该浏览器 | 使用 `ctx.browser()` |
| 单元格保存了登录状态 | 新运行进程无状态 | 在入口中完成初始化/登录 |
| 手工变量类型正确 | 工作台提交 JSON 类型 | 显式转换和校验 |
| 输出写在项目根目录 | 安装区只读或产物不可见 | 使用 output_file |

完成重构后继续 [调试、测试与发布](./08_调试测试与发布.md)。

