from __future__ import annotations

import argparse
import hashlib
import json
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any


CATALOG_NAME = "install-manifest.json"
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


def sha256_bytes(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


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


def collect_catalog(stage: Path) -> list[CatalogEntry]:
    entries: list[CatalogEntry] = []
    for path in sorted(stage.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(stage).as_posix()
        if relative == CATALOG_NAME or relative.casefold().startswith("data/"):
            continue
        entries.append(
            CatalogEntry(
                path=relative,
                bytes=path.stat().st_size,
                sha256=sha256(path),
                component=component_for(relative),
            )
        )
    return entries


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
        "schema": 1,
        "version": version,
        "target": "windows-x86_64",
        "files": catalog_files,
    }
    source = (json.dumps(catalog, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    return catalog, source


def read_catalog(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    source = json.loads(path.read_text(encoding="utf-8"))
    if source.get("schema") != 1 or source.get("target") != "windows-x86_64":
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
        files.append(item)

    catalog_item = {
        "path": CATALOG_NAME,
        "bytes": len(catalog_source),
        "sha256": sha256_bytes(catalog_source),
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
        "sha256": sha256(worker_path),
    }

    manifest = {
        "schema": 1,
        "version": version,
        "baseVersion": base.get("version") if base else None,
        "target": "windows-x86_64",
        "files": files,
        "remove": remove,
        "worker": worker_info,
        "totalBytes": sum(item["bytes"] for item in files),
        "components": sorted({item["component"] for item in files}),
        "catalogSha256": catalog_item["sha256"],
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
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--base-manifest", type=Path)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("--partial", action="store_true", help="merge the staged overlay into the base catalog without deleting omitted files")
    args = parser.parse_args()
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
