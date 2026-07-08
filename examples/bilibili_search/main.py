from __future__ import annotations

import json
from urllib.parse import quote


def main(ctx):
    keyword = str(ctx.params.get("keyword") or "").strip()
    if not keyword:
        raise ValueError("搜索关键词不能为空")

    max_results = int(ctx.params.get("max_results") or 10)
    max_results = max(1, min(max_results, 50))
    headless = bool(ctx.params.get("headless", False))

    ctx.log.info("启动 Bilibili 搜索，关键词：%s，最大结果数：%s", keyword, max_results)
    page = ctx.browser(headless=headless)
    search_url = f"https://search.bilibili.com/all?keyword={quote(keyword)}"
    page.get(search_url)
    page.wait.load_complete()
    ctx.progress(20, "搜索页已打开")

    results = collect_results(page, max_results)
    ctx.progress(80, f"已收集 {len(results)} 条结果")

    json_path = ctx.output_file("bilibili-search-results.json", "Bilibili 搜索 JSON")
    json_path.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")

    markdown_path = ctx.output_file("bilibili-search-results.md", "Bilibili 搜索 Markdown")
    markdown_path.write_text(render_markdown(keyword, results), encoding="utf-8")

    ctx.log.info("搜索结果已保存：%s", json_path)
    ctx.progress(100, "Bilibili 搜索完成")


def collect_results(page, max_results: int) -> list[dict[str, str]]:
    selectors = [
        "css:.bili-video-card__info--tit",
        "css:.video-list-item .title",
        "css:a[href*='video']",
    ]
    seen = set()
    results: list[dict[str, str]] = []
    for selector in selectors:
        try:
            elements = page.eles(selector, timeout=5)
        except Exception:
            continue
        for element in elements:
            title = clean_text(getattr(element, "text", "") or "")
            href = safe_attr(element, "href")
            if not title or not href:
                continue
            if href.startswith("//"):
                href = "https:" + href
            key = (title, href)
            if key in seen:
                continue
            seen.add(key)
            results.append({"title": title, "url": href})
            if len(results) >= max_results:
                return results
    return results


def safe_attr(element, name: str) -> str:
    try:
        value = element.attr(name)
    except Exception:
        return ""
    return str(value or "")


def clean_text(value: str) -> str:
    return " ".join(value.split())


def render_markdown(keyword: str, results: list[dict[str, str]]) -> str:
    lines = [f"# Bilibili 搜索结果：{keyword}", ""]
    if not results:
        lines.append("没有收集到搜索结果。请检查页面结构、网络或登录状态。")
        return "\n".join(lines) + "\n"
    for index, item in enumerate(results, start=1):
        lines.append(f"{index}. [{item['title']}]({item['url']})")
    return "\n".join(lines) + "\n"
