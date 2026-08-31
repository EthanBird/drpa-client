from __future__ import annotations

import importlib
import sys

import drpa_runner


def test_profile_package_overlay_is_added_ahead_of_runtime_paths(tmp_path, monkeypatch) -> None:
    module_name = "drpa_profile_overlay_smoke"
    (tmp_path / f"{module_name}.py").write_text("VALUE = 314\n", encoding="utf-8")
    monkeypatch.setenv("DRPA_PYTHON_PACKAGE_PATH", str(tmp_path))

    drpa_runner._activate_profile_packages()
    try:
        module = importlib.import_module(module_name)
        assert module.VALUE == 314
        assert sys.path[0] == str(tmp_path)
    finally:
        sys.modules.pop(module_name, None)
        while str(tmp_path) in sys.path:
            sys.path.remove(str(tmp_path))
