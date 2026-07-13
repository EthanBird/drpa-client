from __future__ import annotations

from pathlib import Path


class PathPolicyError(ValueError):
    """Raised when a package-provided path escapes its allowed root."""


def resolve_child(root: Path, relative_path: str | Path, *, must_exist: bool = False) -> Path:
    """Resolve a package-controlled relative path while enforcing containment."""

    root = root.resolve()
    candidate_input = Path(relative_path)
    if candidate_input.is_absolute():
        raise PathPolicyError(f"absolute paths are not allowed: {relative_path}")

    candidate = (root / candidate_input).resolve()
    if candidate != root and root not in candidate.parents:
        raise PathPolicyError(f"path escapes allowed root: {relative_path}")
    if must_exist and not candidate.exists():
        raise PathPolicyError(f"path does not exist: {relative_path}")
    return candidate
