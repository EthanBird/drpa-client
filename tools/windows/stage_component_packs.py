from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Callable, Iterable


SCHEMA = 1
MAX_FILES = 50_000
MAX_BYTES = 8 * 1024 * 1024 * 1024


@dataclass(frozen=True)
class PackDefinition:
    component_id: str
    display_name: str
    source_root: Path
    provides: tuple[str, ...]
    entrypoints: dict[str, str]
    include: Callable[[PurePosixPath], bool] = lambda _path: True


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def safe_relative(path: str) -> str:
    normalized = path.replace("\\", "/")
    candidate = PurePosixPath(normalized)
    if (
        not normalized
        or candidate.is_absolute()
        or ":" in normalized
        or any(part in {"", ".", ".."} for part in candidate.parts)
    ):
        raise ValueError(f"unsafe component path: {path}")
    return candidate.as_posix()


def collect_files(definition: PackDefinition) -> list[tuple[Path, str]]:
    if not definition.source_root.is_dir():
        raise ValueError(f"component source directory is missing: {definition.source_root}")
    files: list[tuple[Path, str]] = []
    for path in sorted(definition.source_root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"component source must not contain symlinks: {path}")
        if not path.is_file():
            continue
        relative = PurePosixPath(path.relative_to(definition.source_root).as_posix())
        if definition.include(relative):
            files.append((path, safe_relative(relative.as_posix())))
    if len(files) > MAX_FILES:
        raise ValueError(f"component contains too many files: {len(files)}")
    return files


def build_pack(definition: PackDefinition, version: str, platform: str, output: Path) -> dict[str, object]:
    files = collect_files(definition)
    inventory: list[dict[str, object]] = []
    total = 0
    for path, relative in files:
        size = path.stat().st_size
        total += size
        if total > MAX_BYTES:
            raise ValueError(f"component exceeds {MAX_BYTES} bytes")
        inventory.append(
            {
                "path": relative,
                "bytes": size,
                "sha256": sha256(path),
                "executable": bool(path.stat().st_mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)),
            }
        )
    inventory_paths = {item["path"] for item in inventory}
    for name, relative in definition.entrypoints.items():
        safe_relative(relative)
        if relative not in inventory_paths:
            raise ValueError(f"entrypoint {name} is missing from component: {relative}")
    manifest: dict[str, object] = {
        "schema": SCHEMA,
        "id": definition.component_id,
        "version": version,
        "platform": platform,
        "displayName": definition.display_name,
        "provides": list(definition.provides),
        "requires": {},
        "entrypoints": definition.entrypoints,
        "files": inventory,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".tmp")
    with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9, allowZip64=True) as archive:
        for (path, relative), item in zip(files, inventory, strict=True):
            archive.write(path, f"payload/{relative}")
        archive.writestr("component.json", json.dumps(manifest, ensure_ascii=False, indent=2).encode("utf-8"))
    os.replace(temporary, output)
    return manifest


