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

    ctx.log.info("正在访问 Bing 每日一图接口，地区：%s，数量：%s", market, image_count)
    archive_url = (
        f"{BING_BASE}/HPImageArchive.aspx?format=js&idx=0&n={image_count}&mkt={market}"
    )
    payload = _read_json(archive_url)
    images = payload.get("images") or []
    if not images:
        raise RuntimeError("Bing 接口没有返回图片数据")

    metadata_path = ctx.output_file("bing-daily-images.json", "Bing 图片元数据")
    metadata_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    ctx.log.info("元数据已保存：%s", metadata_path)

    for index, image in enumerate(images, start=1):
        image_url = urljoin(BING_BASE, image["url"])
        filename = _safe_filename(image.get("hsh") or image.get("startdate") or f"bing-{index}")
        extension = Path(urlparse(image_url).path).suffix or ".jpg"
        image_path = ctx.output_file(f"{filename}{extension}", image.get("title") or "Bing 每日一图")
        ctx.log.info("正在下载图片：%s", image_url)
        _download(image_url, image_path)
        ctx.progress(index / len(images) * 100, f"已下载 {index}/{len(images)}")

    ctx.log.info("Bing 每日一图下载完成")
    if open_output_directory:
        ctx.log.info("正在 Windows 资源管理器中打开输出目录")
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
