from __future__ import annotations

import hashlib
import io
import os
import tarfile
from pathlib import Path

import pytest

from tools.linux import fetch_jcode


def make_archive(path: Path, member_name: str = fetch_jcode.ARCHIVE_MEMBER) -> str:
    wrapper = b'#!/bin/sh\nexec "$self_dir/jcode-linux-x86_64.bin" "$@"\n'
    payload = b"synthetic-linux-jcode-binary"
    with tarfile.open(path, "w:gz") as bundle:
        info = tarfile.TarInfo(member_name)
        info.size = len(wrapper)
        bundle.addfile(info, io.BytesIO(wrapper))
        binary = tarfile.TarInfo(fetch_jcode.ARCHIVE_BINARY_MEMBER)
        binary.size = len(payload)
        bundle.addfile(binary, io.BytesIO(payload))
    return hashlib.sha256(path.read_bytes()).hexdigest()


def test_install_stages_executable_linux_sidecar(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    expected = make_archive(archive)

    executable = fetch_jcode.install(archive, tmp_path / "stage", expected)

    assert executable.name == "jcode"
    assert b'jcode.bin" "$@"' in executable.read_bytes()
    assert (tmp_path / "stage" / "jcode.bin").read_bytes() == b"synthetic-linux-jcode-binary"
    if os.name != "nt":
        assert executable.stat().st_mode & 0o111
        assert (tmp_path / "stage" / "jcode.bin").stat().st_mode & 0o111
    assert (tmp_path / "stage" / "VERSION.txt").read_text(encoding="utf-8") == "jcode v0.74.0\n"
    assert "MIT License" in (tmp_path / "stage" / "LICENSE.txt").read_text(encoding="utf-8")


def test_install_rejects_checksum_mismatch(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    make_archive(archive)
    with pytest.raises(ValueError, match="checksum mismatch"):
        fetch_jcode.install(archive, tmp_path / "stage", "0" * 64)


def test_install_rejects_unexpected_archive_layout(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    expected = make_archive(archive, "nested/jcode")
    with pytest.raises(ValueError, match="exactly one"):
        fetch_jcode.install(archive, tmp_path / "stage", expected)


def test_install_rejects_unmanaged_output_files(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    expected = make_archive(archive)
    output = tmp_path / "stage"
    output.mkdir()
    (output / "stale").write_bytes(b"stale")
    with pytest.raises(ValueError, match="unmanaged files"):
        fetch_jcode.install(archive, output, expected)


def test_executable_mode_is_not_inherited_from_archive(tmp_path: Path) -> None:
    archive = tmp_path / "jcode.tar.gz"
    expected = make_archive(archive)
    executable = fetch_jcode.install(archive, tmp_path / "stage", expected)
    assert os.access(executable, os.X_OK)
    assert os.access(executable.with_name("jcode.bin"), os.X_OK)
