# DrissionPage 浏览器自动化

DRPA 的浏览器入口是 `ctx.browser()`。它使用安装包中的 Chrome for Testing、DrissionPage 和本次运行独立下载目录，避免依赖系统浏览器路径。本文以 DrissionPage 4.x 常用语法为主，并说明 RPAZ 中的推荐组织方式。

## 1. 获取浏览器

```python
def main(ctx):
    headless = bool(ctx.params.get("headless", False))
    page = ctx.browser(headless=headless)
    try:
        page.get("https://example.com")
        ctx.log.info("页面已打开 · title=%s", page.title)
    finally:
        page.quit()
```

`ctx.browser()` 已完成：

- 从 `DRPA_BROWSER_PATH` 设置封装浏览器位置。
- 根据参数设置 headless。
- 将浏览器下载目录设为本次 output 下的 `downloads/`。

不要再创建 `ChromiumOptions().set_browser_path("C:/...")`。确有高级选项时，优先扩展 Context 能力或在目标离线 runtime 中验证后再使用。

## 2. 打开页面

```python
page.get("https://example.com")
```

常用页面信息：

```python
ctx.log.info("title=%s", page.title)
ctx.log.info("url=%s", page.url)
html = page.html
```

访问后进行业务验证，不要只检查 `page.get()` 是否返回：

```python
page.get(login_url)
if "登录" not in page.title and not page.ele("css:form", timeout=5):
    raise RuntimeError("登录页面结构不符合预期")
```

## 3. 页面加载与等待

网页自动化最常见的不稳定来源是固定 `sleep()`。优先等待元素或业务状态：

```python
search_box = page.ele("css:input[type='search']", timeout=10)
if not search_box:
    raise RuntimeError("搜索框在 10 秒内未出现")
```

对必须完成页面加载的场景：

```python
page.wait.load_complete()
```

对异步应用，load complete 不代表列表数据已经出现，仍应等待目标元素：

```python
cards = page.eles("css:.result-card", timeout=10)
if not cards:
    ctx.log.warning("页面已加载，但结果列表为空")
```

固定等待只用于无法观察的短动画，并注明原因：

```python
page.wait(0.3)  # 等待菜单收起动画，避免点击被遮挡
```

## 4. 定位器总览

常用定位器：

```python
page.ele("css:#submit")
page.ele("xpath://button[@type='submit']")
page.ele("@id=submit")
page.ele("@name=username")
page.ele("tag:button")
page.ele("text=登录")
```

获取多个元素：

```python
items = page.eles("css:.result-item", timeout=10)
for item in items:
    print(item.text)
```

在元素内部继续查找：

```python
card = page.ele("css:.result-card")
title = card.ele("css:.title")
link = card.ele("tag:a")
```

推荐优先级：

1. 稳定业务属性：`id`、`name`、`data-testid`、`aria-label`。
2. 稳定结构和语义标签。
3. 简短 CSS 组合。
4. XPath，用于文本关系或复杂层级。
5. 易变 class、绝对 XPath、索引作为最后手段。

## 5. 定位元素

### CSS

```python
page.ele("css:input[name='keyword']")
page.ele("css:button[data-action='search']")
page.eles("css:main article.result")
```

CSS 适合稳定属性和结构。避免复制浏览器开发者工具生成的超长 `body > div:nth-child(...)`。

### XPath

```python
page.ele("xpath://button[contains(normalize-space(.), '提交')]")
page.ele("xpath://label[contains(., '用户名')]/following::input[1]")
```

XPath 文本定位容易受语言、空白和 A/B 文案影响，应保留属性定位回退。

### 多定位器回退

```python
def find_first(owner, selectors, *, timeout=3):
    for selector in selectors:
        try:
            element = owner.ele(selector, timeout=timeout)
        except Exception:
            continue
        if element:
            return element
    return None


search = find_first(page, [
    "@data-testid=search-input",
    "css:input[name='keyword']",
    "xpath://input[contains(@placeholder, '搜索')]",
])
if not search:
    raise RuntimeError("没有找到搜索框")
```

日志中可以记录尝试了哪些**选择器名称**，不要把页面敏感内容完整输出。

## 6. 读取元素信息

```python
element = page.ele("css:a.result-title")

title = element.text
href = element.attr("href")
all_attributes = element.attrs
tag = element.tag
```

链接处理：

```python
from urllib.parse import urljoin

href = str(element.attr("href") or "")
absolute = urljoin(page.url, href)
```

文本清理：

```python
def clean_text(value: str) -> str:
    return " ".join(str(value or "").split())
```

读取属性应容忍缺失：

```python
def safe_attr(element, name: str) -> str:
    try:
        return str(element.attr(name) or "")
    except Exception:
        return ""
```

## 7. 输入与点击

```python
username = page.ele("@name=username")
username.input("demo-user", clear=True)

submit = page.ele("css:button[type='submit']")
submit.click()
```

对敏感字段不要记录值：

```python
password = str(ctx.params.get("account_password") or "")
if not password:
    raise ValueError("缺少账号密码")
page.ele("@name=password").input(password, clear=True)
ctx.log.info("登录表单已填写")
```

点击后等待结果，而不是立刻读取：

```python
submit.click()
dashboard = page.ele("@data-testid=dashboard", timeout=15)
if not dashboard:
    error = page.ele("css:.login-error", timeout=1)
    message = clean_text(error.text) if error else "未知登录失败"
    raise RuntimeError(message)
```

