"""DRPA Next Python runtime adapter."""

import os
import site
import sys


def _activate_profile_packages() -> None:
    """Add the host-managed per-Profile package layer, including its .pth files."""
    raw = os.environ.get("DRPA_PYTHON_PACKAGE_PATH", "").strip()
    if not raw:
        return
    site.addsitedir(raw)
    if raw in sys.path:
        sys.path.remove(raw)
    sys.path.insert(0, raw)


_activate_profile_packages()

from .executor import ExecutionRequest, execute_request  # noqa: E402
from .context import RuntimeContext  # noqa: E402
from .sql import SqlClient  # noqa: E402

__all__ = ["ExecutionRequest", "RuntimeContext", "SqlClient", "execute_request"]