def runtime_manifest(stage: Path) -> dict[str, object]:
    path = stage / "runtime" / "manifest.json"
    if not path.is_file():
        raise ValueError(f"sealed runtime manifest is missing: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def component_definitions(stage: Path) -> list[PackDefinition]:
    manifest = runtime_manifest(stage)
    browser_entry = safe_relative(str(manifest["browserExecutable"]))
    if not browser_entry.startswith("browser/"):
        raise ValueError(f"runtime browserExecutable must be under browser/: {browser_entry}")
    browser_entry = browser_entry.removeprefix("browser/")
    definitions = [
        PackDefinition(
            component_id="org.drpa.python-runtime",
            display_name="DRPA Python 运行环境",
            source_root=stage / "runtime",
            provides=("runtime.python", "runtime.rpaz"),
            entrypoints={"bootstrap": "bootstrap_runtime.py"},
            include=lambda path: not (path.parts and path.parts[0] == "browser"),
        ),
        PackDefinition(
            component_id="org.drpa.browser.chromium",
            display_name="Chromium 浏览器自动化组件",
            source_root=stage / "runtime" / "browser",
            provides=("browser.chromium", "browser.agent"),
            entrypoints={"browser": browser_entry},
        ),
        PackDefinition(
            component_id="org.drpa.webview2-fixed",
            display_name="WebView2 桌面界面组件",
            source_root=stage / "webview2",
            provides=("desktop.webview2",),
            entrypoints={"webview2": "msedgewebview2.exe"},
        ),
    ]
    desktop = stage / "DRPA Next.exe"
    if desktop.is_file():
        definitions.insert(
            0,
            PackDefinition(
                component_id="org.drpa.desktop-ui",
                display_name="DRPA Next 桌面界面",
                source_root=stage,
                provides=("desktop.ui",),
                entrypoints={"desktop": "DRPA Next.exe"},
                include=lambda path: path.as_posix() == "DRPA Next.exe",
            ),
        )
    jcode = stage / "jcode"
    if jcode.is_dir():
        executable = "jcode.exe" if (jcode / "jcode.exe").is_file() else "jcode"
        definitions.append(
            PackDefinition(
                component_id="org.drpa.jcode",
                display_name="JCode 开发 Agent",
                source_root=jcode,
                provides=("agent.jcode",),
                entrypoints={"jcode": executable},
            )
        )
    return definitions


def stage_component_packs(
    stage: Path,
    version: str,
    prune_legacy: bool = False,
    launcher: Path | None = None,
    output_root: Path | None = None,
    write_core_manifest: bool = True,
) -> list[Path]:
    stage = stage.resolve()
    output_root = (output_root or stage / "component-packs").resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    outputs: list[Path] = []
    for definition in component_definitions(stage):
        output = output_root / f"{definition.component_id}.drpac"
        build_pack(definition, version, "windows-x86_64", output)
        outputs.append(output)
    expected = {path.resolve() for path in outputs}
    for stale in output_root.glob("*.drpac"):
        if stale.resolve() not in expected:
            stale.unlink()
    catalog = {
        "schema": 1,
        "version": version,
        "platform": "windows-x86_64",
        "components": [path.name for path in outputs],
        "artifacts": [
            {"filename": path.name, "bytes": path.stat().st_size, "sha256": sha256(path)}
            for path in outputs
        ],
    }
    if write_core_manifest:
        catalog["coreManifest"] = "core-files.json"
    (output_root / "catalog.json").write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if prune_legacy:
        for path in (stage / "runtime", stage / "webview2", stage / "jcode"):
            if path.is_dir():
                shutil.rmtree(path)
        if (output_root / "org.drpa.desktop-ui.drpac").is_file():
            (stage / "DRPA Next.exe").unlink(missing_ok=True)
    if launcher is not None:
        launcher = launcher.resolve()
        if not launcher.is_file():
            raise ValueError(f"DRPA launcher is missing: {launcher}")
        shutil.copy2(launcher, stage / "DRPA Next.exe")
    if write_core_manifest:
        protected_roots = {"data", "components", "state"}
        core_files = []
        for path in sorted(stage.rglob("*")):
            if path.is_symlink():
                raise ValueError(f"core stage must not contain symlinks: {path}")
            if not path.is_file():
                continue
            relative = safe_relative(path.relative_to(stage).as_posix())
            if PurePosixPath(relative).parts[0].lower() in protected_roots:
                continue
            if relative in {".drpa-install.json", "component-packs/core-files.json"}:
                continue
            core_files.append(relative)
        core_files.append("component-packs/core-files.json")
        (output_root / "core-files.json").write_text(
            json.dumps({"schema": 1, "files": sorted(core_files)}, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    return outputs


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Split a DRPA Windows stage into offline .drpac components")
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--prune-legacy", action="store_true")
    parser.add_argument("--launcher", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--components-only", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    outputs = stage_component_packs(
        args.stage,
        args.version,
        args.prune_legacy,
        args.launcher,
        args.output,
        not args.components_only,
    )
    for output in outputs:
        print(output)


if __name__ == "__main__":
    main()
