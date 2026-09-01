from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
from pathlib import Path, PurePosixPath


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_file(root: Path, relative: str) -> Path:
    logical = PurePosixPath(relative)
    if logical.is_absolute() or not logical.parts or ".." in logical.parts or "\\" in relative:
        raise ValueError(f"unsafe runtime path: {relative}")
    target = root.joinpath(*logical.parts)
    if not target.is_file():
        raise ValueError(f"runtime file is missing: {relative}")
    return target


def verify_runtime_layout(root: Path) -> list[str]:
    errors: list[str] = []
    try:
        manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [f"invalid runtime manifest: {error}"]
    try:
        wheel_lock = json.loads((root / "wheelhouse-lock.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [f"invalid wheelhouse lock: {error}"]

    if manifest.get("platform") != "linux-x86_64":
        errors.append(f"runtime platform must be linux-x86_64, got {manifest.get('platform')!r}")
    if wheel_lock.get("platform") != "linux-x86_64":
        errors.append(f"wheel lock platform must be linux-x86_64, got {wheel_lock.get('platform')!r}")
    if manifest.get("pythonVersion") != wheel_lock.get("pythonVersion"):
        errors.append("manifest and wheel lock Python versions differ")

    executables = [
        manifest.get("pythonExecutable"),
        manifest.get("browserExecutable"),
        "tools/uv",
    ]
    for relative in executables:
        if not isinstance(relative, str):
            errors.append("runtime manifest is missing an executable path")
            continue
        try:
            target = resolve_file(root, relative)
        except ValueError as error:
            errors.append(str(error))
            continue
        mode_is_executable = bool(target.stat().st_mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH))
        if not os.access(target, os.X_OK) or (os.name != "nt" and not mode_is_executable):
            errors.append(f"runtime executable lost its mode bits: {relative}")

    for relative in (
        "bootstrap_runtime.py",
        "locks/runtime.txt",
        "locks/rpa-for-python.json",
        "rpa/rpa_python.zip",
        "rpa/asset-lock.json",
    ):
        try:
            resolve_file(root, relative)
        except ValueError as error:
            errors.append(str(error))

    try:
        rpa_lock = json.loads((root / "rpa/asset-lock.json").read_text(encoding="utf-8"))
        rpa_bundle = resolve_file(root, "rpa/rpa_python.zip")
        if rpa_lock.get("platform") != "linux-x86_64":
            errors.append("RPA asset lock platform must be linux-x86_64")
        if rpa_lock.get("bundle", {}).get("bytes") != rpa_bundle.stat().st_size:
            errors.append("RPA offline bundle size mismatch")
        if rpa_lock.get("bundle", {}).get("sha256") != sha256(rpa_bundle):
            errors.append("RPA offline bundle hash mismatch")
    except (OSError, ValueError, json.JSONDecodeError, AttributeError) as error:
        errors.append(f"invalid RPA for Python asset lock: {error}")

    source_builds = wheel_lock.get("sourceBuilds")
    source_names = {
        item.get("name")
        for item in source_builds
        if isinstance(item, dict)
    } if isinstance(source_builds, list) else set()
    if not {"rpa", "tagui"}.issubset(source_names):
        errors.append("wheelhouse lock is missing RPA for Python source provenance")

    wheels = wheel_lock.get("wheels")
    if not isinstance(wheels, list) or not wheels:
        errors.append("wheelhouse lock does not contain wheels")
        return errors
    for item in wheels:
        if not isinstance(item, dict) or not isinstance(item.get("filename"), str):
            errors.append("wheelhouse lock contains an invalid entry")
            continue
        relative = f"wheelhouse/{item['filename']}"
        try:
            wheel = resolve_file(root, relative)
        except ValueError as error:
            errors.append(str(error))
            continue
        if wheel.stat().st_size != item.get("bytes"):
            errors.append(f"wheel size mismatch: {item['filename']}")
        if sha256(wheel) != item.get("sha256"):
            errors.append(f"wheel hash mismatch: {item['filename']}")
        lowered = item["filename"].lower()
        if any(token in lowered for token in ("win32", "win_amd64", "macosx", "musllinux")):
            errors.append(f"non-glibc Linux wheel in layout: {item['filename']}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime-root", type=Path, required=True)
    args = parser.parse_args()
    errors = verify_runtime_layout(args.runtime_root.resolve())
    if errors:
        print("\n".join(errors))
        return 1
    print(f"verified Linux runtime layout: {args.runtime_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
