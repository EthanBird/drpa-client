from __future__ import annotations

import argparse
import os
import shutil
import subprocess
from pathlib import Path

try:
    from tools.linux.verify_runtime_layout import verify_runtime_layout
except ModuleNotFoundError:
    from verify_runtime_layout import verify_runtime_layout


PACKAGE_NAME = "drpa-next"
INSTALL_ROOT = Path("opt/drpa-next")
BASE_DEPENDENCIES = (
    "libc6 (>= 2.35)",
    "libgcc-s1",
    "libstdc++6",
    "libegl1",
    "libgl1",
    "libgbm1",
)
REQUIRED_APPDIR_PATHS = (
    "AppRun",
    "AppRun.wrapped",
    "usr/bin/drpa-desktop",
    "usr/lib/DRPA Next/jcode/jcode",
    "usr/lib/DRPA Next/jcode/jcode.bin",
    "usr/lib/libwebkit2gtk-4.1.so.0",
    "usr/lib/libjavascriptcoregtk-4.1.so.0",
    "usr/lib/libgtk-3.so.0",
    "usr/lib/libgdk-3.so.0",
    "usr/lib/libgstreamer-1.0.so.0",
    "usr/lib/libnss3.so",
    "usr/lib/libsoup-3.0.so.0",
    "usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess",
    "usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess",
    "usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/injected-bundle/libwebkit2gtkinjectedbundle.so",
)


def validate_appdir(appdir: Path) -> Path:
    if not appdir.is_dir():
        raise ValueError(f"AppDir is missing: {appdir}")
    missing = [relative for relative in REQUIRED_APPDIR_PATHS if not (appdir / relative).exists()]
    if missing:
        raise ValueError(f"AppDir desktop runtime is incomplete: {', '.join(missing)}")
    jcode = appdir / "usr/lib/DRPA Next/jcode/jcode"
    if not jcode.is_file() or not os.access(jcode, os.X_OK):
        raise ValueError("AppDir JCode sidecar must be an executable Linux binary")
    jcode_binary = appdir / "usr/lib/DRPA Next/jcode/jcode.bin"
    if not jcode_binary.is_file() or not os.access(jcode_binary, os.X_OK):
        raise ValueError("AppDir JCode native binary must be present and executable")
    runtime_manifests = [
        path for path in appdir.rglob("manifest.json") if path.parent.name == "runtime"
    ]
    if len(runtime_manifests) != 1:
        raise ValueError(
            f"AppDir must contain exactly one runtime manifest, found {len(runtime_manifests)}"
        )
    errors = verify_runtime_layout(runtime_manifests[0].parent)
    if errors:
        raise ValueError("AppDir sealed runtime is invalid:\n" + "\n".join(errors))
    return runtime_manifests[0]


def installed_size_kib(root: Path) -> int:
    installed_bytes = sum(
        path.stat().st_size
        for path in root.rglob("*")
        if path.is_file() and not path.is_symlink() and "DEBIAN" not in path.parts
    )
    return max(1, (installed_bytes + 1023) // 1024)


def install_desktop_metadata(appdir: Path, package_root: Path) -> None:
    desktop_files = sorted((appdir / "usr/share/applications").glob("*.desktop"))
    if len(desktop_files) != 1:
        raise ValueError(f"AppDir must contain exactly one desktop file, found {len(desktop_files)}")
    desktop_lines = []
    for line in desktop_files[0].read_text(encoding="utf-8").splitlines():
        if line.startswith("Exec="):
            line = "Exec=/usr/bin/drpa-next"
        elif line.startswith("TryExec="):
            line = "TryExec=/usr/bin/drpa-next"
        desktop_lines.append(line)
    desktop_target = package_root / "usr/share/applications/drpa-next.desktop"
    desktop_target.parent.mkdir(parents=True, exist_ok=True)
    desktop_target.write_text("\n".join(desktop_lines) + "\n", encoding="utf-8")

    icons_source = appdir / "usr/share/icons/hicolor"
    if not icons_source.is_dir():
        raise ValueError("AppDir hicolor icons are missing")
    shutil.copytree(
        icons_source,
        package_root / "usr/share/icons/hicolor",
        symlinks=True,
        dirs_exist_ok=True,
    )


def build_bundled_deb(
    appdir: Path,
    output: Path,
    package_version: str,
    work_dir: Path,
) -> Path:
    appdir = appdir.resolve()
    output = output.resolve()
    work_dir = work_dir.resolve()
    validate_appdir(appdir)
    if not package_version or any(character.isspace() for character in package_version):
        raise ValueError(f"invalid Debian package version: {package_version!r}")
    if work_dir.exists():
        shutil.rmtree(work_dir)
    package_root = work_dir / "package"
    app_target = package_root / INSTALL_ROOT
    app_target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(appdir, app_target, symlinks=True)

    launcher = package_root / "usr/bin/drpa-next"
    launcher.parent.mkdir(parents=True, exist_ok=True)
    launcher.write_text('#!/bin/sh\nexec /opt/drpa-next/AppRun "$@"\n', encoding="utf-8")
    launcher.chmod(0o755)
    compatibility_launcher = launcher.parent / "drpa-desktop"
    compatibility_launcher.symlink_to("drpa-next")
    install_desktop_metadata(appdir, package_root)

    documentation = package_root / "usr/share/doc/drpa-next"
    documentation.mkdir(parents=True, exist_ok=True)
    documentation.joinpath("README.Debian").write_text(
        "DRPA Next installs its private, AppImage-derived desktop runtime under "
        "/opt/drpa-next. Third-party license files are preserved under "
        "/opt/drpa-next/usr/share/doc. User data remains outside the package in "
        "the XDG local data directory.\n",
        encoding="utf-8",
    )

    control_root = package_root / "DEBIAN"
    control_root.mkdir(parents=True, exist_ok=True)
    control_root.joinpath("control").write_text(
        "\n".join(
            [
                f"Package: {PACKAGE_NAME}",
                f"Version: {package_version}",
                "Section: devel",
                "Priority: optional",
                "Architecture: amd64",
                f"Installed-Size: {installed_size_kib(package_root)}",
                f"Depends: {', '.join(BASE_DEPENDENCIES)}",
                "Maintainer: DRPA Next maintainers",
                "Description: DRPA Next offline automation desktop",
                " Self-contained x86_64 desktop package with a private WebKitGTK,",
                " GTK, sealed Python/Jupyter and Chrome runtime. User data is kept",
                " in the XDG local data directory and is not removed with the package.",
                "",
            ]
        ),
        encoding="utf-8",
    )

    output.parent.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.setdefault("XZ_OPT", "-T0")
    subprocess.run(
        [
            "dpkg-deb",
            "--root-owner-group",
            "-Zxz",
            "-z6",
            "--build",
            str(package_root),
            str(output),
        ],
        check=True,
        env=environment,
    )
    return output


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build a WebKitGTK-self-contained deb from a verified AppImage AppDir"
    )
    parser.add_argument("--appdir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--package-version", required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        output = build_bundled_deb(
            args.appdir,
            args.output,
            args.package_version,
            args.work_dir,
        )
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(error)
        return 1
    print(f"built self-contained Debian package: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