## 8. 表单、选择与键盘

普通文本输入使用 `.input()`。需要模拟按键时可使用元素 actions/keys 能力；不同 DrissionPage 小版本的高级动作 API 可能变化，发布前以当前 sealed runtime 实测为准。

稳定做法是优先调用页面自身可见控件：

```python
box.input(keyword, clear=True)
page.ele("css:button.search-submit").click()
```

对于自定义下拉：

```python
page.ele("@data-testid=region-select").click()
option = page.ele("xpath://div[@role='option' and normalize-space(.)='华东']", timeout=5)
if not option:
    raise RuntimeError("地区选项没有出现")
option.click()
```

## 9. 标签页和窗口

网页可能打开新标签页。处理前记录当前 tab，触发后获取新 tab，并在使用后关闭。DrissionPage 4.x 提供页面 tab 管理接口；因为具体站点行为不同，建议将新窗口操作封装成小函数并在当前 runtime 回归。

设计原则：

- 不假设新 tab 一定是列表最后一个，确认 URL 或标题。
- 使用完成后关闭，避免长任务积累标签页。
- finally 中关闭整个 page。

## 10. iframe

元素位于 iframe 时，主文档定位器找不到它。先找到 frame，再在 frame 内定位：

```python
frame = page.get_frame("css:iframe.payment-frame")
pay_button = frame.ele("css:button.confirm", timeout=10)
```

frame 可能跨域或动态重建。每次页面刷新后重新获取 frame 对象，不长期缓存失效元素。

## 11. 网络监听

对前端页面背后的 JSON API，网络监听通常比解析 DOM 更稳定。典型流程：

```python
page.listen.start("api/search")
page.get(search_url)
packet = page.listen.wait(timeout=15)
if not packet:
    raise RuntimeError("没有捕获到搜索接口响应")

body = packet.response.body
```

监听目标要尽量具体。响应 body 可能是 dict、文本或 bytes，先检查类型。不要在日志中输出完整响应，尤其是包含账号或令牌时。

## 12. 下载

`ctx.browser()` 已把浏览器下载目录设为：

```text
ctx.output_dir / downloads
```

点击下载后，等待任务完成，再通过 `ctx.output_file()` 登记最终文件。如果浏览器直接写入 downloads，可以在完成后把文件复制/移动到登记路径：

```python
from shutil import copy2

downloaded = wait_for_download(ctx.output_dir / "downloads")
target = ctx.output_file(f"exports/{downloaded.name}", "网页导出文件")
copy2(downloaded, target)
```

等待下载时排除 `.crdownload` 等临时文件，并设置超时和最大文件数。

## 13. 截图与诊断

失败时截图很有价值，但不要每一步都截图：

```python
try:
    run_flow(page)
except Exception:
    screenshot = ctx.output_file("diagnostics/failure.png", "失败页面截图")
    page.get_screenshot(path=str(screenshot), full_page=True)
    ctx.log.exception("浏览器流程失败")
    raise
```

如果页面可能包含隐私数据，应提供开关控制诊断截图，并在文档中说明。

## 14. 页面结构变化策略

```python
def collect_results(page, limit: int, ctx) -> list[dict[str, str]]:
    selectors = [
        "@data-testid=result-title",
        "css:.result-card .title",
        "css:a[href*='/detail/']",
    ]
    for selector in selectors:
        elements = page.eles(selector, timeout=4)
        if elements:
            ctx.log.info("结果定位成功 · strategy=%s · count=%s", selector, len(elements))
            return normalize(elements[:limit])
    raise RuntimeError("页面结构已变化：没有找到结果项")
```

不要无声返回空列表。空业务结果和 selector 失效需要用页面状态区分。

## 15. 浏览器任务完整骨架

```python
from __future__ import annotations

import json
from urllib.parse import quote


def main(ctx):
    keyword = str(ctx.params.get("keyword") or "").strip()
    if not keyword:
        raise ValueError("keyword 不能为空")
    limit = max(1, min(int(ctx.params.get("limit") or 10), 50))
    headless = bool(ctx.params.get("headless", True))

    ctx.log.info("搜索开始 · keyword=%s · limit=%s", keyword, limit)
    page = ctx.browser(headless=headless)
    try:
        page.get(f"https://example.com/search?q={quote(keyword)}")
        page.wait.load_complete()
        ctx.progress(25, "搜索页已打开")
        results = collect_results(page, limit, ctx)
        ctx.progress(80, f"已采集 {len(results)} 条")
    finally:
        page.quit()

    target = ctx.output_file("search-results.json", "搜索结果")
    target.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    ctx.log.info("搜索完成 · results=%s · output=%s", len(results), target.name)
    ctx.progress(100, "搜索完成")
```

## 16. 官方参考

- [DrissionPage 浏览器启动配置](https://www.drissionpage.cn/dp40docs/ChromiumPage/browser_opt/)
- [查找元素](https://drissionpage.cn/dp40docs/get_elements/find_in_object/)
- [获取元素信息](https://drissionpage.cn/dp40docs/ChromiumPage/get_ele_info/)
- [网络监听](https://drissionpage.cn/dp40docs/ChromiumPage/listener/)
- [浏览器下载](https://drissionpage.cn/dp40docs/download/browser/)
- [页面交互](https://drissionpage.cn/dp40docs/ChromiumPage/page_operation/)

这些链接需要网络；本章保留完成常规 RPAZ 的核心离线用法。

