from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.linux.build_uos20_deb import (
    BASE_DEPENDENCIES,
    INSTALL_ROOT,
    PRIVATE_LOADER,
    copy_private_runtime,
    is_x86_64_elf,
    launcher_source,
)


class Uos20PackagePolicyTests(unittest.TestCase):
    def test_uos_dependencies_keep_webkit_and_cpp_runtime_private(self) -> None:
        dependencies = ", ".join(BASE_DEPENDENCIES)
        self.assertIn("libc6 (>= 2.28)", dependencies)
        self.assertNotIn("libwebkit2gtk", dependencies)
        self.assertNotIn("libgcc-s1", dependencies)
        self.assertNotIn("libstdc++6", dependencies)
        self.assertNotIn("libgbm1", dependencies)
        self.assertNotIn("libdrm2", dependencies)
        self.assertNotIn("libegl1", dependencies)
        self.assertNotIn("libgl1", dependencies)

    def test_launcher_does_not_poison_system_child_processes(self) -> None:
        source = launcher_source()
        self.assertNotIn("LD_LIBRARY_PATH", source)
        self.assertIn(f"/{INSTALL_ROOT.as_posix()}/usr/bin/drpa-desktop", source)
        self.assertIn(f"/{PRIVATE_LOADER.as_posix()}", source)
        self.assertIn("GDK_BACKEND=x11", source)
        self.assertIn('cd "$APPDIR/usr"', source)
        self.assertNotIn("WEBKIT_EXEC_PATH", source)
        self.assertIn("WEBKIT_DISABLE_DMABUF_RENDERER=1", source)
        self.assertIn("WEBKIT_DISABLE_COMPOSITING_MODE=1", source)
        self.assertIn("LIBGL_ALWAYS_SOFTWARE=1", source)
        self.assertIn("DRPA_UI_REDUCED_EFFECTS=1", source)
        self.assertIn('LIBGL_DRIVERS_PATH="$APPDIR/uos-runtime/dri"', source)
        self.assertIn("__EGL_VENDOR_LIBRARY_FILENAMES=", source)
        self.assertIn('GTK_PATH="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0"', source)
        self.assertNotIn('gtk-3.0:/usr/lib', source)
        self.assertIn("unset GTK_MODULES GTK3_MODULES", source)
        self.assertIn("NO_AT_BRIDGE=1", source)
        self.assertIn("GTK_IM_MODULE=gtk-im-context-simple", source)
        self.assertIn("GDK_CORE_DEVICE_EVENTS=1", source)

    def test_container_smoke_runs_the_builtin_dify2api_service(self) -> None:
        source = Path(__file__).resolve().parents[1].joinpath("uos20_container_smoke.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("plugins/dify2api/service/dify2api-server", source)
        self.assertIn("http://127.0.0.1:39423/healthz", source)
        self.assertIn("drpa-uos20-dify2api-health.json", source)
        self.assertIn('"$runtime_python" -I -c', source)
        self.assertIn("drpa-uos20-dify2api-health-errors.log", source)

    def test_container_smoke_injects_and_rejects_system_gtk_modules(self) -> None:
        source = Path(__file__).resolve().parents[1].joinpath("uos20_container_smoke.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("GTK_IM_MODULE=fcitx", source)
        self.assertIn("GTK_MODULES=gail:atk-bridge", source)
        self.assertIn("GTK3_MODULES=atk-bridge", source)
        self.assertIn("drpa-uos20-host-environment.txt", source)
        self.assertIn('runuser -u drpa-smoke -- cat "/proc/$host_pid/environ"', source)
        self.assertIn("GTK_IM_MODULE=gtk-im-context-simple", source)
        self.assertIn("GDK_CORE_DEVICE_EVENTS=1", source)
        self.assertIn("NO_AT_BRIDGE=1", source)

    def test_x86_64_elf_detection_rejects_other_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            elf = root / "elf"
            header = bytearray(20)
            header[:4] = b"\x7fELF"
            header[4] = 2
            header[18:20] = (62).to_bytes(2, "little")
            elf.write_bytes(header)
            self.assertTrue(is_x86_64_elf(elf))
            elf.write_text("shell", encoding="utf-8")
            self.assertFalse(is_x86_64_elf(elf))

    def test_private_runtime_copy_requires_nss_and_modern_libstdcxx(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / "lib/x86_64-linux-gnu"
            cxx = root / "usr/lib/x86_64-linux-gnu"
            runtime.mkdir(parents=True)
            cxx.mkdir(parents=True)
            nss = cxx / "nss"
            nss.mkdir()
            dri = cxx / "dri"
            dri.mkdir()
            egl_vendor = root / "usr/share/glvnd/egl_vendor.d"
            egl_vendor.mkdir(parents=True)
            runtime_filenames = (
                "ld-linux-x86-64.so.2",
                "libc.so.6",
                "libm.so.6",
                "libdl.so.2",
                "libpthread.so.0",
                "librt.so.1",
                "libresolv.so.2",
                "libanl.so.1",
                "libutil.so.1",
                "libthread_db.so.1",
                "libgcc_s.so.1",
                "libnss_dns.so.2",
                "libnss_files.so.2",
            )
            nss_filenames = (
                "libfreebl3.so",
                "libfreebl3.chk",
                "libfreeblpriv3.so",
                "libfreeblpriv3.chk",
                "libnssckbi.so",
                "libnssdbm3.so",
                "libnssdbm3.chk",
                "libsoftokn3.so",
                "libsoftokn3.chk",
            )
            for filename in runtime_filenames:
                (runtime / filename).write_bytes(filename.encode("utf-8"))
            for filename in nss_filenames:
                destination = (
                    nss / filename if filename == "libnssckbi.so" else cxx / filename
                )
                destination.write_bytes(filename.encode("utf-8"))
            for filename in (
                "libEGL.so.1",
                "libGL.so.1",
                "libGLX.so.0",
                "libGLdispatch.so.0",
                "libOpenGL.so.0",
                "libEGL_mesa.so.0",
                "libGLX_mesa.so.0",
                "libglapi.so.0",
            ):
                (cxx / filename).write_bytes(filename.encode("utf-8"))
            (dri / "swrast_dri.so").write_bytes(b"swrast")
            (dri / "kms_swrast_dri.so").write_bytes(b"kms_swrast")
            (egl_vendor / "50_mesa.json").write_text(
                '{"ICD":{"library_path":"libEGL_mesa.so.0"}}', encoding="utf-8"
            )
            (cxx / "libstdc++.so.6").write_bytes(b"GLIBCXX_3.4.30")

            inventory = copy_private_runtime(root, root / "output")

            copied = {entry["path"] for entry in inventory}
            self.assertIn("libnss_dns.so.2", copied)
            self.assertIn("libnss_files.so.2", copied)
            self.assertIn("libsoftokn3.so", copied)
            self.assertIn("libfreeblpriv3.so", copied)
            self.assertIn("libnssckbi.so", copied)
            self.assertIn("libstdc++.so.6", copied)
            self.assertIn("libEGL_mesa.so.0", copied)
            self.assertIn("dri/swrast_dri.so", copied)
            self.assertIn("dri/kms_swrast_dri.so", copied)
            self.assertIn("egl_vendor.d/50_mesa.json", copied)

    def test_graphics_userspace_is_not_a_system_dependency(self) -> None:
        dependencies = ", ".join(BASE_DEPENDENCIES)
        self.assertNotIn("libx11-6", dependencies)
        self.assertNotIn("libasound2", dependencies)
        self.assertNotIn("libfontconfig1", dependencies)
        self.assertNotIn("libegl1", dependencies)
        self.assertNotIn("libgl1", dependencies)
        self.assertNotIn("libgbm1", dependencies)
        self.assertNotIn("libdrm2", dependencies)


if __name__ == "__main__":
    unittest.main()
