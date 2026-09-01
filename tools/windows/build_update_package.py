from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any


CATALOG_NAME = "install-manifest.json"
UPDATE_SCHEMA = 2
HOST_PROTOCOL = 2
WORKER_PROTOCOL = 2
MINIMUM_HOST_VERSION = "0.3.0"
PROTECTED_PREFIXES = ("data/", "webview2/")
PROTECTED_FILES: set[str] = set()


@dataclass(frozen=True)
class CatalogEntry:
    path: str
    bytes: int
    sha256: str
    component: str

    def as_dict(self) -> dict[str, Any]:
        return {
            "path": self.path,
            "bytes": self.bytes,
            "sha256": self.sha256,
            "component": self.component,
        }


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def component_for(relative: str) -> str:
    folded = relative.casefold()
    if folded == "drpa-updater.exe":
        return "updater"
    if folded.startswith("webview2/"):
        return "webview2"
    if folded.startswith("runtime/browser/"):
        return "browser"
    if folded.startswith("runtime/"):
        return "runtime"
    if folded.startswith("examples/"):
        return "examples"
    if folded.endswith(".md"):
        return "documentation"
    return "application"


def is_protected(relative: str) -> bool:
    folded = relative.casefold()
    return folded in PROTECTED_FILES or any(folded.startswith(prefix) for prefix in PROTECTED_PREFIXES)


def is_link_or_reparse_point(path: Path) -> bool:
    metadata = path.lstat()
    file_attributes = getattr(metadata, "st_file_attributes", 0)
    reparse_flag = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)
    return stat.S_ISLNK(metadata.st_mode) or bool(file_attributes & reparse_flag)


def collect_catalog(stage: Path) -> list[CatalogEntry]:
    entries: list[CatalogEntry] = []
    for root, directories, filenames in os.walk(stage, followlinks=False):
        root_path = Path(root)
        directories.sort()
        filenames.sort()

        for directory in list(directories):
            path = root_path / directory
            relative = path.relative_to(stage).as_posix()
            if relative.casefold() == "data" or relative.casefold().startswith("data/"):
                directories.remove(directory)
                continue
            if is_link_or_reparse_point(path):
                raise ValueError(f"install stage contains a link or reparse point: {relative}")

        for filename in filenames:
            path = root_path / filename
            relative = path.relative_to(stage).as_posix()
            if relative == CATALOG_NAME or relative.casefold().startswith("data/"):
                continue
            if is_link_or_reparse_point(path):
                raise ValueError(f"install stage contains a link or reparse point: {relative}")
            entries.append(
                CatalogEntry(
                    path=relative,
                    bytes=path.stat().st_size,
                    sha256=sha256(path),
                    component=component_for(relative),
                )
            )
    return sorted(entries, key=lambda entry: entry.path)


def make_catalog(
    stage: Path,
    version: str,
    *,
    base: dict[str, Any] | None = None,
    partial: bool = False,
) -> tuple[dict[str, Any], bytes]:
    entries = collect_catalog(stage)
    if partial:
        if base is None:
            raise ValueError("partial update requires a base install manifest")
        merged = {item["path"]: item for item in base.get("files", [])}
        merged.update({entry.path: entry.as_dict() for entry in entries})
        catalog_files = [merged[path] for path in sorted(merged)]
    else:
        catalog_files = [entry.as_dict() for entry in entries]
    catalog = {
        "schema": UPDATE_SCHEMA,
        "version": version,
        "target": "windows-x86_64",
        "updateProtocol": HOST_PROTOCOL,
        "files": catalog_files,
    }
    source = (json.dumps(catalog, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    return catalog, source


def read_catalog(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    source = json.loads(path.read_text(encoding="utf-8"))
    if source.get("schema") not in {1, UPDATE_SCHEMA} or source.get("target") != "windows-x86_64":
        raise ValueError("base install manifest is invalid")
    return source


def build(
    stage: Path,
    output: Path,
    version: str,
    *,
    base_manifest: Path | None = None,
    worker: Path | None = None,
    partial: bool = False,
) -> dict[str, Any]:
    stage = stage.resolve()
    base = read_catalog(base_manifest)
    catalog, catalog_source = make_catalog(stage, version, base=base, partial=partial)
    (stage / CATALOG_NAME).write_bytes(catalog_source)

    current = {item["path"]: item for item in catalog["files"]}
    previous = {item["path"]: item for item in (base or {}).get("files", [])}
    files: list[dict[str, Any]] = []

    for relative, item in sorted(current.items()):
        if is_protected(relative):
            continue
        old = previous.get(relative)
        if base is None and item["component"] in {"runtime", "browser"}:
            continue
        if old is not None and old.get("sha256") == item["sha256"]:
            continue
        files.append(
            {
                "path": item["path"],
                "bytes": item["bytes"],
                "component": item["component"],
            }
        )

    catalog_item = {
        "path": CATALOG_NAME,
        "bytes": len(catalog_source),
        "component": "catalog",
    }
    files.append(catalog_item)

    remove = [] if partial else [
        path for path in sorted(previous.keys() - current.keys()) if not is_protected(path)
    ]

    worker_path = (worker or stage / "drpa-updater.exe").resolve()
    if not worker_path.is_file():
        raise FileNotFoundError(f"update worker is missing: {worker_path}")
    worker_info = {
        "bytes": worker_path.stat().st_size,
    }

    manifest = {
        "schema": UPDATE_SCHEMA,
        "hostProtocol": HOST_PROTOCOL,
        "workerProtocol": WORKER_PROTOCOL,
        "packageKind": "delta" if base else "bootstrap",
        "minimumHostVersion": MINIMUM_HOST_VERSION,
        "version": version,
        "baseVersion": base.get("version") if base else None,
        "target": "windows-x86_64",
        "files": files,
        "remove": remove,
        "worker": worker_info,
        "totalBytes": sum(item["bytes"] for item in files),
        "components": sorted({item["component"] for item in files}),
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        archive.writestr("update-manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
        archive.writestr(f"files/{CATALOG_NAME}", catalog_source)
        archive.write(worker_path, "worker/drpa-updater.exe")
        for item in files:
            if item["path"] == CATALOG_NAME:
                continue
            archive.write(stage / item["path"], f"files/{item['path']}")
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--base-manifest", type=Path)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("--partial", action="store_true", help="merge the staged overlay into the base catalog without deleting omitted files")
    parser.add_argument("--catalog-only", action="store_true", help="write install-manifest.json without creating an update package")
    args = parser.parse_args()
    if args.catalog_only:
        base = read_catalog(args.base_manifest)
        _, source = make_catalog(args.stage.resolve(), args.version, base=base, partial=args.partial)
        (args.stage / CATALOG_NAME).write_bytes(source)
        return 0
    if args.output is None:
        parser.error("--output is required unless --catalog-only is used")
    build(
        args.stage,
        args.output,
        args.version,
        base_manifest=args.base_manifest,
        worker=args.worker,
        partial=args.partial,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
