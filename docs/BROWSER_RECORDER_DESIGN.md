# 浏览器行为录制与脚本包生成设计规范

本文档设计 DRPA Client 的浏览器录制功能。目标是把用户在浏览器中的操作录制为结构化事件，再生成一个可运行但需要人工修订的 `.rpaz` 草稿脚本包。

重要原则：

> 录制功能不是“录一次就永久可靠运行”的魔法工具，而是“把人工操作转换为可维护 Python 脚本草稿”的生产力工具。

生成脚本必须允许开发者继续修改、抽象、参数化和加固。

## 1. 目标与非目标

### 1.1 目标

- 录制浏览器中的常见操作：
  - 打开网页
  - 点击
  - 输入
  - 选择下拉项
  - 勾选复选框
  - 提交表单
  - 页面跳转
  - 下载动作提示
- 生成结构化录制文件。
- 根据录制文件生成 `.rpaz` 脚本包。
- 生成的脚本使用 DRPA SDK 和 DrissionPage。
- 自动识别需要人工处理的地方，并生成 TODO。
- 对密码、令牌、敏感字段做默认脱敏。

### 1.2 非目标

当前阶段不承诺：

- 自动处理验证码。
- 自动处理短信/邮箱/Authenticator MFA。
- 自动理解复杂业务逻辑。
- 自动推断循环、条件分支、分页、异常重试。
- 自动适配所有 iframe、shadow DOM 和复杂前端框架。
- 录制后无需人工修改。

这些场景应在生成脚本后由开发者人工补充。

## 2. 用户流程

```text
用户打开“录制器”
  -> 输入起始 URL
  -> 选择浏览器 Profile / Headless / 录制选项
  -> 点击“开始录制”
  -> DRPA 启动 Chromium 并注入录制脚本
  -> 用户手动操作网页
  -> 录制器收集事件、选择器、页面信息和截图
  -> 用户点击“停止录制”
  -> GUI 展示事件时间线
  -> 用户标记参数、删除无效步骤、修正选择器
  -> 点击“生成脚本包”
  -> 输出 draft .rpaz
  -> 开发者人工修改 main.py / manifest.yaml
  -> 在 DRPA Client 中安装并运行
```

## 3. 总体架构

```text
PySide6 Recorder UI
  ├── 起始 URL / Profile / 录制选项
  ├── 实时事件时间线
  ├── 事件详情和选择器候选
  ├── 参数化面板
  ├── 代码预览
  └── 生成脚本包

Recorder Core
  ├── BrowserRecorderSession
  │   ├── 启动 DrissionPage ChromiumPage
  │   ├── 注入 JS Recorder Agent
  │   ├── 监听导航和页面加载
  │   └── 收集事件
  ├── RecordingStore
  │   ├── recording.json
  │   ├── screenshots/
  │   └── network-hints.json
  └── ScriptPackageGenerator
      ├── manifest.yaml
      ├── main.py
      ├── recorder_notes.md
      ├── recording.json
      └── assets/
```

## 4. 技术路线

### 4.1 推荐路线：JS 注入 + CDP 辅助

录制浏览器行为推荐使用：

1. DrissionPage 启动 Chromium。
2. 通过 CDP 或页面执行 JS 注入 Recorder Agent。
3. Recorder Agent 监听 DOM 事件。
4. Python 侧定期拉取事件队列，或通过浏览器绑定接收事件。
5. DrissionPage 侧监听页面跳转、加载、下载和网络请求提示。

为什么不做纯屏幕录制：

- 屏幕坐标不稳定。
- 不利于生成可维护脚本。
- 不能可靠处理浏览器缩放、窗口大小、页面响应式布局。

坐标只能作为调试信息，不能作为生成脚本的主要定位方式。

### 4.2 JS Recorder Agent 职责

注入脚本负责：

- 捕获用户事件。
- 提取目标元素信息。
- 生成选择器候选。
- 判断字段是否敏感。
- 对输入事件做防抖合并。
- 记录事件发生时的 URL、标题、时间戳。
- 将事件放入 `window.__DRPA_RECORDER_QUEUE__`。

需要监听的事件：

