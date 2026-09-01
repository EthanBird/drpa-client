"""Constrained document worker used by the desktop AI Agent.

The worker accepts exactly one JSON request on stdin and emits exactly one JSON
response on stdout.  It deliberately has no code-execution operation: every
request is dispatched through the fixed read/create/convert handlers below.
"""

from __future__ import annotations

import json
import os
import re
import sys
import textwrap
import uuid
import zipfile
from contextlib import redirect_stdout
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable

PROTOCOL_VERSION = 1
SUPPORTED_FORMATS = frozenset({"pdf", "docx", "xlsx", "pptx"})
MACRO_FORMATS = frozenset({"docm", "dotm", "xlsm", "xltm", "pptm", "potm", "ppsm"})
SESSION_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$")
INVALID_XML_CHARACTER_RE = re.compile(
    "[\x00-\x08\x0b\x0c\x0e-\x1f\ud800-\udfff\ufffe\uffff]"
)

MAX_REQUEST_BYTES = 6 * 1024 * 1024
MAX_INPUT_BYTES = 100 * 1024 * 1024
MAX_OUTPUT_BYTES = 100 * 1024 * 1024
MAX_TEXT_BYTES = 5 * 1024 * 1024
MAX_ZIP_ENTRIES = 20_000
MAX_ZIP_UNCOMPRESSED_BYTES = 250 * 1024 * 1024
MAX_ZIP_COMPRESSION_RATIO = 250
MAX_PDF_PAGES = 2_000
MAX_SHEETS = 256
MAX_ROWS_PER_SHEET = 100_000
MAX_COLUMNS_PER_SHEET = 1_024
MAX_SLIDES = 2_000
MAX_EXCEL_CELL_CHARACTERS = 32_767


