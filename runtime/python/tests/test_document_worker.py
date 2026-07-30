from __future__ import annotations

import json
import os
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest

from drpa_runner import document_worker


def request(workspace: Path, session: str, operation: str, **values: object) -> dict[str, object]:
    return {
        "version": 1,
        "operation": operation,
        "workspace_root": str(workspace),
        "session_id": session,
        **values,
    }


def attachment_path(workspace: Path, session: str, name: str) -> Path:
    path = workspace / "agent" / "attachments" / session / "files" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def artifact_path(workspace: Path, session: str, name: str) -> Path:
    return workspace / "agent" / "artifacts" / session / "files" / name


def test_rejects_unknown_operations_without_executing_code(tmp_path: Path) -> None:
    with pytest.raises(document_worker.DocumentError, match="只支持"):
        document_worker.handle_request(
            request(tmp_path, "session-1", "python", code="open('owned', 'w').write('x')")
        )
    assert not (tmp_path / "owned").exists()


def test_read_is_limited_to_session_attachment_and_artifact_roots(tmp_path: Path) -> None:
    outside = tmp_path / "outside.docx"
    outside.write_bytes(b"not a document")

    with pytest.raises(document_worker.DocumentError) as captured:
        document_worker.handle_request(
            request(tmp_path, "session-1", "read", input_path=str(outside))
        )

    assert captured.value.code == "path_denied"


def test_output_must_be_current_session_artifact_and_never_overwrites(tmp_path: Path) -> None:
    outside = tmp_path / "report.xlsx"
    with pytest.raises(document_worker.DocumentError) as captured:
        document_worker.handle_request(
            request(
                tmp_path,
                "session-1",
                "create",
                output_path=str(outside),
                format="xlsx",
                content="value",
            )
        )
    assert captured.value.code == "path_denied"

    destination = artifact_path(tmp_path, "session-1", "report.xlsx")
    destination.parent.mkdir(parents=True)
    destination.write_bytes(b"keep me")
    with pytest.raises(document_worker.DocumentError) as captured:
        document_worker.handle_request(
            request(
                tmp_path,
                "session-1",
                "create",
                output_path=str(destination),
                format="xlsx",
                content="replacement",
            )
        )
    assert captured.value.code == "output_exists"
    assert destination.read_bytes() == b"keep me"


def test_macro_payload_is_rejected_even_with_docx_extension(tmp_path: Path) -> None:
    source = attachment_path(tmp_path, "session-1", "macro.docx")
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr("[Content_Types].xml", "<Types />")
        archive.writestr("word/vbaProject.bin", b"macro")

    with pytest.raises(document_worker.DocumentError) as captured:
        document_worker.handle_request(
            request(tmp_path, "session-1", "read", input_path=str(source))
        )

    assert captured.value.code == "macro_format_denied"


def test_ooxml_with_a_document_type_declaration_is_rejected(tmp_path: Path) -> None:
    source = attachment_path(tmp_path, "session-1", "unsafe.docx")
    with zipfile.ZipFile(source, "w") as archive:
        archive.writestr(
            "[Content_Types].xml",
            '<!DOCTYPE x [<!ENTITY e SYSTEM "file:///etc/passwd">]><Types>&e;</Types>',
        )

    with pytest.raises(document_worker.DocumentError) as captured:
        document_worker.handle_request(
            request(tmp_path, "session-1", "read", input_path=str(source))
        )

    assert captured.value.code == "invalid_ooxml"


def test_creates_reads_and_converts_xlsx_without_leaving_artifact_root(
    tmp_path: Path,
) -> None:
    pytest.importorskip("openpyxl")
    pytest.importorskip("docx")
    session = "session-1"
    source = artifact_path(tmp_path, session, "source.xlsx")
    created = document_worker.handle_request(
        request(
            tmp_path,
            session,
            "create",
            output_path=str(source),
            format="xlsx",
            title="Sales",
            content={
                "sheets": [
                    {
                        "name": "Data",
                        "rows": [["name", "value"], ["A", 3], ["unsafe", "=WEBSERVICE(A1)"]],
                    }
                ]
            },
        )
    )
    assert created["ok"] is True
    assert source.is_file()

    read = document_worker.handle_request(
        request(tmp_path, session, "read", input_path=str(source))
    )
    assert read["result"]["sheets"][0]["rows"][1] == ["A", "3"]
    assert read["result"]["sheets"][0]["rows"][2][1] == "'=WEBSERVICE(A1)"

    converted = artifact_path(tmp_path, session, "converted.docx")
    result = document_worker.handle_request(
        request(
            tmp_path,
            session,
            "convert",
            input_path=str(source),
            output_path=str(converted),
            format="docx",
            title="Converted",
        )
    )
    assert result["result"]["format"] == "docx"
    assert converted.is_file()
    assert converted.resolve().is_relative_to(
        (tmp_path / "agent" / "artifacts" / session).resolve()
    )


@pytest.mark.parametrize(
    ("document_format", "content", "expected_text"),
    [
        ("pdf", {"text": "PDF document body"}, "PDF document body"),
        ("docx", {"paragraphs": ["Word document body"]}, "Word document body"),
        (
            "pptx",
            {"slides": [{"title": "Slide title", "text": "PowerPoint document body"}]},
            "PowerPoint document body",
        ),
    ],
)
def test_creates_and_reads_pdf_word_and_powerpoint(
    tmp_path: Path,
    document_format: str,
    content: dict[str, object],
    expected_text: str,
) -> None:
    modules = {
        "pdf": ("pypdf", "reportlab"),
        "docx": ("docx",),
        "pptx": ("pptx",),
    }
    for module in modules[document_format]:
        pytest.importorskip(module)
    session = "session-1"
    destination = artifact_path(tmp_path, session, f"document.{document_format}")
    created = document_worker.handle_request(
        request(
            tmp_path,
            session,
            "create",
            output_path=str(destination),
            format=document_format,
            title="Document title",
            content=content,
        )
    )
    assert created["result"]["format"] == document_format
    assert destination.is_file()

    read = document_worker.handle_request(
        request(tmp_path, session, "read", input_path=str(destination))
    )
    assert expected_text in read["result"]["text"]


def test_cli_emits_one_json_response_and_no_traceback(tmp_path: Path) -> None:
    source_root = Path(document_worker.__file__).resolve().parents[1]
    environment = os.environ.copy()
    environment["PYTHONPATH"] = os.pathsep.join(
        value for value in [str(source_root), environment.get("PYTHONPATH", "")] if value
    )
    process = subprocess.run(
        [sys.executable, "-m", "drpa_runner.document_worker"],
        input=json.dumps(request(tmp_path, "session-1", "eval", code="1 + 1")),
        text=True,
        capture_output=True,
        check=False,
        env=environment,
    )

    assert process.returncode == 1
    assert process.stderr == ""
    lines = process.stdout.splitlines()
    assert len(lines) == 1
    response = json.loads(lines[0])
    assert response["ok"] is False
    assert response["error"]["code"] == "unsupported_operation"
