from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.linux.verify_deb_bundle import verify_deb_bundle


@unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is required")
class DebianBundleTests(unittest.TestCase):
    def build_deb(self, root: Path, dependencies: list[str]) -> Path:
        package_root = root / "package"
        control_root = package_root / "DEBIAN"
        runtime_root = package_root / "usr/lib/drpa-next/runtime"
        control_root.mkdir(parents=True)
        runtime_root.mkdir(parents=True)
        (control_root / "control").write_text(
            "\n".join([
                "Package: drpa-next",
                "Version: 1.0.0",
                "Section: devel",
                "Priority: optional",
                "Architecture: amd64",
                f"Depends: {', '.join(dependencies)}",
                "Maintainer: DRPA",
                "Description: DRPA test package",
                "",
            ]),
            encoding="utf-8",
        )
        binary = package_root / "usr/bin/drpa-next"
        binary.parent.mkdir(parents=True)
        binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        binary.chmod(binary.stat().st_mode | 0o111)

        files = {
            "python/cpython/bin/python3.11": b"python",
            "browser/chrome-linux64/chrome": b"chrome",
            "tools/uv": b"uv",
            "bootstrap_runtime.py": b"bootstrap",
            "locks/runtime.txt": b"demo==1.0\n",
            "wheelhouse/demo-1.0-py3-none-any.whl": b"wheel",
        }
        for relative, content in files.items():
            path = runtime_root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        for relative in ("python/cpython/bin/python3.11", "browser/chrome-linux64/chrome", "tools/uv"):
            path = runtime_root / relative
            path.chmod(os.stat(path).st_mode | 0o111)
        wheel = runtime_root / "wheelhouse/demo-1.0-py3-none-any.whl"
        (runtime_root / "manifest.json").write_text(json.dumps({
            "platform": "linux-x86_64",
            "pythonVersion": "3.11.9",
            "pythonExecutable": "python/cpython/bin/python3.11",
            "browserExecutable": "browser/chrome-linux64/chrome",
        }), encoding="utf-8")
        (runtime_root / "wheelhouse-lock.json").write_text(json.dumps({
            "platform": "linux-x86_64",
            "pythonVersion": "3.11.9",
            "wheels": [{
                "filename": wheel.name,
                "bytes": wheel.stat().st_size,
                "sha256": hashlib.sha256(wheel.read_bytes()).hexdigest(),
            }],
        }), encoding="utf-8")
        deb = root / "drpa-next_1.0.0_amd64.deb"
        subprocess.run(
            ["dpkg-deb", "--build", str(package_root), str(deb)],
            check=True,
            capture_output=True,
        )
        return deb

    def test_accepts_runtime_complete_amd64_deb(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            deb = self.build_deb(root, [
                "libwebkit2gtk-4.1-0",
                "libgtk-3-0",
                "libgbm1",
                "libnss3",
                "xdg-utils",
            ])
            manifest = verify_deb_bundle(deb, "1.0.0", root / "extract")
            self.assertEqual(manifest["architecture"], "amd64")
            self.assertEqual(manifest["runtimeManifestPath"], "/usr/lib/drpa-next/runtime/manifest.json")
            self.assertEqual(manifest["deb"]["sha256"], hashlib.sha256(deb.read_bytes()).hexdigest())

    def test_rejects_missing_runtime_dependency(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            deb = self.build_deb(root, ["libwebkit2gtk-4.1-0", "libgtk-3-0"])
            with self.assertRaisesRegex(ValueError, "dependencies are incomplete"):
                verify_deb_bundle(deb, "1.0.0", root / "extract")


if __name__ == "__main__":
    unittest.main()
