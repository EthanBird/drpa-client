from __future__ import annotations

import hashlib
import io
import tarfile
from pathlib import Path

import pytest

from tools.windows import fetch_jcode


def make_archive(path: Path, member_name: str = fetch_jcode.ARCHIVE_MEMBER) -> str:
    payload = b"synthetic-jcode-executable"
    with tarfile.open(path, "w:gz") as bundle:
        info = tarfile.TarInfo(member_name)
        info.size = len(payload)
        bundle.addfile(info, io.BytesIO(payload))
    return hashlib.sha256(path.read_bytes()).hexdigest()


def test_install_extracts_only_the_pinned_executable(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    expected = make_archive(archive)

    executable = fetch_jcode.install(archive, tmp_path / "stage", expected)

    assert executable.name == "jcode.exe"
    assert executable.read_bytes() == b"synthetic-jcode-executable"
    assert (tmp_path / "stage" / "VERSION.txt").read_text(encoding="utf-8") == "jcode v0.79.1\n"
    assert "MIT License" in (tmp_path / "stage" / "LICENSE.txt").read_text(encoding="utf-8")


def test_install_rejects_checksum_mismatch(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    make_archive(archive)

    with pytest.raises(ValueError, match="checksum mismatch"):
        fetch_jcode.install(archive, tmp_path / "stage", "0" * 64)


def test_install_rejects_unexpected_archive_layout(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    expected = make_archive(archive, "nested/unexpected.exe")

    with pytest.raises(ValueError, match="exactly one"):
        fetch_jcode.install(archive, tmp_path / "stage", expected)


def test_install_rejects_unmanaged_output_files(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    expected = make_archive(archive)
    output = tmp_path / "stage"
    output.mkdir()
    (output / "stale-archive.tar.gz").write_bytes(b"stale")

    with pytest.raises(ValueError, match="unmanaged files"):
        fetch_jcode.install(archive, output, expected)


def test_verified_download_retries_a_corrupt_response(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    valid = tmp_path / "valid.tar.gz"
    expected = make_archive(valid)
    destination = tmp_path / "download.tar.gz"
    attempts = 0

    def fake_download(_url: str, output: Path) -> None:
        nonlocal attempts
        attempts += 1
        output.write_bytes(b"corrupt" if attempts == 1 else valid.read_bytes())

    monkeypatch.setattr(fetch_jcode, "ARCHIVE_SHA256", expected)
    monkeypatch.setattr(fetch_jcode, "download", fake_download)
    monkeypatch.setattr(fetch_jcode.time, "sleep", lambda _seconds: None)

    fetch_jcode.download_verified("https://example.invalid/jcode", destination)

    assert attempts == 2
    assert destination.read_bytes() == valid.read_bytes()