| 事件 | 说明 | 生成脚本 |
| --- | --- | --- |
| `click` | 鼠标点击 | `ele(...).click()` |
| `dblclick` | 双击 | 生成 TODO，默认不自动回放 |
| `input` | 输入文本 | `ele(...).input(...)` |
| `change` | select/checkbox/radio 变化 | 根据控件类型生成 |
| `submit` | 表单提交 | 通常作为提示事件，不重复生成 |
| `keydown.enter` | 回车提交 | `actions.key_down('ENTER')` 或 TODO |
| `scroll` | 滚动 | 默认作为提示，必要时生成 `scroll.to_see()` |
| `navigation` | URL 变化 | `page.get(...)` 或 wait |
| `download` | 下载触发 | 生成 TODO 和下载目录提示 |

### 4.3 页面注入时机

需要处理：

- 初始页面。
- 页面刷新。
- 同源或跨源跳转。
- SPA 路由变化。
- 新标签页。
- iframe。

优先级：

1. 初始页面和普通跳转必须支持。
2. SPA 路由变化应记录 URL 变化。
3. iframe 先记录 frame 信息和 TODO，后续再增强自动回放。
4. shadow DOM 先记录候选信息和 TODO，后续再增强。

## 5. 录制事件规范

录制输出文件建议命名：

```text
recording.json
```

顶层结构：

```json
{
  "schema_version": "1.0",
  "tool": "drpa-browser-recorder",
  "created_at": "2026-07-08T00:00:00Z",
  "start_url": "https://example.com",
  "browser": {
    "engine": "chromium",
    "headless": false,
    "viewport": {
      "width": 1365,
      "height": 900
    }
  },
  "events": []
}
```

### 5.1 通用事件字段

每个事件都应包含：

```json
{
  "id": "evt_000001",
  "type": "click",
  "timestamp": "2026-07-08T00:00:01.234Z",
  "url": "https://example.com/login",
  "title": "Login",
  "frame": {
    "is_main": true,
    "url": "https://example.com/login",
    "name": "",
    "index_path": []
  },
  "target": {},
  "value": null,
  "sensitive": false,
  "confidence": "medium",
  "notes": []
}
```

### 5.2 target 字段

目标元素结构：

```json
{
  "tag": "input",
  "type": "text",
  "text": "",
  "label": "用户名",
  "placeholder": "请输入用户名",
  "attributes": {
    "id": "username",
    "name": "username",
    "data-testid": "login-username"
  },
  "rect": {
    "x": 320,
    "y": 240,
    "width": 260,
    "height": 36
  },
  "selectors": [
    {
      "kind": "css",
      "value": "[data-testid='login-username']",
      "score": 95,
      "reason": "data-testid is usually stable"
    },
    {
      "kind": "css",
      "value": "#username",
      "score": 80,
      "reason": "stable id"
    },
    {
      "kind": "xpath",
      "value": "//input[@name='username']",
      "score": 70,
      "reason": "name attribute"
    }
  ],
  "primary_selector": {
    "kind": "css",
    "value": "[data-testid='login-username']"
  }
}
```

### 5.3 输入事件

文本输入：

```json
{
  "id": "evt_000002",
  "type": "input",
  "value": {
    "mode": "literal",
    "text": "demo-user"
  },
  "sensitive": false
}
```

密码输入：

```json
{
  "id": "evt_000003",
  "type": "input",
  "value": {
    "mode": "param",
    "param_name": "password",
    "redacted": true
  },
  "sensitive": true
}
```

规则：

- `input[type=password]` 必须脱敏。
- `autocomplete=current-password/new-password` 必须脱敏。
- 字段名包含 `password`、`token`、`secret`、`otp` 时默认脱敏。
- 敏感值不写入 `recording.json`。

## 6. 选择器生成规范

录制脚本可维护性的核心是选择器。

### 6.1 候选选择器优先级

从高到低：

1. `data-testid` / `data-test` / `data-qa`
2. `aria-label`
3. 可关联 `label` 文本的表单控件
4. 稳定 `id`
5. `name`
6. `role` + 可见文本
7. `placeholder`
8. 短 CSS 路径
9. XPath
10. 坐标信息

坐标信息只作为调试，不作为默认回放。

### 6.2 稳定性评分

每个选择器候选要有 `score`：

