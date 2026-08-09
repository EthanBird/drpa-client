from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools.offline.rpa_for_python import (
    SPEC_PATH,
    load_rpa_spec,
    patch_tagui_source,
    source_build_names,
    write_binary_requirements,
)


class RpaForPythonOfflineTests(unittest.TestCase):
    def test_spec_pins_source_and_engine_hashes(self) -> None:
        spec = load_rpa_spec()

        self.assertEqual(source_build_names(spec), {"rpa", "tagui"})
        self.assertEqual(spec["version"], "1.50.0")
        for source in spec["sourceDistributions"]:
            self.assertRegex(source["sha256"], r"^[0-9a-f]{64}$")
            self.assertTrue(source["url"].startswith("https://files.pythonhosted.org/"))
        self.assertEqual(
            set(spec["engineAssets"]),
            {"windows-x86_64", "linux-x86_64", "macos-arm64", "macos-x86_64"},
        )
        self.assertEqual(json.loads(SPEC_PATH.read_text(encoding="utf-8"))["schema"], 1)

    def test_source_packages_are_removed_from_binary_only_download_input(self) -> None:
        spec = load_rpa_spec()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "runtime.txt"
            target = root / "binary.txt"
            source.write_text(
                "# locked\nrequests==2.34.2\nrpa==1.50.0\ntagui==1.50.0\n",
                encoding="utf-8",
            )

            write_binary_requirements(source, target, spec)

            self.assertEqual(target.read_text(encoding="utf-8"), "# locked\nrequests==2.34.2\n")

    def test_tagui_patch_uses_host_managed_offline_paths(self) -> None:
        source = """if platform.system() == 'Windows':
    _tagui_location = os.environ['APPDATA']
else:
    _tagui_location = os.path.expanduser('~')

def setup():
    if not os.path.isfile('rpa_python.zip'):
        pass
"""

        patched = patch_tagui_source(source)

        self.assertIn("DRPA_RPA_HOME", patched)
        self.assertIn("DRPA_RPA_BUNDLE", patched)
        self.assertIn("shutil.copyfile(drpa_offline_bundle, 'rpa_python.zip')", patched)
        self.assertEqual(patched.count("if not os.path.isfile('rpa_python.zip'):"), 1)


if __name__ == "__main__":
    unittest.main()
