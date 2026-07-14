# 完整示例：Bing 每日一图

该示例只使用 Python 标准库，不依赖浏览器。它验证 manifest 默认参数、Unicode 事件流、网络请求、JSON 解析、二进制下载、产物登记和进度报告，是检查新安装 runtime 的优先脚本包。

## 1. 目录

```text
bing_daily_image/
├── manifest.yaml
└── main.py
```

## 2. manifest.yaml

```yaml
schema: 2
id: com.drpa.bing-daily-image
name: Bing 每日一图
version: 0.3.0

entrypoint:
  runtime: python
  module: main.py
  callable: main

runtime:
  python: "3.11.*"

capabilities:
  network:
    allow:
      - "www.bing.com"
  filesystem:
    read: []
    write:
      - "$outputs"

parameters:
  - id: market
    type: string
    required: false
    default: zh-CN
  - id: image_count
    type: number
    required: false
    default: 1
  - id: open_output_directory
    type: boolean
    required: false
    default: false
```

为什么必须显式默认：

- 首次安装后工作台无需手工输入即可运行。
- `market` 决定 Bing 的地区内容。
- 接口 `n` 支持有限，代码把数量约束到 1..8。

## 3. main.py

```python
from __future__ import annotations

import json
import re
from pathlib import Path
from urllib.parse import urljoin, urlparse
from urllib.request import Request, urlopen


BING_BASE = "https://www.bing.com"


def main(ctx):
    market = str(ctx.params.get("market") or "zh-CN")
    image_count = int(ctx.params.get("image_count") or 1)
    open_output_directory = bool(ctx.params.get("open_output_directory", False))
    image_count = max(1, min(image_count, 8))

    ctx.log.info(
        "正在访问 Bing 每日一图接口，地区：%s，数量：%s",
        market,
        image_count,
    )
    archive_url = (
        f"{BING_BASE}/HPImageArchive.aspx"
        f"?format=js&idx=0&n={image_count}&mkt={market}"
    )
    payload = _read_json(archive_url)
    images = payload.get("images") or []
    if not images:
        raise RuntimeError("Bing 接口没有返回图片数据")

    metadata_path = ctx.output_file(
        "bing-daily-images.json",
        "Bing 图片元数据",
    )
    metadata_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    ctx.log.info("元数据已保存：%s", metadata_path)

    for index, image in enumerate(images, start=1):
        image_url = urljoin(BING_BASE, image["url"])
        filename = _safe_filename(
            image.get("hsh")
            or image.get("startdate")
            or f"bing-{index}"
        )
        extension = Path(urlparse(image_url).path).suffix or ".jpg"
        image_path = ctx.output_file(
            f"{filename}{extension}",
            image.get("title") or "Bing 每日一图",
        )
        ctx.log.info("正在下载图片：%s", image_url)
        _download(image_url, image_path)
        ctx.progress(
            index / len(images) * 100,
            f"已下载 {index}/{len(images)}",
        )

    ctx.log.info("Bing 每日一图下载完成")
    if open_output_directory:
        ctx.open_output_directory()


def _read_json(url: str) -> dict:
    request = Request(url, headers={"User-Agent": "DRPA-Client/0.1"})
    with urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def _download(url: str, target: Path) -> None:
    request = Request(url, headers={"User-Agent": "DRPA-Client/0.1"})
    with urlopen(request, timeout=60) as response, target.open("wb") as fp:
        fp.write(response.read())


def _safe_filename(value: str) -> str:
    return re.sub(r"[^a-zA-Z0-9._-]+", "-", value).strip("-") or "bing-image"
```

## 4. 执行过程

### 参数解析

```python
market = str(ctx.params.get("market") or "zh-CN")
image_count = int(ctx.params.get("image_count") or 1)
```

manifest 和 Python 都有相同默认，兼容工作台和直接调用。

### 数量限制

```python
image_count = max(1, min(image_count, 8))
```

脚本不会向接口请求 0 或超大数量。更严格的版本可以在越界时抛错；示例选择截断。

### 请求元数据

