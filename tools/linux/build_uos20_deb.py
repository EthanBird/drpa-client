from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
from pathlib import Path

try:
    from tools.linux.build_bundled_deb import (
        PACKAGE_NAME,
        install_desktop_metadata,
        installed_size_kib,
        validate_appdir,
    )
except ModuleNotFoundError:
    from build_bundled_deb import (  # type: ignore[no-redef]
        PACKAGE_NAME,
        install_desktop_metadata,
        installed_size_kib,
        validate_appdir,
    )


INSTALL_ROOT = Path("opt/drpa-next-uos20")
PRIVATE_RUNTIME = INSTALL_ROOT / "uos-runtime"
PRIVATE_LOADER = PRIVATE_RUNTIME / "ld-linux-x86-64.so.2"
PRIVATE_LIBRARY_PATHS = (
    f"/{PRIVATE_RUNTIME.as_posix()}",
    f"/{(INSTALL_ROOT / 'usr/lib').as_posix()}",
    f"/{(INSTALL_ROOT / 'usr/lib/x86_64-linux-gnu').as_posix()}",
)
PRIVATE_RPATH = ":".join(PRIVATE_LIBRARY_PATHS)
TARGET_GLIBC_VERSION = "2.35"

# These packages are ABI boundaries supplied by the target desktop or its graphics
# driver. WebKitGTK, GTK, GStreamer, NSS, Soup, glibc and the C++ runtime are private.
BASE_DEPENDENCIES = (
    "libc6 (>= 2.28)",
    "libegl1",
    "libgl1",
    "xdg-utils",
)

# These sonames are coupled to the target system's Mesa/GLVND/DRM driver stack.
# Every other missing DT_NEEDED entry is copied recursively into the private layer.
SYSTEM_DRIVER_SONAMES = {
    "libEGL.so.1",
    "libGL.so.1",
    "libGLX.so.0",
    "libGLdispatch.so.0",
    "libOpenGL.so.0",
}
SYSTEM_LIBRARY_DIRS = (
    "lib/x86_64-linux-gnu",
    "usr/lib/x86_64-linux-gnu",
    "lib64",
    "usr/lib64",
)

