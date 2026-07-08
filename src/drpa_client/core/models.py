from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal


ParamType = Literal["string", "password", "boolean", "integer", "number", "date", "file", "directory"]


@dataclass(frozen=True)
class PackageParam:
    name: str
    label: str
    type: ParamType = "string"
    required: bool = False
    default: Any = None
    description: str = ""


@dataclass(frozen=True)
class RuntimeSpec:
    python: str = ">=3.11"
    isolation: Literal["venv", "shared"] = "venv"


@dataclass(frozen=True)
class DependencySpec:
    strategy: Literal["online", "offline-first", "offline-only"] = "offline-first"
    pip: tuple[str, ...] = ()
    requirements: str | None = "requirements.txt"
    local_common: tuple[str, ...] = ()
    local_windows: tuple[str, ...] = ()
    local_linux: tuple[str, ...] = ()


@dataclass(frozen=True)
class PackageManifest:
    id: str
    name: str
    version: str
    entry: str
    description: str = ""
    author: str = ""
    runtime: RuntimeSpec = field(default_factory=RuntimeSpec)
    dependencies: DependencySpec = field(default_factory=DependencySpec)
    params: tuple[PackageParam, ...] = ()


@dataclass(frozen=True)
class InstalledPackage:
    manifest: PackageManifest
    root_dir: Path
    package_dir: Path
    venv_dir: Path | None
    installed_at: str

    @property
    def display_name(self) -> str:
        return f"{self.manifest.name} {self.manifest.version}"


@dataclass(frozen=True)
class TaskEvent:
    type: str
    payload: dict[str, Any]


@dataclass(frozen=True)
class TaskRunRecord:
    id: str
    package_id: str
    package_name: str
    package_version: str
    status: str
    params: dict[str, Any]
    output_dir: Path
    log_file: Path
    started_at: str
    finished_at: str | None = None
    exit_code: int | None = None