```text
https://www.bing.com/HPImageArchive.aspx
  ?format=js
  &idx=0
  &n=1
  &mkt=zh-CN
```

使用 `Request` 设置 User-Agent，使用 30 秒超时，按 UTF-8 解码后解析 JSON。

### 空结果

`payload.get("images") or []` 同时处理字段缺失和 null。为空时抛出业务错误，避免生成“成功但没有图片”的运行。

### 元数据产物

先登记 `bing-daily-images.json`，再以 `ensure_ascii=False` 和 UTF-8 写入，中文标题保持可读。

### URL 和文件名

- `urljoin()` 把 Bing 相对 URL 变成绝对 URL。
- `urlparse(...).path` 获取扩展名，不受 query 影响。
- 文件名优先使用 hash，其次日期，最后序号。
- 正则只保留跨平台稳定字符。

### 图片产物

每张图片调用一次 `ctx.output_file()`，label 使用接口标题。二进制用 `wb` 写入。

### 进度

图片下载完成后按 `index / total * 100` 报告。metadata 写入没有单独占用进度区间，所以第一张下载完成时可能直接到 100；对于 1 张图片的短任务是合理的。更复杂任务可以把元数据阶段放在 0..20，下载放在 20..95，收尾到 100。

## 5. 运行参数

默认：

```json
{
  "market": "zh-CN",
  "image_count": 1,
  "open_output_directory": false
}
```

多图：

```json
{
  "market": "en-US",
  "image_count": 3,
  "open_output_directory": true
}
```

## 6. 预期产物

```text
bing-daily-images.json
{hash}.jpg
```

请求 3 张时通常得到 metadata 加 3 张图。实际扩展名取决于 Bing URL。

## 7. 预期日志

```text
正在访问 Bing 每日一图接口，地区：zh-CN，数量：1
元数据已保存：...
正在下载图片：https://www.bing.com/...
已下载 1/1
Bing 每日一图下载完成
```

Runtime 事件使用 `ensure_ascii=True` 的 JSONL，所以中文在协议线上转义，Host 解码后展示中文，不受子进程控制台代码页影响。

## 8. 可改进点

生产版本可以增加：

- market 白名单。
- HTTP status 和 Content-Type 检查。
- 流式复制，避免整图载入内存。
- 图片最大大小限制。
- 下载后检查非空。
- 网络错误转换为更清晰业务消息。

流式下载版本：

```python
from shutil import copyfileobj


def _download(url: str, target: Path) -> None:
    request = Request(url, headers={"User-Agent": "DRPA-Client/0.3"})
    with urlopen(request, timeout=60) as response, target.open("wb") as output:
        content_type = response.headers.get_content_type()
        if not content_type.startswith("image/"):
            raise RuntimeError(f"响应不是图片：{content_type}")
        copyfileobj(response, output, length=1024 * 1024)
    if target.stat().st_size == 0:
        raise RuntimeError("下载图片为空")
```

## 9. 故障定位

### 接口连接失败

检查目标机网络、代理和 DNS。manifest 已声明 `www.bing.com`。记录异常类型和 reason，不输出敏感代理凭据。

### JSON 解码失败

可能收到代理错误页或上游 HTML。打印 status 与 Content-Type，不把完整页面写入日志。

### 图片 URL 访问失败

元数据接口成功不代表图片请求成功。保留已写 metadata 作为诊断产物，运行状态仍应失败。

### `stream did not contain valid UTF-8`

当前 Runtime 事件输出使用 ASCII 安全 JSON，并且包日志通过 logger 事件写入，不应把任意二进制写入 stdout。脚本不要 `print(response.read())`，更不要把图片 bytes 写到 stdout。

## 10. 为什么它是安装验收包

它同时验证：

- sealed Python 能启动。
- Runtime worker 模块可导入。
- Host/worker JSONL 协议可读中文。
- manifest 参数默认值贯通。
- 网络和标准库 TLS 可用。
- output 路径创建、产物事件和二进制写入可用。
- 运行日志、进度、成功状态可展示。

浏览器 runtime 验收应再运行一个使用 `ctx.browser()` 的包，两者结合覆盖 Python 与 Chrome 主路径。
