from __future__ import annotations

import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path

from tools.linux.verify_runtime_layout import verify_runtime_layout


class LinuxRuntimeLayoutTests(unittest.TestCase):
    def make_runtime(self, root: Path) -> None:
        files = {
            "python/cpython/bin/python3.11": b"python",
            "browser/chrome-linux64/chrome": b"chrome",
            "tools/uv": b"uv",
            "bootstrap_runtime.py": b"bootstrap",
            "locks/runtime.txt": b"demo==1.0\n",
            "locks/rpa-for-python.json": b"{}",
            "rpa/rpa_python.zip": b"rpa-engine",
            "wheelhouse/demo-1.0-py3-none-any.whl": b"wheel",
        }
        for relative, content in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        for relative in ("python/cpython/bin/python3.11", "browser/chrome-linux64/chrome", "tools/uv"):
            path = root / relative
            path.chmod(path.stat().st_mode | 0o100)
        wheel = root / "wheelhouse/demo-1.0-py3-none-any.whl"
        rpa_bundle = root / "rpa/rpa_python.zip"
        (root / "rpa/asset-lock.json").write_text(json.dumps({
            "platform": "linux-x86_64",
            "bundle": {
                "bytes": rpa_bundle.stat().st_size,
                "sha256": hashlib.sha256(rpa_bundle.read_bytes()).hexdigest(),
            },
        }), encoding="utf-8")
        (root / "manifest.json").write_text(json.dumps({
            "platform": "linux-x86_64",
            "pythonVersion": "3.11.9",
            "pythonExecutable": "python/cpython/bin/python3.11",
            "browserExecutable": "browser/chrome-linux64/chrome",
        }), encoding="utf-8")
        (root / "wheelhouse-lock.json").write_text(json.dumps({
            "platform": "linux-x86_64",
            "pythonVersion": "3.11.9",
            "wheels": [{
                "filename": wheel.name,
                "bytes": wheel.stat().st_size,
                "sha256": hashlib.sha256(wheel.read_bytes()).hexdigest(),
            }],
            "sourceBuilds": [{"name": "rpa"}, {"name": "tagui"}],
        }), encoding="utf-8")

    def test_accepts_complete_glibc_runtime(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_runtime(root)
            self.assertEqual(verify_runtime_layout(root), [])

    @unittest.skipIf(os.name == "nt", "Windows does not preserve POSIX executable mode bits")
    def test_rejects_lost_executable_mode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_runtime(root)
            python = root / "python/cpython/bin/python3.11"
            python.chmod(os.stat(python).st_mode & ~0o111)
            self.assertTrue(any("mode bits" in error for error in verify_runtime_layout(root)))


if __name__ == "__main__":
    unittest.main()