SYSTEM_RUNTIME_FILES: dict[str, tuple[str, ...]] = {
    "ld-linux-x86-64.so.2": (
        "lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
        "lib64/ld-linux-x86-64.so.2",
    ),
    "libc.so.6": ("lib/x86_64-linux-gnu/libc.so.6",),
    "libm.so.6": ("lib/x86_64-linux-gnu/libm.so.6",),
    "libdl.so.2": ("lib/x86_64-linux-gnu/libdl.so.2",),
    "libpthread.so.0": ("lib/x86_64-linux-gnu/libpthread.so.0",),
    "librt.so.1": ("lib/x86_64-linux-gnu/librt.so.1",),
    "libresolv.so.2": ("lib/x86_64-linux-gnu/libresolv.so.2",),
    "libanl.so.1": ("lib/x86_64-linux-gnu/libanl.so.1",),
    "libutil.so.1": ("lib/x86_64-linux-gnu/libutil.so.1",),
    "libthread_db.so.1": ("lib/x86_64-linux-gnu/libthread_db.so.1",),
    "libgcc_s.so.1": (
        "lib/x86_64-linux-gnu/libgcc_s.so.1",
        "usr/lib/x86_64-linux-gnu/libgcc_s.so.1",
    ),
    "libstdc++.so.6": ("usr/lib/x86_64-linux-gnu/libstdc++.so.6",),
    # NSS discovers these modules with dlopen(3), so they do not appear in the
    # browser's DT_NEEDED closure and must be included explicitly.
    "libfreebl3.so": (
        "usr/lib/x86_64-linux-gnu/libfreebl3.so",
        "usr/lib/x86_64-linux-gnu/nss/libfreebl3.so",
    ),
    "libfreebl3.chk": (
        "usr/lib/x86_64-linux-gnu/libfreebl3.chk",
        "usr/lib/x86_64-linux-gnu/nss/libfreebl3.chk",
    ),
    "libfreeblpriv3.so": (
        "usr/lib/x86_64-linux-gnu/libfreeblpriv3.so",
        "usr/lib/x86_64-linux-gnu/nss/libfreeblpriv3.so",
    ),
    "libfreeblpriv3.chk": (
        "usr/lib/x86_64-linux-gnu/libfreeblpriv3.chk",
        "usr/lib/x86_64-linux-gnu/nss/libfreeblpriv3.chk",
    ),
    "libnssckbi.so": (
        "usr/lib/x86_64-linux-gnu/libnssckbi.so",
        "usr/lib/x86_64-linux-gnu/nss/libnssckbi.so",
        "usr/lib/x86_64-linux-gnu/pkcs11/p11-kit-trust.so",
    ),
    "libnssdbm3.so": (
        "usr/lib/x86_64-linux-gnu/libnssdbm3.so",
        "usr/lib/x86_64-linux-gnu/nss/libnssdbm3.so",
    ),
    "libnssdbm3.chk": (
        "usr/lib/x86_64-linux-gnu/libnssdbm3.chk",
        "usr/lib/x86_64-linux-gnu/nss/libnssdbm3.chk",
    ),
    "libsoftokn3.so": (
        "usr/lib/x86_64-linux-gnu/libsoftokn3.so",
        "usr/lib/x86_64-linux-gnu/nss/libsoftokn3.so",
    ),
    "libsoftokn3.chk": (
        "usr/lib/x86_64-linux-gnu/libsoftokn3.chk",
        "usr/lib/x86_64-linux-gnu/nss/libsoftokn3.chk",
    ),
}
SYSTEM_RUNTIME_GLOBS = ("lib/x86_64-linux-gnu/libnss_*.so.2",)
RUNTIME_COPYRIGHTS = {
    "libc6-copyright": "usr/share/doc/libc6/copyright",
    "libgcc-s1-copyright": "usr/share/doc/libgcc-s1/copyright",
    "libstdc++6-copyright": "usr/share/doc/libstdc++6/copyright",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def host_glibc_version() -> str:
    output = subprocess.run(
        ["getconf", "GNU_LIBC_VERSION"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    match = re.fullmatch(r"glibc\s+([0-9]+\.[0-9]+)", output)
    if not match:
        raise ValueError(f"cannot determine build-host glibc version from {output!r}")
    return match.group(1)


def resolved_source(root: Path, candidates: tuple[str, ...]) -> Path:
    for relative in candidates:
        candidate = root / relative
        if candidate.exists():
            resolved = candidate.resolve()
            if not resolved.is_file():
                break
            return resolved
    raise ValueError(f"required private runtime file is missing: {', '.join(candidates)}")


def resolve_runtime_soname(system_root: Path, soname: str) -> Path:
    for directory in SYSTEM_LIBRARY_DIRS:
        candidate = system_root / directory / soname
        if candidate.exists() and candidate.resolve().is_file():
            return candidate.resolve()
    raise ValueError(f"Ubuntu 22.04 runtime closure cannot resolve {soname}")


def copy_private_runtime(
    system_root: Path, target: Path, app_root: Path | None = None
) -> list[dict[str, object]]:
    target.mkdir(parents=True, exist_ok=True)
    inventory: list[dict[str, object]] = []
    copied_names: set[str] = set()

    def copy(source: Path, name: str) -> None:
        if name in copied_names:
            return
        destination = target / name
        shutil.copy2(source.resolve(), destination)
        copied_names.add(name)
        inventory.append(
            {
                "path": name,
                "source": f"/{source.relative_to(system_root).as_posix()}",
                "bytes": destination.stat().st_size,
                "sha256": sha256(destination),
            }
        )

    for name, candidates in SYSTEM_RUNTIME_FILES.items():
        copy(resolved_source(system_root, candidates), name)
    for pattern in SYSTEM_RUNTIME_GLOBS:
        for source in sorted(system_root.glob(pattern)):
            if source.exists():
                copy(source, source.name)
    required_nss = {"libnss_dns.so.2", "libnss_files.so.2"}
    missing_nss = sorted(required_nss - copied_names)
    if missing_nss:
        raise ValueError(f"private glibc NSS closure is incomplete: {', '.join(missing_nss)}")
    libstdcxx = target / "libstdc++.so.6"
    if b"GLIBCXX_3.4.30" not in libstdcxx.read_bytes():
        raise ValueError("private libstdc++.so.6 does not provide GLIBCXX_3.4.30")

    if app_root is not None:
        existing_names = {
            path.name for path in app_root.rglob("*") if path.is_file() or path.is_symlink()
        }
        existing_names.update(copied_names)
        queue = [path for path in app_root.rglob("*") if is_x86_64_elf(path)]
        queue.extend(path for path in target.rglob("*") if is_x86_64_elf(path))
        inspected: set[Path] = set()
        while queue:
            path = queue.pop()
            if path in inspected:
                continue
            inspected.add(path)
            needed = patchelf_output(path, "--print-needed")
            if needed.returncode != 0:
                continue
            for soname in needed.stdout.splitlines():
                soname = soname.strip()
                if (
                    not soname
                    or "/" in soname
                    or soname in existing_names
                    or soname in SYSTEM_DRIVER_SONAMES
                ):
                    continue
                source = resolve_runtime_soname(system_root, soname)
                copy(source, soname)
                existing_names.add(soname)
                queue.append(target / soname)
    return sorted(inventory, key=lambda entry: str(entry["path"]))


def is_x86_64_elf(path: Path) -> bool:
    if path.is_symlink() or not path.is_file():
        return False
    try:
        with path.open("rb") as stream:
            header = stream.read(20)
    except OSError:
        return False
    return (
        len(header) >= 20
        and header[:4] == b"\x7fELF"
        and header[4] == 2
        and int.from_bytes(header[18:20], "little") == 62
    )


def patchelf_output(path: Path, option: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["patchelf", option, str(path)],
        check=False,
        capture_output=True,
        text=True,
    )


def patch_appdir_elfs(app_root: Path) -> tuple[list[str], list[str]]:
    patched: list[str] = []
    interpreters: list[str] = []
    for path in sorted(app_root.rglob("*")):
        if not is_x86_64_elf(path):
            continue
        needed = patchelf_output(path, "--print-needed")
        if needed.returncode != 0:
            continue
        relative = path.relative_to(app_root).as_posix()
        original_mode = path.stat().st_mode
        current_rpath_result = patchelf_output(path, "--print-rpath")
        current_rpath = (
            current_rpath_result.stdout.strip() if current_rpath_result.returncode == 0 else ""
        )
        rpath_entries = [*PRIVATE_LIBRARY_PATHS]
        rpath_entries.extend(
            entry for entry in current_rpath.split(":") if entry and entry not in rpath_entries
        )
        subprocess.run(
            [
                "patchelf",
                "--force-rpath",
                "--set-rpath",
                ":".join(rpath_entries),
                str(path),
            ],
            check=True,
        )
        interpreter = patchelf_output(path, "--print-interpreter")
        if interpreter.returncode == 0 and interpreter.stdout.strip():
            subprocess.run(
                ["patchelf", "--set-interpreter", f"/{PRIVATE_LOADER.as_posix()}", str(path)],
                check=True,
            )
            interpreters.append(relative)
        path.chmod(original_mode)
        patched.append(relative)
    if not patched:
        raise ValueError("AppDir does not contain patchable x86_64 dynamic ELF files")
    if "usr/bin/drpa-desktop" not in interpreters:
        raise ValueError("DRPA desktop executable did not receive the private ELF interpreter")
    return patched, interpreters


def launcher_source() -> str:
    app = f"/{INSTALL_ROOT.as_posix()}"
    private_runtime = f"/{PRIVATE_RUNTIME.as_posix()}"
    return f"""#!/bin/sh
set -eu
APPDIR={app}
export APPDIR
export DRPA_PRIVATE_LOADER={private_runtime}/ld-linux-x86-64.so.2
export DRPA_PRIVATE_LIBRARY_PATH={PRIVATE_RPATH}
export GTK_DATA_PREFIX="$APPDIR"
export GTK_THEME="${{APPIMAGE_GTK_THEME:-Adwaita:light}}"
export GDK_BACKEND=x11
export XDG_DATA_DIRS="$APPDIR/usr/share:/usr/share${{XDG_DATA_DIRS:+:$XDG_DATA_DIRS}}"
export GSETTINGS_SCHEMA_DIR="$APPDIR/usr/share/glib-2.0/schemas"
export GTK_EXE_PREFIX="$APPDIR/usr"
export GTK_PATH="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0"
export GTK_IM_MODULE_FILE="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache"
export GDK_PIXBUF_MODULE_FILE="$APPDIR/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders.cache"
export GIO_EXTRA_MODULES="$APPDIR/usr/lib/x86_64-linux-gnu/gio/modules"
export GST_PLUGIN_SYSTEM_PATH_1_0="$APPDIR/usr/lib/gstreamer-1.0:$APPDIR/usr/lib/x86_64-linux-gnu/gstreamer-1.0"
export PATH="$APPDIR/usr/bin${{PATH:+:$PATH}}"
exec {app}/usr/bin/drpa-desktop "$@"
"""


def copy_runtime_copyrights(system_root: Path, documentation: Path) -> list[str]:
    copied: list[str] = []
    for name, relative in RUNTIME_COPYRIGHTS.items():
        source = system_root / relative
        if not source.exists():
            raise ValueError(f"private runtime copyright is missing: /{relative}")
        target = documentation / name
        shutil.copy2(source.resolve(), target)
        copied.append(target.name)
    return copied


def build_uos20_deb(
    appdir: Path,
    output: Path,
    package_version: str,
    work_dir: Path,
    system_root: Path = Path("/"),
) -> Path:
    appdir = appdir.resolve()
    output = output.resolve()
    work_dir = work_dir.resolve()
    system_root = system_root.resolve()
    validate_appdir(appdir)
    if not package_version or any(character.isspace() for character in package_version):
        raise ValueError(f"invalid Debian package version: {package_version!r}")
    glibc_version = host_glibc_version()
    if system_root == Path("/") and glibc_version != TARGET_GLIBC_VERSION:
        raise ValueError(
            f"UOS runtime must be assembled on glibc {TARGET_GLIBC_VERSION}, got {glibc_version}"
        )
    if work_dir.exists():
        shutil.rmtree(work_dir)
    package_root = work_dir / "package"
    app_target = package_root / INSTALL_ROOT
    app_target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(appdir, app_target, symlinks=True)

    patched, interpreters = patch_appdir_elfs(app_target)
    private_inventory = copy_private_runtime(
        system_root, package_root / PRIVATE_RUNTIME, app_target
    )

    launcher = package_root / "usr/bin/drpa-next"
    launcher.parent.mkdir(parents=True, exist_ok=True)
    launcher.write_text(launcher_source(), encoding="utf-8")
    launcher.chmod(0o755)
    (launcher.parent / "drpa-desktop").symlink_to("drpa-next")
    install_desktop_metadata(appdir, package_root)

    documentation = package_root / "usr/share/doc/drpa-next"
    documentation.mkdir(parents=True, exist_ok=True)
    copyrights = copy_runtime_copyrights(system_root, documentation)
    documentation.joinpath("README.UOS20").write_text(
        "This package is the UOS Desktop 20 / Debian 10 compatibility build.\n"
        "It installs a private glibc 2.35 dynamic loader, libstdc++ and libgcc under\n"
        "/opt/drpa-next-uos20/uos-runtime and pins every bundled ELF to that runtime.\n"
        "WebKitGTK, GTK, Python/Jupyter and Chrome remain private application files.\n"
        "GBM and generic libdrm are private because UOS-era Mesa lacks symbols required\n"
        "by WebKitGTK. EGL, GL, kernel DRM and vendor DRI components stay system-owned\n"
        "to match the installed graphics driver. User data remains in the XDG local\n"
        "data directory.\n",
        encoding="utf-8",
    )
    provenance = {
        "schemaVersion": 1,
        "target": "uos20-x86_64",
        "minimumSystemGlibc": "2.28",
        "privateGlibcVersion": glibc_version,
        "privateInterpreter": f"/{PRIVATE_LOADER.as_posix()}",
        "privateRpath": PRIVATE_RPATH,
        "systemDriverSonames": sorted(SYSTEM_DRIVER_SONAMES),
        "patchedElfCount": len(patched),
        "interpreterElfCount": len(interpreters),
        "patchedElfs": patched,
        "interpreterElfs": interpreters,
        "privateRuntimeFiles": private_inventory,
        "copyrightFiles": copyrights,
    }
    provenance_path = app_target / "uos-runtime-manifest.json"
    provenance_path.write_text(
        json.dumps(provenance, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
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
                "Description: DRPA Next offline automation desktop for UOS 20",
                " UOS Desktop 20 and Debian 10 compatibility package with a private",
                " glibc/C++ runtime, WebKitGTK, Python/Jupyter and Chrome closure.",
                " Graphics driver ABI libraries remain supplied by the target system.",
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
        description="Build a fixed-root UOS 20/glibc 2.28 compatible Debian package"
    )
    parser.add_argument("--appdir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--package-version", required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--system-root", type=Path, default=Path("/"))
    args = parser.parse_args()
    try:
        output = build_uos20_deb(
            args.appdir,
            args.output,
            args.package_version,
            args.work_dir,
            args.system_root,
        )
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(error)
        return 1
    print(f"built UOS 20 compatibility Debian package: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
