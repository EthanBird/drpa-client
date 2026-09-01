from __future__ import annotations

import hashlib
import tempfile
import unittest
from pathlib import Path

from tools.release.collect_desktop_assets import collect


class CollectDesktopAssetsTests(unittest.TestCase):
    def test_collects_linux_bundles_with_stable_names_and_checksums(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle_dir = root / "bundle"
            (bundle_dir / "appimage").mkdir(parents=True)
            (bundle_dir / "deb").mkdir(parents=True)
            (bundle_dir / "appimage" / "DRPA Next.AppImage").write_bytes(b"appimage")
            (bundle_dir / "deb" / "drpa-next.deb").write_bytes(b"debian")

            files = collect(
                platform="linux-x86_64",
                version="0.2.0",
                bundle_dir=bundle_dir,
                output=root / "out",
            )

            names = {path.name for path in files}
            self.assertEqual(
                names,
                {
                    "drpa-next-0.2.0-preview-linux-x86_64.AppImage",
                    "drpa-next-0.2.0-preview-linux-x86_64.AppImage.sha256",
                    "drpa-next-0.2.0-preview-linux-x86_64.deb",
                    "drpa-next-0.2.0-preview-linux-x86_64.deb.sha256",
                },
            )
            expected = hashlib.sha256(b"appimage").hexdigest()
            checksum = root / "out/drpa-next-0.2.0-preview-linux-x86_64.AppImage.sha256"
            self.assertEqual(
                checksum.read_text(encoding="utf-8"),
                f"{expected}  drpa-next-0.2.0-preview-linux-x86_64.AppImage\n",
            )

    def test_rejects_missing_expected_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle_dir = root / "bundle"
            bundle_dir.mkdir()

            with self.assertRaisesRegex(RuntimeError, "expected exactly one bundle"):
                collect(
                    platform="windows-x86_64",
                    version="0.2.0",
                    bundle_dir=bundle_dir,
                    output=root / "out",
                )


if __name__ == "__main__":
    unittest.main()
