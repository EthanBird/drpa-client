from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.linux.build_bundled_deb import REQUIRED_APPDIR_PATHS, build_bundled_deb
from tools.linux.verify_deb_bundle import verify_deb_bundle


@unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is required")
class DebianBundleTests(unittest.TestCase):
    def create_appdir(self, root: Path) -> Path:
        appdir = root / "appdir"
        for relative in REQUIRED_APPDIR_PATHS:
            path = appdir / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(relative.encode("utf-8"))
        for relative in (
            "AppRun",
            "AppRun.wrapped",
            "usr/bin/drpa-desktop",
            "usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess",
            "usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess",
        ):
            path = appdir / relative
            path.chmod(os.stat(path).st_mode | 0o111)

        desktop = appdir / "usr/share/applications/DRPA Next.desktop"
        desktop.parent.mkdir(parents=True, exist_ok=True)
        desktop.write_text(
            "[Desktop Entry]\nType=Application\nName=DRPA Next\n"
            "Exec=drpa-desktop\nIcon=drpa-desktop\n",
            encoding="utf-8",
        )
        icon = appdir / "usr/share/icons/hicolor/128x128/apps/drpa-desktop.png"
        icon.parent.mkdir(parents=True, exist_ok=True)
        icon.write_bytes(b"png")

        runtime_root = appdir / "usr/lib/DRPA Next/runtime"
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
            path = runtime_root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        for relative in (
            "python/cpython/bin/python3.11",
            "browser/chrome-linux64/chrome",
            "tools/uv",
        ):
            path = runtime_root / relative
            path.chmod(os.stat(path).st_mode | 0o111)
        wheel = runtime_root / "wheelhouse/demo-1.0-py3-none-any.whl"
        rpa_bundle = runtime_root / "rpa/rpa_python.zip"
        (runtime_root / "rpa/asset-lock.json").write_text(
            json.dumps(
                {
                    "platform": "linux-x86_64",
                    "bundle": {
                        "bytes": rpa_bundle.stat().st_size,
                        "sha256": hashlib.sha256(rpa_bundle.read_bytes()).hexdigest(),
                    },
                }
            ),
            encoding="utf-8",
        )
        (runtime_root / "manifest.json").write_text(
            json.dumps(
                {
                    "platform": "linux-x86_64",
                    "pythonVersion": "3.11.9",
                    "pythonExecutable": "python/cpython/bin/python3.11",
                    "browserExecutable": "browser/chrome-linux64/chrome",
                }
            ),
            encoding="utf-8",
        )
        (runtime_root / "wheelhouse-lock.json").write_text(
            json.dumps(
                {
                    "platform": "linux-x86_64",
                    "pythonVersion": "3.11.9",
                    "wheels": [
                        {
                            "filename": wheel.name,
                            "bytes": wheel.stat().st_size,
                            "sha256": hashlib.sha256(wheel.read_bytes()).hexdigest(),
                        }
                    ],
                    "sourceBuilds": [{"name": "rpa"}, {"name": "tagui"}],
                }
            ),
            encoding="utf-8",
        )
        return appdir

    def build_deb(self, root: Path) -> Path:
        return build_bundled_deb(
            self.create_appdir(root),
            root / "drpa-next_1.0.0-2_amd64.deb",
            "1.0.0-2",
            root / "deb-work",
        )

    def test_builds_and_accepts_self_contained_amd64_deb(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            deb = self.build_deb(root)
            manifest = verify_deb_bundle(deb, "1.0.0-2", root / "extract")
            self.assertEqual(manifest["package"], "drpa-next")
            self.assertEqual(manifest["architecture"], "amd64")
            self.assertEqual(
                manifest["runtimeManifestPath"],
                "/opt/drpa-next/usr/lib/DRPA Next/runtime/manifest.json",
            )
            self.assertNotIn("libwebkit2gtk-4.1-0", manifest["depends"])
            self.assertIn(
                "/opt/drpa-next/usr/lib/libwebkit2gtk-4.1.so.0",
                manifest["bundledDesktopRuntimePaths"],
            )
            self.assertEqual(
                manifest["deb"]["sha256"], hashlib.sha256(deb.read_bytes()).hexdigest()
            )

    def test_rejects_system_webkit_dependency(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            deb = self.build_deb(root)
            unpacked = root / "unpacked"
            subprocess.run(["dpkg-deb", "-R", str(deb), str(unpacked)], check=True)
            control = unpacked / "DEBIAN/control"
            control.write_text(
                control.read_text(encoding="utf-8").replace(
                    "Depends: ", "Depends: libwebkit2gtk-4.1-0, ", 1
                ),
                encoding="utf-8",
            )
            legacy_deb = root / "legacy.deb"
            subprocess.run(
                ["dpkg-deb", "--build", str(unpacked), str(legacy_deb)],
                check=True,
                capture_output=True,
            )
            with self.assertRaisesRegex(ValueError, "must not depend on system desktop libraries"):
                verify_deb_bundle(legacy_deb, "1.0.0-2", root / "legacy-extract")

    def test_rejects_incomplete_appdir(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            appdir = self.create_appdir(root)
            (appdir / "usr/lib/libwebkit2gtk-4.1.so.0").unlink()
            with self.assertRaisesRegex(ValueError, "desktop runtime is incomplete"):
                build_bundled_deb(
                    appdir,
                    root / "broken.deb",
                    "1.0.0-2",
                    root / "broken-work",
                )


if __name__ == "__main__":
    unittest.main()
