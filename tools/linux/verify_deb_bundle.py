from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

try:
    from tools.linux.verify_runtime_layout import sha256, verify_runtime_layout
except ModuleNotFoundError:
    from verify_runtime_layout import sha256, verify_runtime_layout


REQUIRED_BASE_DEPENDENCIES = {
    "libc6",
    "libgcc-s1",
    "libstdc++6",
    "libegl1",
    "libgl1",
    "libgbm1",
}
FORBIDDEN_DESKTOP_DEPENDENCIES = {
    "libwebkit2gtk-4.1-0",
    "libjavascriptcoregtk-4.1-0",
    "libgtk-3-0",
    "libnss3",
}
REQUIRED_BUNDLED_PATHS = (
    "opt/drpa-next/AppRun",
    "opt/drpa-next/AppRun.wrapped",
    "opt/drpa-next/usr/lib/DRPA Next/jcode/jcode",
    "opt/drpa-next/usr/lib/DRPA Next/jcode/jcode.bin",
    "opt/drpa-next/usr/lib/libwebkit2gtk-4.1.so.0",
    "opt/drpa-next/usr/lib/libjavascriptcoregtk-4.1.so.0",
    "opt/drpa-next/usr/lib/libgtk-3.so.0",
    "opt/drpa-next/usr/lib/libgdk-3.so.0",
    "opt/drpa-next/usr/lib/libgstreamer-1.0.so.0",
    "opt/drpa-next/usr/lib/libnss3.so",
    "opt/drpa-next/usr/lib/libsoup-3.0.so.0",
    "opt/drpa-next/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess",
    "opt/drpa-next/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess",
    "opt/drpa-next/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/injected-bundle/libwebkit2gtkinjectedbundle.so",
)


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
    dependency_set = dependency_names(depends)
    missing_dependencies = sorted(REQUIRED_BASE_DEPENDENCIES - dependency_set)
    if missing_dependencies:
        errors.append(f"deb base dependencies are incomplete: {', '.join(missing_dependencies)}")
    forbidden_dependencies = sorted(FORBIDDEN_DESKTOP_DEPENDENCIES & dependency_set)
    if forbidden_dependencies:
        errors.append(
            "deb must not depend on system desktop libraries: "
            + ", ".join(forbidden_dependencies)
        )

    subprocess.run(["dpkg-deb", "-x", str(deb), str(extract_root)], check=True)
    missing_bundled_paths = [
        relative for relative in REQUIRED_BUNDLED_PATHS if not (extract_root / relative).exists()
    ]
    if missing_bundled_paths:
        errors.append(
            "deb private desktop runtime is incomplete: " + ", ".join(missing_bundled_paths)
        )
    jcode = extract_root / "opt/drpa-next/usr/lib/DRPA Next/jcode/jcode"
    if not jcode.is_file() or not jcode.stat().st_mode & 0o111:
        errors.append("deb does not contain executable Linux JCode sidecar")
    jcode_binary = extract_root / "opt/drpa-next/usr/lib/DRPA Next/jcode/jcode.bin"
    if not jcode_binary.is_file() or not jcode_binary.stat().st_mode & 0o111:
        errors.append("deb does not contain executable JCode native binary")
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

    launcher = extract_root / "usr/bin/drpa-next"
    if not launcher.is_file() or not launcher.stat().st_mode & 0o111:
        errors.append("deb does not install executable /usr/bin/drpa-next")
    elif "/opt/drpa-next/AppRun" not in launcher.read_text(encoding="utf-8"):
        errors.append("deb launcher does not execute the private AppDir")
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
        "jcodePath": "/opt/drpa-next/usr/lib/DRPA Next/jcode/jcode",
        "binaryPaths": [f"/{path.relative_to(extract_root).as_posix()}" for path in binaries],
        "bundledDesktopRuntimePaths": [f"/{path}" for path in REQUIRED_BUNDLED_PATHS],
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
