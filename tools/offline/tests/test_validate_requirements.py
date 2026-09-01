from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.offline.validate_requirements import validate_requirements


class RequirementsPolicyTests(unittest.TestCase):
    def validate(self, content: str) -> list[str]:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "requirements.txt"
            path.write_text(content, encoding="utf-8")
            return validate_requirements(path)

    def test_accepts_exact_pin_and_marker(self) -> None:
        self.assertEqual(self.validate('requests==2.34.2\ncolorama==0.4.6; sys_platform == "win32"\n'), [])

    def test_rejects_unpinned_dependency(self) -> None:
        self.assertTrue(self.validate("requests>=2\n"))

    def test_rejects_direct_url_even_with_exact_name(self) -> None:
        self.assertTrue(self.validate("package @ https://example.test/package.whl\n"))


if __name__ == "__main__":
    unittest.main()
