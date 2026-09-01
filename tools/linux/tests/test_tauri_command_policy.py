from __future__ import annotations

import re
import unittest
from pathlib import Path


SOURCE_ROOT = Path(__file__).resolve().parents[3] / "apps/desktop/src-tauri/src"
COMMAND_PATTERN = re.compile(
    r"#\[tauri::command(?P<options>[^\]]*)\]\s*"
    r"(?:pub(?:\([^)]*\))?\s+)?(?P<async>async\s+)?fn\s+(?P<name>[a-zA-Z0-9_]+)",
    re.MULTILINE,
)

UI_IO_MODULES = {
    "agent_config.rs",
    "agent_documents.rs",
    "agent_sessions.rs",
    "automations.rs",
    "credential_vault.rs",
    "dashboard.rs",
    "database.rs",
    "knowledge.rs",
    "knowledge_base.rs",
    "local_dify.rs",
    "plugins.rs",
    "system_metrics.rs",
    "workspaces.rs",
}

LIB_IO_COMMANDS = {
    "install_package",
    "uninstall_package",
    "get_run_detail",
    "open_run_output_directory",
    "list_studio_projects",
    "create_studio_project",
    "rename_studio_project",
    "read_project_file",
    "write_project_file",
    "create_project_directory",
    "rename_project_entry",
    "delete_project_entry",
    "delete_studio_project",
    "import_project_file",
    "build_studio_project",
    "install_studio_project",
    "open_installed_package",
    "list_agent_extensions",
    "install_agent_extension",
    "set_agent_extension_enabled",
    "remove_agent_extension",
    "restart_studio_kernel",
    "open_workspace_data_directory",
    "export_user_data",
    "import_user_data",
    "open_build_output_directory",
    "apply_windows_update",
    "get_windows_update_status",
    "get_latest_windows_update_status",
    "restart_for_windows_update",
    "get_runtime_status",
    "initialize_runtime",
    "repair_runtime",
}


def command_inventory(path: Path) -> dict[str, tuple[bool, bool]]:
    source = path.read_text(encoding="utf-8")
    return {
        match.group("name"): (
            match.group("async") is not None,
            "async" in match.group("options"),
        )
        for match in COMMAND_PATTERN.finditer(source)
    }


class TauriCommandPolicyTests(unittest.TestCase):
    def test_business_io_commands_never_run_on_the_gtk_main_thread(self) -> None:
        violations: list[str] = []
        for filename in sorted(UI_IO_MODULES):
            commands = command_inventory(SOURCE_ROOT / filename)
            self.assertTrue(commands, filename)
            for name, (is_async_function, has_async_attribute) in commands.items():
                if not is_async_function and not has_async_attribute:
                    violations.append(f"{filename}:{name}")
        self.assertEqual([], violations)

    def test_desktop_host_file_and_runtime_commands_are_offloaded(self) -> None:
        commands = command_inventory(SOURCE_ROOT / "lib.rs")
        missing = sorted(LIB_IO_COMMANDS.difference(commands))
        blocking = sorted(
            name
            for name in LIB_IO_COMMANDS.intersection(commands)
            if not any(commands[name])
        )
        self.assertEqual([], missing)
        self.assertEqual([], blocking)


if __name__ == "__main__":
    unittest.main()
