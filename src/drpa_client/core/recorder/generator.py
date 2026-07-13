from __future__ import annotations

import json
import re
import shutil
import textwrap
import zipfile
from datetime import UTC, datetime
from pathlib import Path
from urllib.parse import urlparse

from .models import Recording, RecordingEvent, SelectorCandidate


class RecorderPackageGenerator:
    """Generate a review-required .rpaz draft package from a browser recording."""

    def __init__(self, output_dir: Path):
        self.output_dir = output_dir
        self.output_dir.mkdir(parents=True, exist_ok=True)

    def generate(self, recording: Recording, package_id: str | None = None, name: str | None = None) -> Path:
        package_id = package_id or _default_package_id(recording.start_url)
        name = name or f"录制流程 - {_display_domain(recording.start_url)}"
        build_dir = self.output_dir / package_id
        if build_dir.exists():
            shutil.rmtree(build_dir)
        build_dir.mkdir(parents=True)

        (build_dir / "manifest.yaml").write_text(
            _render_manifest(package_id, name, recording),
            encoding="utf-8",
        )
        (build_dir / "main.py").write_text(_render_main(recording), encoding="utf-8")
        (build_dir / "recording.json").write_text(
            json.dumps(recording.to_dict(), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        (build_dir / "selectors.json").write_text(
            json.dumps(_selector_summary(recording), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        (build_dir / "recorder_notes.md").write_text(_render_notes(recording), encoding="utf-8")

        archive_path = self.output_dir / f"{package_id}.rpaz"
        if archive_path.exists():
            archive_path.unlink()
        with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(build_dir.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(build_dir))
        return archive_path


def _render_manifest(package_id: str, name: str, recording: Recording) -> str:
    params = [
        textwrap.dedent(
            f"""\
              - name: start_url
                label: 起始网址
                type: string
                required: true
                default: "{recording.start_url}"

              - name: headless
                label: 无头浏览器
                type: boolean
                required: false
                default: false
            """
        ).rstrip()
    ]
    for param_name, label, param_type in _recorded_params(recording):
        params.append(
            textwrap.dedent(
                f"""\
                  - name: {param_name}
                    label: {label}
                    type: {param_type}
                    required: true
                """
            ).rstrip()
        )

    manifest = textwrap.dedent(
        f"""\
        id: {package_id}
        name: {name}
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
          local:
            common:
              - wheels/common/*.whl
            windows:
              - wheels/windows/*.whl
            linux:
              - wheels/linux/*.whl

        recorder:
          generated: true
          schema_version: "{recording.schema_version}"
          source: browser
          confidence: draft
          requires_review: true

        params:
        """
    )
    return manifest + "\n" + "\n\n".join(params) + "\n"


def _render_main(recording: Recording) -> str:
    lines = [
        "from __future__ import annotations",
        "",
        "",
        "def main(ctx):",
        "    page = ctx.browser(headless=ctx.params.get('headless', False))",
        "    page.get(ctx.params['start_url'])",
        "    ctx.log.info('打开起始页面：%s', ctx.params['start_url'])",
        "",
    ]
    for event in recording.events:
        lines.extend(_render_event(event))
        lines.append("")
    lines.extend(
        [
            "    # TODO: 请人工补充业务成功断言、异常处理和必要的等待条件。",
            "    ctx.log.info('录制草稿流程执行完成')",
            "",
            "",
            "def find_first(page, selectors, ctx, label):",
            "    last_error = None",
            "    for selector in selectors:",
            "        try:",
            "            ele = page.ele(selector, timeout=5)",
            "            if ele:",
            "                return ele",
            "        except Exception as exc:  # noqa: BLE001 - generated draft helper",
            "            last_error = exc",
            "    raise RuntimeError(f'无法定位元素：{label}，候选选择器：{selectors}，错误：{last_error}')",
        ]
    )
    return "\n".join(lines) + "\n"


def _render_event(event: RecordingEvent) -> list[str]:
    label = _event_label(event)
    selector_values = [_format_selector(selector) for selector in event.selectors[:3]]
    if event.type == "navigate":
        if event.url:
            return [
                f"    ctx.log.info('导航到录制 URL：{event.url}')",
                f"    page.get({event.url!r})",
                "    page.wait.load_complete()",
            ]
        return ["    # TODO: 录制到导航事件，但缺少 URL。"]

    if not selector_values:
        return [
            f"    # TODO: {event.id} {event.type} 缺少可靠选择器，请人工补充。",
        ]

    selector_literal = _python_list(selector_values)
    if event.type == "click":
        return [
            f"    ctx.log.info('点击：{label}')",
            f"    find_first(page, {selector_literal}, ctx, {label!r}).click()",
            "    page.wait.load_complete()",
        ]
    if event.type == "input":
        value = event.value or {}
        if event.sensitive or value.get("redacted"):
            param_name = str(value.get("param_name") or _param_name(label, "input_value"))
            return [
                f"    ctx.log.info('输入敏感参数：{label}')",
                f"    find_first(page, {selector_literal}, ctx, {label!r}).input(ctx.params.get({param_name!r}, ''))",
                f"    # TODO: 请在 manifest.yaml 中确认参数 {param_name!r} 的 label/type/required。",
            ]
        text = str(value.get("text", ""))
        return [
            f"    ctx.log.info('输入：{label}')",
            f"    find_first(page, {selector_literal}, ctx, {label!r}).input({text!r})",
            "    # TODO: 如该输入值会变化，请改为 ctx.params 参数。",
        ]
    if event.type == "change":
        return [
            f"    ctx.log.info('变更控件：{label}')",
            f"    find_first(page, {selector_literal}, ctx, {label!r}).click()",
            "    # TODO: 请人工确认 select/checkbox/radio 的回放逻辑。",
        ]
    return [
        f"    # TODO: 录制事件 {event.id} 类型 {event.type} 需要人工实现。目标：{label}",
    ]


def _render_notes(recording: Recording) -> str:
    todo_events = []
    sensitive_events = []
    low_confidence = []
    for event in recording.events:
        if event.sensitive:
            sensitive_events.append(event)
        if event.confidence in {"low", "unknown"} or (
            event.primary_selector and event.primary_selector.score < 70
        ):
            low_confidence.append(event)
        if not event.selectors and event.type not in {"navigate"}:
            todo_events.append(event)

    return textwrap.dedent(
        f"""\
        # 录制说明

        该脚本包由 DRPA 浏览器录制器生成，不能视为最终生产脚本。

        - 起始 URL：{recording.start_url}
        - 录制时间：{recording.created_at}
        - 事件数量：{len(recording.events)}

        ## 必须人工检查

        - 补充业务成功断言。
        - 检查所有选择器是否稳定。
        - 参数化账号、日期、查询条件、文件路径等会变化的数据。
        - 为页面加载、下载、分页和异常提示补充等待与处理逻辑。

        ## 敏感字段

        { _event_markdown_list(sensitive_events) }

        ## 低置信度事件

        { _event_markdown_list(low_confidence) }

        ## 缺少自动回放逻辑的事件

        { _event_markdown_list(todo_events) }
        """
    )


def _selector_summary(recording: Recording) -> list[dict[str, object]]:
    return [
        {
            "event_id": event.id,
            "event_type": event.type,
            "label": _event_label(event),
            "selectors": [selector.to_dict() for selector in event.selectors],
            "primary_selector": event.primary_selector.to_dict() if event.primary_selector else None,
            "requires_review": event.confidence in {"low", "unknown"}
            or not event.primary_selector
            or event.primary_selector.score < 70,
        }
        for event in recording.events
    ]


def _default_package_id(start_url: str) -> str:
    domain = _display_domain(start_url) or "recorded_flow"
    stamp = datetime.now(UTC).strftime("%Y%m%d%H%M%S")
    return _slug(f"recorded_{domain}_{stamp}")


def _display_domain(start_url: str) -> str:
    host = urlparse(start_url).netloc or "browser"
    return host.split(":")[0]


def _slug(value: str) -> str:
    return re.sub(r"[^a-zA-Z0-9_]+", "_", value).strip("_").lower() or "recorded_flow"


def _recorded_params(recording: Recording) -> list[tuple[str, str, str]]:
    params: dict[str, tuple[str, str, str]] = {}
    for event in recording.events:
        if event.type != "input":
            continue
        value = event.value or {}
        if event.sensitive or value.get("redacted"):
            label = _event_label(event)
            name = _slug(str(value.get("param_name") or label or "secret_value"))
            params[name] = (name, label or name, "password")
    return list(params.values())


def _event_label(event: RecordingEvent) -> str:
    target = event.target or {}
    for key in ("label", "text", "placeholder"):
        value = target.get(key)
        if value:
            return str(value).strip()[:80]
    attributes = target.get("attributes") or {}
    for key in ("data-testid", "aria-label", "name", "id"):
        value = attributes.get(key)
        if value:
            return str(value).strip()[:80]
    return event.id or event.type


def _format_selector(selector: SelectorCandidate) -> str:
    if selector.kind == "xpath" and not selector.value.startswith("xpath:"):
        return f"xpath:{selector.value}"
    if selector.kind == "text" and not selector.value.startswith("text="):
        return f"text={selector.value}"
    return selector.value


def _python_list(values: list[str]) -> str:
    return "[" + ", ".join(repr(value) for value in values) + "]"


def _param_name(label: str, fallback: str) -> str:
    value = _slug(label)
    return value if value else fallback


def _event_markdown_list(events: list[RecordingEvent]) -> str:
    if not events:
        return "- 无"
    return "\n".join(f"- `{event.id}` {event.type}：{_event_label(event)}" for event in events)
