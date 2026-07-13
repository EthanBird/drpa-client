from __future__ import annotations

import argparse
import hashlib
import json
import zipfile
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build(stage: Path, output: Path, version: str) -> None:
    stage = stage.resolve()
    files = []
    for path in sorted(stage.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(stage).as_posix()
        if relative.startswith("data/") or relative.casefold() == "drpa-updater.exe":
            continue
        # Fixed WebView2 is version-pinned and very large. It is updated only by
        # an installer release; normal file-level application updates omit it.
        if relative.startswith("webview2/"):
            continue
        files.append({"path": relative, "bytes": path.stat().st_size, "sha256": sha256(path)})
    manifest = {
        "schema": 1,
        "version": version,
        "target": "windows-x86_64",
        "files": files,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        archive.writestr("update-manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
        for item in files:
            archive.write(stage / item["path"], f"files/{item['path']}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    build(args.stage, args.output, args.version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