class DocumentError(Exception):
    """An expected, user-facing document processing failure."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


@dataclass
class TextBudget:
    remaining: int = MAX_TEXT_BYTES
    truncated: bool = False

    def take(self, value: Any) -> str:
        text = "" if value is None else str(value)
        encoded = text.encode("utf-8")
        if len(encoded) <= self.remaining:
            self.remaining -= len(encoded)
            return text
        self.truncated = True
        clipped = encoded[: self.remaining]
        while clipped:
            try:
                result = clipped.decode("utf-8")
                self.remaining = 0
                return result
            except UnicodeDecodeError:
                clipped = clipped[:-1]
        self.remaining = 0
        return ""


def _require_mapping(value: Any, field: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise DocumentError("invalid_request", f"{field} 必须是对象")
    return value


def _require_string(request: dict[str, Any], field: str) -> str:
    value = request.get(field)
    if not isinstance(value, str) or not value.strip():
        raise DocumentError("invalid_request", f"{field} 不能为空")
    return value.strip()


def _validate_session(value: Any) -> str:
    if not isinstance(value, str) or not SESSION_RE.fullmatch(value):
        raise DocumentError("invalid_session", "session_id 只能包含字母、数字、_ 和 -")
    return value


def _resolved_root(request: dict[str, Any]) -> Path:
    root = Path(_require_string(request, "workspace_root"))
    try:
        resolved = root.resolve(strict=True)
    except OSError as exc:
        raise DocumentError("invalid_workspace", "工作区不存在") from exc
    if not resolved.is_dir():
        raise DocumentError("invalid_workspace", "工作区不是目录")
    return resolved


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def _session_roots(root: Path, session_id: str) -> tuple[Path, Path]:
    attachments = (root / "agent" / "attachments" / session_id / "files").resolve()
    artifacts = (root / "agent" / "artifacts" / session_id / "files").resolve()
    if not _is_within(attachments, root) or not _is_within(artifacts, root):
        raise DocumentError("path_denied", "会话文档目录不能通过链接指向工作区外")
    return attachments, artifacts


def _input_path(request: dict[str, Any], root: Path, session_id: str) -> Path:
    raw = Path(_require_string(request, "input_path"))
    try:
        path = raw.resolve(strict=True)
    except OSError as exc:
        raise DocumentError("input_not_found", "文档不存在") from exc
    attachments, artifacts = _session_roots(root, session_id)
    if not (_is_within(path, attachments) or _is_within(path, artifacts)):
        raise DocumentError("path_denied", "只能读取当前会话已导入的附件或生成的产物")
    if not path.is_file() or path.is_symlink():
        raise DocumentError("invalid_input", "输入必须是普通文件")
    size = path.stat().st_size
    if size > MAX_INPUT_BYTES:
        raise DocumentError("input_too_large", f"输入文件不能超过 {MAX_INPUT_BYTES} 字节")
    _validate_extension(path)
    _inspect_ooxml(path)
    return path


def _output_path(request: dict[str, Any], root: Path, session_id: str) -> Path:
    raw = Path(_require_string(request, "output_path"))
    path = raw.resolve(strict=False)
    _, artifacts = _session_roots(root, session_id)
    if not _is_within(path, artifacts):
        raise DocumentError("path_denied", "产物只能写入当前会话的 artifacts 目录")
    if path.parent != artifacts:
        raise DocumentError("path_denied", "产物路径不允许创建额外子目录")
    if path.exists():
        raise DocumentError("output_exists", "产物已存在，禁止覆盖")
    artifacts.mkdir(parents=True, exist_ok=True)
    _validate_extension(path)
    return path


def _format_for_path(path: Path) -> str:
    return path.suffix.lower().lstrip(".")


def _validate_extension(path: Path) -> str:
    document_format = _format_for_path(path)
    if document_format in MACRO_FORMATS:
        raise DocumentError("macro_format_denied", "不支持带宏的 Office 文档")
    if document_format not in SUPPORTED_FORMATS:
        raise DocumentError(
            "unsupported_format",
            "仅支持 PDF、DOCX、XLSX 和 PPTX 文档",
        )
    return document_format


def _inspect_ooxml(path: Path) -> None:
    if _format_for_path(path) not in {"docx", "xlsx", "pptx"}:
        return
    if not zipfile.is_zipfile(path):
        raise DocumentError("invalid_ooxml", "Office 文档不是有效的 OOXML 文件")
    total_uncompressed = 0
    try:
        with zipfile.ZipFile(path) as archive:
            entries = archive.infolist()
            if len(entries) > MAX_ZIP_ENTRIES:
                raise DocumentError("zip_bomb", "Office 文档包含过多文件")
            for entry in entries:
                member = PurePosixPath(entry.filename.replace("\\", "/"))
                if member.is_absolute() or ".." in member.parts:
                    raise DocumentError("invalid_ooxml", "Office 文档包含不安全路径")
                lower_name = entry.filename.lower()
                if "vbaproject.bin" in lower_name or "/macrosheets/" in lower_name:
                    raise DocumentError("macro_format_denied", "Office 文档包含宏内容")
                if lower_name.endswith((".xml", ".rels")):
                    with archive.open(entry) as member_file:
                        preview = member_file.read(1024 * 1024).lower()
                    if b"<!doctype" in preview or b"<!entity" in preview:
                        raise DocumentError("invalid_ooxml", "Office 文档包含不安全 XML")
                total_uncompressed += entry.file_size
                if total_uncompressed > MAX_ZIP_UNCOMPRESSED_BYTES:
                    raise DocumentError("zip_bomb", "Office 文档解压后过大")
                if (
                    entry.file_size > 1_000_000
                    and entry.compress_size > 0
                    and entry.file_size / entry.compress_size > MAX_ZIP_COMPRESSION_RATIO
                ):
                    raise DocumentError("zip_bomb", "Office 文档压缩率异常")
    except zipfile.BadZipFile as exc:
        raise DocumentError("invalid_ooxml", "Office 文档已损坏") from exc


def _dependency(module: str, package: str) -> Any:
    try:
        return __import__(module, fromlist=["*"])
    except ImportError as exc:
        raise DocumentError(
            "dependency_missing",
            f"文档运行时缺少 {package}，请安装后重试",
        ) from exc


def _read_pdf(path: Path) -> dict[str, Any]:
    pypdf = _dependency("pypdf", "pypdf")
    try:
        reader = pypdf.PdfReader(str(path), strict=True)
        if reader.is_encrypted:
            raise DocumentError("encrypted_document", "不支持加密 PDF")
        if len(reader.pages) > MAX_PDF_PAGES:
            raise DocumentError("document_too_complex", "PDF 页数过多")
        budget = TextBudget()
        pages: list[dict[str, Any]] = []
        for index, page in enumerate(reader.pages):
            text = budget.take(page.extract_text() or "")
            pages.append({"pageNumber": index + 1, "text": text})
            if budget.remaining == 0:
                break
        joined = "\n\n".join(page["text"] for page in pages)
        metadata = reader.metadata or {}
        return {
            "format": "pdf",
            "title": str(metadata.get("/Title", "") or ""),
            "text": joined,
            "pages": pages,
            "pageCount": len(reader.pages),
            "truncated": budget.truncated or len(pages) < len(reader.pages),
        }
    except DocumentError:
        raise
    except Exception as exc:
        raise DocumentError("document_read_failed", f"PDF 读取失败：{exc}") from exc


def _read_docx(path: Path) -> dict[str, Any]:
    docx = _dependency("docx", "python-docx")
    try:
        document = docx.Document(str(path))
        budget = TextBudget()
        paragraphs: list[str] = []
        for paragraph in document.paragraphs:
            paragraphs.append(budget.take(paragraph.text))
            if budget.remaining == 0:
                break
        tables: list[list[list[str]]] = []
        if budget.remaining:
            for table in document.tables:
                rows: list[list[str]] = []
                for row in table.rows:
                    rows.append([budget.take(cell.text) for cell in row.cells])
                    if budget.remaining == 0:
                        break
                tables.append(rows)
                if budget.remaining == 0:
                    break
        table_text = "\n\n".join(
            "\n".join("\t".join(row) for row in table) for table in tables
        )
        text = "\n".join(paragraphs)
        if table_text:
            text = f"{text}\n\n{table_text}".strip()
        title = document.core_properties.title or ""
        return {
            "format": "docx",
            "title": title,
            "text": text,
            "paragraphs": paragraphs,
            "tables": tables,
            "truncated": budget.truncated,
        }
    except DocumentError:
        raise
    except Exception as exc:
        raise DocumentError("document_read_failed", f"Word 读取失败：{exc}") from exc


def _cell_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, (str, int, float, bool)):
        text = str(value)
    else:
        text = value.isoformat() if hasattr(value, "isoformat") else str(value)
    return _safe_xml_text(text)


def _safe_xml_text(value: Any) -> str:
    return INVALID_XML_CHARACTER_RE.sub("", "" if value is None else str(value))


def _xlsx_safe_value(value: Any) -> Any:
    if value is None or isinstance(value, (bool, int, float)):
        return value
    text = _safe_xml_text(value)
    text = text[:MAX_EXCEL_CELL_CHARACTERS]
    if text.startswith(("=", "+", "-", "@")):
        return f"'{text}"
    return text


def _read_xlsx(path: Path) -> dict[str, Any]:
    openpyxl = _dependency("openpyxl", "openpyxl")
    try:
        workbook = openpyxl.load_workbook(
            filename=str(path),
            read_only=True,
            data_only=False,
            keep_links=False,
        )
        if len(workbook.sheetnames) > MAX_SHEETS:
            raise DocumentError("document_too_complex", "Excel 工作表过多")
        budget = TextBudget()
        sheets: list[dict[str, Any]] = []
        for worksheet in workbook.worksheets:
            rows: list[list[str]] = []
            for row_index, values in enumerate(worksheet.iter_rows(values_only=True)):
                if row_index >= MAX_ROWS_PER_SHEET:
                    budget.truncated = True
                    break
                if len(values) > MAX_COLUMNS_PER_SHEET:
                    raise DocumentError("document_too_complex", "Excel 列数过多")
                rows.append([budget.take(_cell_text(value)) for value in values])
                if budget.remaining == 0:
                    break
            sheets.append({"name": worksheet.title, "rows": rows})
            if budget.remaining == 0:
                break
        workbook.close()
        text = "\n\n".join(
            f"[{sheet['name']}]\n"
            + "\n".join("\t".join(row) for row in sheet["rows"])
            for sheet in sheets
        )
        return {
            "format": "xlsx",
            "title": path.stem,
            "text": text,
            "sheets": sheets,
            "sheetCount": len(workbook.sheetnames),
            "truncated": budget.truncated or len(sheets) < len(workbook.sheetnames),
        }
    except DocumentError:
        raise
    except Exception as exc:
        raise DocumentError("document_read_failed", f"Excel 读取失败：{exc}") from exc


def _shape_text(shape: Any) -> str:
    if not getattr(shape, "has_text_frame", False):
        return ""
    return "\n".join(paragraph.text for paragraph in shape.text_frame.paragraphs)


def _read_pptx(path: Path) -> dict[str, Any]:
    pptx = _dependency("pptx", "python-pptx")
    try:
        presentation = pptx.Presentation(str(path))
        if len(presentation.slides) > MAX_SLIDES:
            raise DocumentError("document_too_complex", "PowerPoint 幻灯片过多")
        budget = TextBudget()
        slides: list[dict[str, Any]] = []
        for index, slide in enumerate(presentation.slides):
            title = ""
            if slide.shapes.title is not None:
                title = budget.take(slide.shapes.title.text)
            body_parts = []
            for shape in slide.shapes:
                if shape == slide.shapes.title:
                    continue
                value = _shape_text(shape)
                if value:
                    body_parts.append(budget.take(value))
                if budget.remaining == 0:
                    break
            slides.append(
                {
                    "slideNumber": index + 1,
                    "title": title,
                    "text": "\n".join(body_parts),
                }
            )
            if budget.remaining == 0:
                break
        text = "\n\n".join(
            f"{slide['title']}\n{slide['text']}".strip() for slide in slides
        )
        return {
            "format": "pptx",
            "title": presentation.core_properties.title or path.stem,
            "text": text,
            "slides": slides,
            "slideCount": len(presentation.slides),
            "truncated": budget.truncated or len(slides) < len(presentation.slides),
        }
    except DocumentError:
        raise
    except Exception as exc:
        raise DocumentError("document_read_failed", f"PowerPoint 读取失败：{exc}") from exc


READERS: dict[str, Callable[[Path], dict[str, Any]]] = {
    "pdf": _read_pdf,
    "docx": _read_docx,
    "xlsx": _read_xlsx,
    "pptx": _read_pptx,
}


def _content_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(_content_text(item) for item in content)
    if not isinstance(content, dict):
        return "" if content is None else str(content)
    if isinstance(content.get("text"), str):
        return content["text"]
    if isinstance(content.get("paragraphs"), list):
        return "\n".join(str(value) for value in content["paragraphs"])
    if isinstance(content.get("slides"), list):
        return "\n\n".join(
            f"{slide.get('title', '')}\n{slide.get('text', slide.get('body', ''))}".strip()
            for slide in content["slides"]
            if isinstance(slide, dict)
        )
    if isinstance(content.get("sheets"), list):
        chunks = []
        for sheet in content["sheets"]:
            if not isinstance(sheet, dict):
                continue
            rows = sheet.get("rows", [])
            chunks.append(
                f"[{sheet.get('name', 'Sheet')}]\n"
                + "\n".join(
                    "\t".join(_cell_text(cell) for cell in row)
                    for row in rows
                    if isinstance(row, list)
                )
            )
        return "\n\n".join(chunks)
    return json.dumps(content, ensure_ascii=False, indent=2)


def _validate_content_size(content: Any) -> None:
    try:
        encoded = json.dumps(content, ensure_ascii=False).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise DocumentError("invalid_content", "文档内容必须可序列化为 JSON") from exc
    if len(encoded) > MAX_TEXT_BYTES:
        raise DocumentError("content_too_large", f"文档内容不能超过 {MAX_TEXT_BYTES} 字节")


def _atomic_document_write(destination: Path, writer: Callable[[Path], None]) -> None:
    temporary = destination.parent / f".{uuid.uuid4().hex}.{destination.suffix.lstrip('.')}"
    try:
        writer(temporary)
        if not temporary.is_file():
            raise DocumentError("document_create_failed", "文档生成器没有输出文件")
        if temporary.stat().st_size > MAX_OUTPUT_BYTES:
            raise DocumentError("output_too_large", "生成的文档超过大小限制")
        try:
            os.link(temporary, destination)
        except FileExistsError as exc:
            raise DocumentError("output_exists", "产物已存在，禁止覆盖") from exc
        except OSError as exc:
            if destination.exists():
                raise DocumentError("output_exists", "产物已存在，禁止覆盖") from exc
            raise DocumentError("document_create_failed", f"无法保存文档：{exc}") from exc
    finally:
        temporary.unlink(missing_ok=True)


def _create_pdf(path: Path, title: str, content: Any) -> dict[str, Any]:
    reportlab_canvas = _dependency("reportlab.pdfgen.canvas", "reportlab")
    reportlab_pagesizes = _dependency("reportlab.lib.pagesizes", "reportlab")
    reportlab_pdfmetrics = _dependency("reportlab.pdfbase.pdfmetrics", "reportlab")
    reportlab_cidfonts = _dependency("reportlab.pdfbase.cidfonts", "reportlab")
    text = _content_text(content)

    def write(destination: Path) -> None:
        page_width, page_height = reportlab_pagesizes.A4
        canvas = reportlab_canvas.Canvas(str(destination), pagesize=reportlab_pagesizes.A4)
        canvas.setTitle(title)
        font = "Helvetica"
        if any(ord(character) > 255 for character in f"{title}{text}"):
            try:
                reportlab_pdfmetrics.registerFont(
                    reportlab_cidfonts.UnicodeCIDFont("STSong-Light")
                )
                font = "STSong-Light"
            except Exception:
                font = "Helvetica"
        y = page_height - 56
        canvas.setFont(font, 18)
        for line in textwrap.wrap(title, width=45) or [""]:
            canvas.drawString(50, y, line)
            y -= 24
        y -= 12
        canvas.setFont(font, 10)
        for source_line in text.splitlines() or [""]:
            for line in textwrap.wrap(source_line, width=95) or [""]:
                if y < 50:
                    canvas.showPage()
                    canvas.setFont(font, 10)
                    y = page_height - 50
                canvas.drawString(50, y, line)
                y -= 14
        canvas.save()

    _atomic_document_write(path, write)
    return {"format": "pdf"}


def _create_docx(path: Path, title: str, content: Any) -> dict[str, Any]:
    docx = _dependency("docx", "python-docx")

    def write(destination: Path) -> None:
        document = docx.Document()
        document.core_properties.title = title
        if title:
            document.add_heading(title, level=0)
        if isinstance(content, dict) and isinstance(content.get("paragraphs"), list):
            for paragraph in content["paragraphs"]:
                document.add_paragraph(_safe_xml_text(paragraph))
        else:
            for paragraph in _content_text(content).splitlines():
                document.add_paragraph(_safe_xml_text(paragraph))
        if isinstance(content, dict):
            for table_data in content.get("tables", []):
                if not isinstance(table_data, list) or not table_data:
                    continue
                width = max((len(row) for row in table_data if isinstance(row, list)), default=0)
                if width == 0:
                    continue
                table = document.add_table(rows=0, cols=width)
                for source_row in table_data:
                    if not isinstance(source_row, list):
                        continue
                    cells = table.add_row().cells
                    for index, value in enumerate(source_row[:width]):
                        cells[index].text = _cell_text(value)
        document.save(str(destination))

    _atomic_document_write(path, write)
    return {"format": "docx"}


def _safe_sheet_name(value: Any, used: set[str]) -> str:
    source = _safe_xml_text(value or "Sheet")
    candidate = re.sub(r"[\[\]:*?/\\]", "_", source)[:31].strip() or "Sheet"
    base = candidate
    index = 2
    while candidate.casefold() in used:
        suffix = f"_{index}"
        candidate = f"{base[: 31 - len(suffix)]}{suffix}"
        index += 1
    used.add(candidate.casefold())
    return candidate


def _xlsx_sheets(title: str, content: Any) -> list[dict[str, Any]]:
    if isinstance(content, dict) and isinstance(content.get("sheets"), list):
        return [sheet for sheet in content["sheets"] if isinstance(sheet, dict)]
    if isinstance(content, dict) and isinstance(content.get("tables"), list):
        return [
            {"name": f"Table {index + 1}", "rows": table}
            for index, table in enumerate(content["tables"])
            if isinstance(table, list)
        ]
    rows = [[line] for line in _content_text(content).splitlines()]
    return [{"name": title or "Sheet", "rows": rows}]


def _create_xlsx(path: Path, title: str, content: Any) -> dict[str, Any]:
    openpyxl = _dependency("openpyxl", "openpyxl")
    sheets = _xlsx_sheets(title, content)
    if len(sheets) > MAX_SHEETS:
        raise DocumentError("document_too_complex", "Excel 工作表过多")

    def write(destination: Path) -> None:
        workbook = openpyxl.Workbook(write_only=True)
        default = workbook.active
        if default is not None:
            workbook.remove(default)
        used: set[str] = set()
        for sheet_data in sheets or [{"name": title or "Sheet", "rows": []}]:
            worksheet = workbook.create_sheet(_safe_sheet_name(sheet_data.get("name"), used))
            rows = sheet_data.get("rows", [])
            if not isinstance(rows, list):
                raise DocumentError("invalid_content", "Excel rows 必须是数组")
            if len(rows) > MAX_ROWS_PER_SHEET:
                raise DocumentError("document_too_complex", "Excel 行数过多")
            for row in rows:
                values = row if isinstance(row, list) else [row]
                if len(values) > MAX_COLUMNS_PER_SHEET:
                    raise DocumentError("document_too_complex", "Excel 列数过多")
                worksheet.append([_xlsx_safe_value(value) for value in values])
        workbook.save(str(destination))

    _atomic_document_write(path, write)
    return {"format": "xlsx", "sheetCount": max(1, len(sheets))}


def _pptx_slides(title: str, content: Any) -> list[dict[str, str]]:
    if isinstance(content, dict) and isinstance(content.get("slides"), list):
        slides = []
        for slide in content["slides"]:
            if isinstance(slide, dict):
                slides.append(
                    {
                        "title": _safe_xml_text(slide.get("title", "")),
                        "text": _safe_xml_text(slide.get("text", slide.get("body", ""))),
                    }
                )
        return slides
    text = _content_text(content)
    chunks = [chunk.strip() for chunk in text.split("\n\n") if chunk.strip()]
    if not chunks:
        chunks = [""]
    return [
        {
            "title": title if index == 0 else f"{title} {index + 1}".strip(),
            "text": _safe_xml_text(chunk),
        }
        for index, chunk in enumerate(chunks)
    ]


def _create_pptx(path: Path, title: str, content: Any) -> dict[str, Any]:
    pptx = _dependency("pptx", "python-pptx")
    slides = _pptx_slides(title, content)
    if len(slides) > MAX_SLIDES:
        raise DocumentError("document_too_complex", "PowerPoint 幻灯片过多")

    def write(destination: Path) -> None:
        presentation = pptx.Presentation()
        presentation.core_properties.title = title
        for slide_data in slides:
            slide = presentation.slides.add_slide(presentation.slide_layouts[1])
            if slide.shapes.title is not None:
                slide.shapes.title.text = slide_data["title"]
            placeholders = slide.placeholders
            if len(placeholders) > 1:
                placeholders[1].text = slide_data["text"]
        presentation.save(str(destination))

    _atomic_document_write(path, write)
    return {"format": "pptx", "slideCount": len(slides)}


CREATORS: dict[str, Callable[[Path, str, Any], dict[str, Any]]] = {
    "pdf": _create_pdf,
    "docx": _create_docx,
    "xlsx": _create_xlsx,
    "pptx": _create_pptx,
}


def _read(path: Path) -> dict[str, Any]:
    return READERS[_validate_extension(path)](path)


def _create(path: Path, title: str, content: Any) -> dict[str, Any]:
    _validate_content_size(content)
    document_format = _validate_extension(path)
    result = CREATORS[document_format](path, title, content)
    result.update(
        {
            "title": title,
            "sizeBytes": path.stat().st_size,
        }
    )
    return result


def _conversion_content(document: dict[str, Any], target_format: str) -> dict[str, Any]:
    if target_format == "xlsx":
        if isinstance(document.get("sheets"), list):
            return {"sheets": document["sheets"]}
        if isinstance(document.get("tables"), list) and document["tables"]:
            return {"tables": document["tables"]}
        return {"sheets": [{"name": "Content", "rows": [[line] for line in document["text"].splitlines()]}]}
    if target_format == "pptx" and isinstance(document.get("slides"), list):
        return {"slides": document["slides"]}
    if target_format == "docx":
        value: dict[str, Any] = {"paragraphs": document["text"].splitlines()}
        if isinstance(document.get("tables"), list):
            value["tables"] = document["tables"]
        return value
    return {"text": document["text"]}


def handle_request(value: Any) -> dict[str, Any]:
    request = _require_mapping(value, "request")
    if request.get("version") != PROTOCOL_VERSION:
        raise DocumentError("unsupported_version", "不支持的文档 worker 协议版本")
    operation = _require_string(request, "operation").lower()
    if operation not in {"read", "create", "convert"}:
        raise DocumentError("unsupported_operation", "只支持 read、create 和 convert")
    root = _resolved_root(request)
    session_id = _validate_session(request.get("session_id"))

    if operation == "read":
        source = _input_path(request, root, session_id)
        return {"ok": True, "result": _read(source)}

    destination = _output_path(request, root, session_id)
    target_format = _format_for_path(destination)
    requested_format = request.get("format")
    if requested_format is not None and str(requested_format).lower() != target_format:
        raise DocumentError("format_mismatch", "format 与输出文件扩展名不一致")
    title = _safe_xml_text(request.get("title", "")).strip()[:500]

    if operation == "create":
        content = request.get("content", "")
    else:
        source = _input_path(request, root, session_id)
        document = _read(source)
        content = _conversion_content(document, target_format)
        if not title:
            title = _safe_xml_text(document.get("title", "")).strip()[:500]
    return {"ok": True, "result": _create(destination, title, content)}


def _error_response(error: Exception) -> dict[str, Any]:
    if isinstance(error, DocumentError):
        return {"ok": False, "error": {"code": error.code, "message": error.message}}
    return {
        "ok": False,
        "error": {
            "code": "internal_error",
            "message": f"文档处理失败：{type(error).__name__}",
        },
    }


def main() -> int:
    try:
        raw = sys.stdin.buffer.read(MAX_REQUEST_BYTES + 1)
        if len(raw) > MAX_REQUEST_BYTES:
            raise DocumentError("request_too_large", "请求体过大")
        if not raw.strip():
            raise DocumentError("invalid_request", "stdin 没有 JSON 请求")
        request = json.loads(raw)
        with redirect_stdout(sys.stderr):
            response = handle_request(request)
        exit_code = 0
    except json.JSONDecodeError:
        response = _error_response(DocumentError("invalid_json", "stdin 不是有效 JSON"))
        exit_code = 1
    except Exception as exc:
        response = _error_response(exc)
        exit_code = 1
    sys.stdout.write(json.dumps(response, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