| 分数 | 含义 |
| --- | --- |
| 90-100 | 很可能稳定，可直接使用 |
| 70-89 | 较稳定，建议人工检查 |
| 50-69 | 勉强可用，必须人工检查 |
| 0-49 | 不建议用于生成脚本 |

### 6.3 不稳定选择器识别

以下情况应降低分数：

- id 包含随机 hash。
- class 名看起来是 CSS Modules/hash。
- nth-child 层级太深。
- selector 依赖页面布局。
- 文本内容为空或动态变化。
- 同 selector 匹配多个元素。

## 7. 生成脚本包规范

录制器生成的包建议命名：

```text
recorded_<domain>_<timestamp>.rpaz
```

包结构：

```text
recorded_example_20260708.rpaz
├── manifest.yaml
├── main.py
├── recorder_notes.md
├── recording.json
├── selectors.json
└── assets/
    └── screenshots/
```

### 7.1 manifest.yaml

生成示例：

```yaml
id: recorded_example_login
name: 录制流程 - example 登录
version: 0.1.0
entry: main.py
description: 由浏览器录制器生成的草稿脚本包，需要人工检查和完善。
author: drpa-recorder

runtime:
  python: ">=3.11"
  isolation: venv

dependencies:
  strategy: offline-first
  pip:
    - DrissionPage

params:
  - name: start_url
    label: 起始网址
    type: string
    required: true
    default: https://example.com/login

  - name: username
    label: 用户名
    type: string
    required: true

  - name: password
    label: 密码
    type: password
    required: true
```

### 7.2 main.py 生成规范

生成脚本应该：

- 使用 `main(ctx)` 作为入口。
- 使用 `ctx.browser()` 创建页面。
- 使用参数而不是硬编码敏感值。
- 每个操作前后有简短日志。
- 对低置信度选择器生成 TODO。
- 使用 fallback selectors。
- 在关键步骤后加入等待。

示例：

```python
from __future__ import annotations


def main(ctx):
    page = ctx.browser(headless=ctx.params.get("headless", False))
    page.get(ctx.params["start_url"])
    ctx.log.info("打开起始页面：%s", ctx.params["start_url"])

    username = find_first(
        page,
        [
            "[data-testid='login-username']",
            "#username",
            "xpath://input[@name='username']",
        ],
        ctx,
        "用户名输入框",
    )
    username.input(ctx.params["username"])

    password = find_first(
        page,
        [
            "#password",
            "xpath://input[@type='password']",
        ],
        ctx,
        "密码输入框",
    )
    password.input(ctx.params["password"])

    login_button = find_first(
        page,
        [
            "text=登录",
            "xpath://button[contains(., '登录')]",
        ],
        ctx,
        "登录按钮",
    )
    login_button.click()
    page.wait.load_complete()

    # TODO: 请人工确认登录成功后的页面断言。
    ctx.log.info("录制流程执行完成")


def find_first(page, selectors, ctx, label):
    last_error = None
    for selector in selectors:
        try:
            ele = page.ele(selector, timeout=5)
            if ele:
                return ele
        except Exception as exc:  # noqa: BLE001 - generated draft helper
            last_error = exc
    raise RuntimeError(f"无法定位元素：{label}，候选选择器：{selectors}，错误：{last_error}")
```

### 7.3 recorder_notes.md

必须生成说明文件，列出：

- 录制时间。
- 起始 URL。
- 低置信度步骤。
- 被脱敏的参数。
- iframe/shadow DOM/TODO。
- 网络请求提示。
- 需要人工确认的断言。

示例：

```markdown
# 录制说明

此脚本包由 DRPA 浏览器录制器生成，不能视为最终生产脚本。

## 必须人工检查

- 第 4 步登录按钮选择器置信度为 62。
- 第 6 步发生页面跳转，但没有自动生成成功断言。
- 检测到 password 字段，已替换为 manifest 参数。
```

## 8. 人工修订规范

录制生成后，开发者至少应检查：

### 8.1 参数化

应参数化：

- 账号
- 密码
- 日期
- 文件路径
- 查询关键词
- 下载目录
- 环境 URL

不应参数化：

- 稳定按钮文本
- 固定菜单路径
- 页面结构常量

### 8.2 等待与断言

录制器只能猜测等待点。开发者应补充：

