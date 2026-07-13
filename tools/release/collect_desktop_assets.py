#!/usr/bin/env python3
"""Collect Tauri bundles into stable GitHub Release asset names."""

from __future__ import annotations

import argparse
import hashlib
import shutil
from pathlib import Path


EXPECTED_EXTENSIONS = {
    "windows-x86_64": (".exe",),
    "linux-x86_64": (".appimage", ".deb"),
    "macos-arm64": (".dmg",),
    "macos-x86_64": (".dmg",),
}

DISPLAY_EXTENSIONS = {
    ".appimage": ".AppImage",
    ".deb": ".deb",
    ".dmg": ".dmg",
    ".exe": ".exe",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def collect(*, platform: str, version: str, bundle_dir: Path, output: Path) -> list[Path]:
    expected = EXPECTED_EXTENSIONS[platform]
    if not bundle_dir.is_dir():
        raise FileNotFoundError(f"Tauri bundle directory does not exist: {bundle_dir}")

    candidates: dict[str, list[Path]] = {extension: [] for extension in expected}
    for path in bundle_dir.rglob("*"):
        if path.is_file() and path.suffix.lower() in candidates:
            candidates[path.suffix.lower()].append(path)

    problems = [
        f"{extension}: expected exactly one bundle, found {len(paths)}"
        for extension, paths in candidates.items()
        if len(paths) != 1
    ]
    if problems:
        raise RuntimeError("; ".join(problems))

    output.mkdir(parents=True, exist_ok=True)
    written: list[Path] = []
    for extension in expected:
        source = candidates[extension][0]
        filename = f"drpa-next-{version}-preview-{platform}{DISPLAY_EXTENSIONS[extension]}"
        destination = output / filename
        shutil.copy2(source, destination)

        checksum = output / f"{filename}.sha256"
        checksum.write_text(f"{sha256(destination)}  {filename}\n", encoding="utf-8")
        written.extend((destination, checksum))
        print(f"collected {source} -> {destination}")

    return written


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", choices=sorted(EXPECTED_EXTENSIONS), required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--bundle-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    collect(
        platform=args.platform,
        version=args.version,
        bundle_dir=args.bundle_dir,
        output=args.output,
    )


if __name__ == "__main__":
    main()
