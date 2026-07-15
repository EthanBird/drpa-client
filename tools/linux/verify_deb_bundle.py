from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

try:
    from tools.linux.verify_runtime_layout import sha256, verify_runtime_layout
except ModuleNotFoundError:
    from verify_runtime_layout import sha256, verify_runtime_layout


REQUIRED_DEPENDENCIES = {
    "libwebkit2gtk-4.1-0",
    "libgtk-3-0",
    "libgbm1",
    "libnss3",
    "xdg-utils",
}


def deb_field(deb: Path, field: str) -> str:
    return subprocess.run(
        ["dpkg-deb", "-f", str(deb), field],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def dependency_names(value: str) -> set[str]:
    names: set[str] = set()
    for group in value.split(","):
        for alternative in group.split("|"):
            name = alternative.strip().split(maxsplit=1)[0]
            if name:
                names.add(name)
    return names


def verify_deb_bundle(deb: Path, expected_version: str, extract_root: Path) -> dict[str, object]:
    if not deb.is_file():
        raise ValueError(f"deb package is missing: {deb}")
    if extract_root.exists() and any(extract_root.iterdir()):
        raise ValueError(f"deb extraction root must be empty: {extract_root}")
    extract_root.mkdir(parents=True, exist_ok=True)

    package = deb_field(deb, "Package")
    version = deb_field(deb, "Version")
    architecture = deb_field(deb, "Architecture")
    depends = deb_field(deb, "Depends")
    installed_size = deb_field(deb, "Installed-Size")
    errors: list[str] = []
    if version != expected_version:
        errors.append(f"deb version must be {expected_version}, got {version}")
    if architecture != "amd64":
        errors.append(f"deb architecture must be amd64, got {architecture}")
    missing_dependencies = sorted(REQUIRED_DEPENDENCIES - dependency_names(depends))
    if missing_dependencies:
        errors.append(f"deb dependencies are incomplete: {', '.join(missing_dependencies)}")

    subprocess.run(["dpkg-deb", "-x", str(deb), str(extract_root)], check=True)
    runtime_manifests = [
        path
        for path in extract_root.rglob("manifest.json")
        if path.parent.name == "runtime"
    ]
    if len(runtime_manifests) != 1:
        errors.append(f"deb must contain exactly one runtime manifest, found {len(runtime_manifests)}")
        runtime_manifest = None
    else:
        runtime_manifest = runtime_manifests[0]
        errors.extend(verify_runtime_layout(runtime_manifest.parent))

    binary_root = extract_root / "usr" / "bin"
    binaries = sorted(path for path in binary_root.glob("*") if path.is_file() or path.is_symlink())
    if not binaries:
        errors.append("deb does not install an executable under /usr/bin")
    if errors:
        raise ValueError("\n".join(errors))

    relative_runtime_manifest = runtime_manifest.relative_to(extract_root).as_posix()
    if installed_size.isdigit():
        installed_size_kib = int(installed_size)
    else:
        installed_bytes = sum(
            path.stat().st_size
            for path in extract_root.rglob("*")
            if path.is_file() and not path.is_symlink()
        )
        installed_size_kib = max(1, (installed_bytes + 1023) // 1024)
    return {
        "schemaVersion": 1,
        "platform": "linux-x86_64",
        "package": package,
        "version": version,
        "architecture": architecture,
        "depends": depends,
        "installedSizeKiB": installed_size_kib,
        "deb": {
            "filename": deb.name,
            "bytes": deb.stat().st_size,
            "sha256": sha256(deb),
        },
        "runtimeManifestPath": f"/{relative_runtime_manifest}",
        "binaryPaths": [f"/{path.relative_to(extract_root).as_posix()}" for path in binaries],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--deb", type=Path, required=True)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--extract-root", type=Path, required=True)
    parser.add_argument("--manifest-output", type=Path, required=True)
    args = parser.parse_args()
    try:
        manifest = verify_deb_bundle(
            args.deb.resolve(),
            args.expected_version,
            args.extract_root.resolve(),
        )
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(error)
        return 1
    args.manifest_output.parent.mkdir(parents=True, exist_ok=True)
    args.manifest_output.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"verified Debian package: {args.deb}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
