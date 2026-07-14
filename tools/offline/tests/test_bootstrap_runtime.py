from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from offline.bootstrap import bootstrap_runtime


class RuntimeBootstrapTests(unittest.TestCase):
    def test_existing_environment_refreshes_only_the_runtime_adapter(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "runtime"
            environment = Path(directory) / "data" / "environment"
            (root / "locks").mkdir(parents=True)
            (root / "wheelhouse").mkdir()
            (root / "tools").mkdir()
            (root / "manifest.json").write_text(
                json.dumps({"bundleVersion": "test", "pythonVersion": "3.11.9"}),
                encoding="utf-8",
            )
            requirements = root / "locks" / "runtime.txt"
            requirements.write_text("requests==2.34.2\n", encoding="utf-8")
            (root / "wheelhouse" / "drpa_runtime_python-0.3.0-py3-none-any.whl").write_bytes(b"new-adapter")
            uv_name = "uv.exe" if os.name == "nt" else "uv"
            (root / "tools" / uv_name).write_bytes(b"uv")
            python = environment / "Scripts" / "python.exe"
            python.parent.mkdir(parents=True)
            python.write_bytes(b"python")
            sentinel = environment / "keep-me.txt"
            sentinel.write_text("existing dependency environment", encoding="utf-8")
            (environment / ".drpa-runtime.json").write_text(
                json.dumps({"requirementsSha256": bootstrap_runtime.digest_file(requirements)}),
                encoding="utf-8",
            )

            with (
                patch.object(bootstrap_runtime, "environment_python_version", return_value="3.11.9"),
                patch.object(bootstrap_runtime, "install_runtime_adapter") as install_adapter,
                patch.object(bootstrap_runtime, "verify_environment"),
            ):
                result = bootstrap_runtime.prepare(root=root, environment_override=environment)

            self.assertEqual(result, python)
            self.assertTrue(sentinel.is_file())
            install_adapter.assert_called_once()
            marker = json.loads((environment / ".drpa-runtime.json").read_text(encoding="utf-8"))
            self.assertEqual(marker["schema"], 2)
            self.assertEqual(marker["runtimeAdapter"]["file"], "drpa_runtime_python-0.3.0-py3-none-any.whl")


if __name__ == "__main__":
    unittest.main()
