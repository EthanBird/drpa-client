from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.linux.build_modular_uos import (
    DESKTOP_COMPONENT_ID,
    build_core_deb,
    apply_python_wheel_overlay,
    patch_desktop_component,
    patch_python_component,
    runtime_browser_entry,
)


class ModularUosBuildTests(unittest.TestCase):
    def test_python_wheel_overlay_replaces_same_distribution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            wheelhouse = root / "runtime/wheelhouse"
            wheelhouse.mkdir(parents=True)
            old = wheelhouse / "debugpy-1.8.21-cp311-cp311-manylinux_2_34_x86_64.whl"
            old.write_bytes(b"old")
            overlay = root / "overlay"
            overlay.mkdir()
            replacement = overlay / "debugpy-1.8.21-cp311-cp311-linux_x86_64.whl"
            replacement.write_bytes(b"uos20")

            installed = apply_python_wheel_overlay(root / "runtime", overlay)

            self.assertEqual(installed, [replacement.name])
            self.assertFalse(old.exists())
            self.assertEqual((wheelhouse / replacement.name).read_bytes(), b"uos20")
            provenance = json.loads(
                (root / "runtime/drpa-wheel-overlay.json").read_text(encoding="utf-8")
            )
            self.assertEqual(provenance["wheels"][0]["file"], replacement.name)

    @unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is required")
    def test_core_deb_contains_cli_stable_launcher_and_native_guide(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            icon_root = workspace / "apps/desktop/src-tauri/icons"
            icon_root.mkdir(parents=True)
            for name in ("32x32.png", "128x128.png", "128x128@2x.png"):
                (icon_root / name).write_bytes(b"png")
            binaries = root / "binaries"
            binaries.mkdir(parents=True)
            for name in ("drpa", "drpa-launcher", "drpa-component-installer"):
                path = binaries / name
                path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
                path.chmod(0o755)
            output = root / "drpa-next-core.deb"

            build_core_deb(
                workspace,
                binaries,
                output,
                "2.1.1-1+uos20.test1",
                root / "work",
            )

            extracted = root / "extracted"
            subprocess.run(["dpkg-deb", "-x", str(output), str(extracted)], check=True)
            for name in ("drpa", "drpa-next", "drpa-component-installer"):
                self.assertTrue(os.access(extracted / "usr/bin" / name, os.X_OK))
            launcher = (extracted / "usr/bin/drpa-next").read_text(encoding="utf-8")
            self.assertIn("DRPA_INSTALL_ROOT=/opt/drpa-next-uos20", launcher)
            self.assertIn("exec /usr/lib/drpa-next/drpa-launcher", launcher)
            desktop = (extracted / "usr/share/applications/drpa-next.desktop").read_text(
                encoding="utf-8"
            )
            self.assertIn("Exec=/usr/bin/drpa-next", desktop)
            self.assertIn("Icon=drpa-next", desktop)
            self.assertIn("StartupWMClass=drpa-desktop", desktop)
            for icon_name in ("drpa-next.png", "drpa-desktop.png"):
                self.assertTrue(
                    (
                        extracted
                        / "usr/share/icons/hicolor/128x128/apps"
                        / icon_name
                    ).is_file()
                )
            control = subprocess.run(
                ["dpkg-deb", "-f", str(output), "Package", "Depends"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout
            self.assertIn("drpa-next-core", control)
            self.assertIn("libc6 (>= 2.28)", control)
            self.assertIn("policykit-1", control)

    def test_browser_entry_is_relative_to_the_browser_component(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            chrome = runtime / "browser/chrome-linux64/chrome"
            chrome.parent.mkdir(parents=True)
            chrome.write_bytes(b"chrome")
            runtime.joinpath("manifest.json").write_text(
                json.dumps(
                    {
                        "platform": "linux-x86_64",
                        "pythonExecutable": "python/bin/python3",
                        "browserExecutable": "browser/chrome-linux64/chrome",
                    }
                ),
                encoding="utf-8",
            )

            self.assertEqual(runtime_browser_entry(runtime), "chrome-linux64/chrome")

    @unittest.skipUnless(
        shutil.which("patchelf") and Path("/bin/true").is_file(),
        "Linux patchelf is required",
    )
    def test_desktop_elfs_remain_pristine_for_legacy_compatibility_links(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "usr/bin/drpa-desktop"
            binary.parent.mkdir(parents=True)
            shutil.copy2("/bin/true", binary)
            runtime = root / "uos-runtime"
            runtime.mkdir()
            shutil.copy2("/lib64/ld-linux-x86-64.so.2", runtime / "ld-linux-x86-64.so.2")
            (root / "AppRun").write_text(
                '#!/bin/sh\nthis_dir="$(readlink -f "$(dirname "$0")")"\n'
                'source "$this_dir"/apprun-hooks/"linuxdeploy-plugin-gtk.sh"\n'
                "exec true\n",
                encoding="utf-8",
            )
            subprocess.run(
                [
                    "patchelf",
                    "--force-rpath",
                    "--set-rpath",
                    "/opt/drpa-next-uos20/uos-runtime:$ORIGIN",
                    str(binary),
                ],
                check=True,
            )

            provenance = patch_desktop_component(root, "2.1.1")

            expected = "/lib64/ld-linux-x86-64.so.2"
            interpreter = subprocess.run(
                ["patchelf", "--print-interpreter", str(binary)],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.assertEqual(interpreter, expected)
            rpath = subprocess.run(
                ["patchelf", "--print-rpath", str(binary)],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.assertEqual(
                rpath,
                "/opt/drpa-next-uos20/uos-runtime:$ORIGIN",
            )
            apprun = (root / "AppRun").read_text(encoding="utf-8")
            self.assertIn("LIBGL_DRIVERS_PATH", apprun)
            self.assertIn("DRPA_GTK_IM_MODULE", apprun)
            self.assertIn("DRPA_SYSTEM_GTK_IM_CACHE", apprun)
            self.assertIn("GTK_IM_MODULE=xim", apprun)
            self.assertEqual(provenance["legacyCompatibilityLinks"], ["usr", "uos-runtime"])
            self.assertEqual(provenance["interpreterElfCount"], 0)

    @unittest.skipUnless(
        shutil.which("patchelf") and Path("/bin/true").is_file(),
        "Linux patchelf is required",
    )
    def test_python_elfs_use_the_uos_system_loader(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "python/bin/python3"
            binary.parent.mkdir(parents=True)
            shutil.copy2("/bin/true", binary)

            provenance = patch_python_component(root, "2.1.1")

            expected = "/lib64/ld-linux-x86-64.so.2"
            interpreter = subprocess.run(
                ["patchelf", "--print-interpreter", str(binary)],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.assertEqual(interpreter, expected)
            self.assertEqual(provenance["systemInterpreter"], expected)
            self.assertEqual(provenance["interpreterElfCount"], 1)


if __name__ == "__main__":
    unittest.main()
