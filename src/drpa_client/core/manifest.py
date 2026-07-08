from __future__ import annotations

from pathlib import Path
from typing import Any

from .models import DependencySpec, PackageManifest, PackageParam, RuntimeSpec


class ManifestError(ValueError):
    """Raised when a script package manifest is invalid."""


def load_manifest(package_dir: Path) -> PackageManifest:
    manifest_path = package_dir / "manifest.yaml"
    if not manifest_path.exists():
        raise ManifestError("脚本包缺少 manifest.yaml")

    try:
        import yaml
    except ImportError as exc:  # pragma: no cover - dependency is declared in pyproject
        raise ManifestError("当前运行环境缺少 PyYAML，无法解析 manifest.yaml") from exc

    with manifest_path.open("r", encoding="utf-8") as fp:
        raw = yaml.safe_load(fp) or {}

    if not isinstance(raw, dict):
        raise ManifestError("manifest.yaml 根节点必须是对象")

    return parse_manifest(raw)


def parse_manifest(raw: dict[str, Any]) -> PackageManifest:
    package_id = _required_str(raw, "id")
    name = _required_str(raw, "name")
    version = _required_str(raw, "version")
    entry = _required_str(raw, "entry")

    runtime_raw = raw.get("runtime") or {}
    if not isinstance(runtime_raw, dict):
        raise ManifestError("runtime 必须是对象")
    runtime = RuntimeSpec(
        python=str(runtime_raw.get("python", ">=3.11")),
        isolation=str(runtime_raw.get("isolation", "venv")),
    )

    deps = _parse_dependencies(raw.get("dependencies") or {})
    params = tuple(_parse_param(item) for item in raw.get("params") or ())

    return PackageManifest(
        id=package_id,
        name=name,
        version=version,
        entry=entry,
        description=str(raw.get("description", "")),
        author=str(raw.get("author", "")),
        runtime=runtime,
        dependencies=deps,
        params=params,
    )


def _parse_dependencies(raw: dict[str, Any]) -> DependencySpec:
    if not isinstance(raw, dict):
        raise ManifestError("dependencies 必须是对象")

    local = raw.get("local") or {}
    if not isinstance(local, dict):
        raise ManifestError("dependencies.local 必须是对象")

    return DependencySpec(
        strategy=str(raw.get("strategy", "offline-first")),
        pip=tuple(str(item) for item in raw.get("pip") or ()),
        requirements=raw.get("requirements", "requirements.txt"),
        local_common=tuple(str(item) for item in local.get("common") or ()),
        local_windows=tuple(str(item) for item in local.get("windows") or ()),
        local_linux=tuple(str(item) for item in local.get("linux") or ()),
    )


def _parse_param(raw: dict[str, Any]) -> PackageParam:
    if not isinstance(raw, dict):
        raise ManifestError("params 每一项必须是对象")
    return PackageParam(
        name=_required_str(raw, "name"),
        label=str(raw.get("label") or raw.get("name")),
        type=str(raw.get("type", "string")),
        required=bool(raw.get("required", False)),
        default=raw.get("default"),
        description=str(raw.get("description", "")),
    )


def _required_str(raw: dict[str, Any], key: str) -> str:
    value = raw.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ManifestError(f"manifest.yaml 缺少必填字符串字段：{key}")
    return value.strip()
