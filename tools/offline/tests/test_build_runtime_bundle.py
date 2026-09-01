from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools.offline.build_runtime_bundle import create_wheelhouse_lock


class WheelhouseLockTests(unittest.TestCase):
    def test_linux_lock_records_exact_wheels_and_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            wheelhouse = stage / "wheelhouse"
            wheelhouse.mkdir()
            (wheelhouse / "demo-1.2.3-py3-none-any.whl").write_bytes(b"pure")
            (wheelhouse / "native-4.5.6-cp311-cp311-manylinux_2_17_x86_64.whl").write_bytes(b"native")

            create_wheelhouse_lock(stage, "linux-x86_64", "3.11.9")

            lock = json.loads((stage / "wheelhouse-lock.json").read_text(encoding="utf-8"))
            self.assertEqual(lock["platform"], "linux-x86_64")
            self.assertEqual(lock["pythonTag"], "cp311")
            self.assertEqual(len(lock["wheels"]), 2)
            self.assertEqual(len(lock["wheels"][0]["sha256"]), 64)

    def test_linux_lock_rejects_windows_wheel(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            wheelhouse = stage / "wheelhouse"
            wheelhouse.mkdir()
            (wheelhouse / "native-1.0-cp311-cp311-win_amd64.whl").write_bytes(b"wrong")

            with self.assertRaisesRegex(RuntimeError, "incompatible"):
                create_wheelhouse_lock(stage, "linux-x86_64", "3.11.9")


if __name__ == "__main__":
    unittest.main()