- 登录成功断言。
- 页面数据加载完成断言。
- 下载完成检查。
- 错误提示检查。
- 空数据处理。

### 8.3 业务逻辑

录制器不会可靠生成：

- 循环处理多条数据。
- 分页。
- 条件分支。
- 重试策略。
- 异常恢复。

这些必须人工写。

### 8.4 选择器修订

如果脚本中出现：

```python
# TODO: low confidence selector
```

开发者必须手动替换为更稳定的定位方式。

优先使用：

- `data-testid`
- 业务语义属性
- 表单 label
- 稳定文本

避免使用：

- 很长的 CSS path
- 深层 nth-child
- 动态 class
- 坐标

## 9. 录制 UI 设计

建议新增页面：

```text
浏览器录制
```

布局：

```text
顶部工具栏
  - 起始 URL
  - 浏览器 Profile
  - 开始录制
  - 停止录制
  - 生成脚本包

左侧
  - 事件时间线
  - 事件类型图标
  - 置信度 Badge

中间
  - 事件详情
  - 目标元素信息
  - 选择器候选列表
  - 参数化设置

右侧
  - 生成代码预览
  - TODO 列表
  - recorder_notes 预览
```

### 9.1 事件操作

用户可以：

- 删除事件。
- 合并连续输入。
- 把字面量改成参数。
- 标记字段为敏感。
- 选择主选择器。
- 添加人工等待。
- 添加断言 TODO。

## 10. Recorder Package Draft 标记

由录制器生成的 manifest 应包含扩展字段：

```yaml
recorder:
  generated: true
  schema_version: "1.0"
  source: browser
  confidence: draft
  requires_review: true
```

GUI 安装这种包时可以显示提示：

```text
该脚本包由录制器生成，仍需人工检查后再用于生产环境。
```

## 11. 事件到代码的映射

| 事件 | 生成代码 | 人工检查 |
| --- | --- | --- |
| `navigate` | `page.get(url)` | 起始 URL 是否参数化 |
| `click` | `find_first(...).click()` | 选择器和点击后等待 |
| `input` | `find_first(...).input(value)` | 是否敏感、是否参数化 |
| `select` | `select.by_text/value` 或 TODO | DrissionPage API 适配 |
| `checkbox` | 检查状态后 click | 当前状态是否稳定 |
| `scroll` | 默认注释/TODO | 是否真的需要 |
| `download` | TODO + 输出目录提示 | 下载完成判断 |
| `navigation` | `page.wait.load_complete()` | 成功断言 |

## 12. 安全与隐私

默认策略：

- 密码不保存。
- token 不保存。
- cookie 不写入脚本包。
- localStorage/sessionStorage 不写入脚本包。
- 截图默认可关闭。
- 录制文件只保存在本地。

敏感字段识别：

- input type 是 password。
- name/id/placeholder 包含：
  - password
  - passwd
  - pwd
  - token
  - secret
  - otp
  - code
  - captcha

验证码字段应生成 TODO，不应生成自动回放逻辑。

## 13. 分阶段实现路线

### 阶段 1：最小可用录制器

- 启动浏览器。
- 注入 JS。
- 捕获 click/input/change。
- 生成 `recording.json`。
- 生成草稿 `.rpaz`。
- GUI 展示事件时间线。

### 阶段 2：可编辑录制结果

- 删除事件。
- 选择主选择器。
- 参数化输入。
- 标记敏感字段。
- 生成代码预览。

### 阶段 3：可靠性增强

- iframe 支持。
- SPA 路由支持。
- 网络请求提示。
- 截图辅助。
- 低置信度检测。
- 生成断言建议。

### 阶段 4：脚本包工程化

- 录制包安装提示。
- 生成测试运行按钮。
- 对比回放结果。
- 失败自动定位到录制事件。

## 14. 关键结论

浏览器录制功能应定位为：

> 自动生成 Python RPA 草稿脚本包，并提供结构化信息帮助开发者快速修订。

设计上必须承认录制脚本不完美：

- 选择器可能脆弱。
- 等待点需要人工确认。
- 业务逻辑需要人工补写。
- 敏感信息必须参数化。
- 验证码和 MFA 只能生成 TODO。

只要规范清晰、草稿结构稳定、人工修订入口明确，这个录制器就能显著降低编写 RPA 脚本包的门槛。
