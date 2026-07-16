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
        self.assertIn("libegl1", dependencies)
        self.assertIn("libgbm1", dependencies)

    def test_launcher_does_not_poison_system_child_processes(self) -> None:
        source = launcher_source()
        self.assertNotIn("LD_LIBRARY_PATH", source)
        self.assertIn(f"/{INSTALL_ROOT.as_posix()}/usr/bin/drpa-desktop", source)
        self.assertIn(f"/{PRIVATE_LOADER.as_posix()}", source)
        self.assertIn("GDK_BACKEND=x11", source)

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
            filenames = (
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
            for filename in filenames:
                (runtime / filename).write_bytes(filename.encode("utf-8"))
            (cxx / "libstdc++.so.6").write_bytes(b"GLIBCXX_3.4.30")

            inventory = copy_private_runtime(root, root / "output")

            copied = {entry["path"] for entry in inventory}
            self.assertIn("libnss_dns.so.2", copied)
            self.assertIn("libnss_files.so.2", copied)
            self.assertIn("libstdc++.so.6", copied)


if __name__ == "__main__":
    unittest.main()
