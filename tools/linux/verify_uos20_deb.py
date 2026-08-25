from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

try:
    from tools.linux.build_uos20_deb import (
        BASE_DEPENDENCIES,
        INSTALL_ROOT,
        PRIVATE_LIBRARY_PATHS,
        PRIVATE_LOADER,
        PRIVATE_DRI,
        PRIVATE_EGL_VENDOR,
        PRIVATE_RUNTIME,
        is_x86_64_elf,
        sha256,
    )
    from tools.linux.verify_deb_bundle import deb_field, dependency_names
    from tools.linux.verify_runtime_layout import verify_runtime_layout
except ModuleNotFoundError:
    from build_uos20_deb import (  # type: ignore[no-redef]
        BASE_DEPENDENCIES,
        INSTALL_ROOT,
        PRIVATE_LIBRARY_PATHS,
        PRIVATE_LOADER,
        PRIVATE_DRI,
        PRIVATE_EGL_VENDOR,
        PRIVATE_RUNTIME,
        is_x86_64_elf,
        sha256,
    )
    from verify_deb_bundle import deb_field, dependency_names  # type: ignore[no-redef]
    from verify_runtime_layout import verify_runtime_layout  # type: ignore[no-redef]


REQUIRED_DEPENDENCIES = {dependency.split(maxsplit=1)[0] for dependency in BASE_DEPENDENCIES}
FORBIDDEN_DEPENDENCIES = {
    "libgcc-s1",
    "libstdc++6",
    "libwebkit2gtk-4.1-0",
    "libjavascriptcoregtk-4.1-0",
    "libgtk-3-0",
    "libgbm1",
    "libdrm2",
    "libegl1",
    "libgl1",
}
REQUIRED_PRIVATE_RUNTIME_FILES = {
    "ld-linux-x86-64.so.2",
    "libc.so.6",
    "libm.so.6",
    "libdl.so.2",
    "libpthread.so.0",
    "librt.so.1",
    "libresolv.so.2",
    "libnss_dns.so.2",
    "libnss_files.so.2",
    "libgcc_s.so.1",
    "libstdc++.so.6",
    "libfreebl3.so",
    "libfreebl3.chk",
    "libfreeblpriv3.so",
    "libfreeblpriv3.chk",
    "libnssckbi.so",
    "libnssdbm3.so",
    "libnssdbm3.chk",
    "libsoftokn3.so",
    "libsoftokn3.chk",
    "libgbm.so.1",
    "libdrm.so.2",
    "libX11.so.6",
    "libasound.so.2",
    "libfontconfig.so.1",
    "libfreetype.so.6",
    "libfribidi.so.0",
    "libxcb.so.1",
    "libEGL.so.1",
    "libGL.so.1",
    "libGLX.so.0",
    "libGLdispatch.so.0",
    "libOpenGL.so.0",
    "libEGL_mesa.so.0",
    "libGLX_mesa.so.0",
    "libglapi.so.0",
}


def is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def patchelf_value(path: Path, option: str) -> tuple[int, str]:
    result = subprocess.run(
        ["patchelf", option, str(path)],
        check=False,
        capture_output=True,
        text=True,
    )
    return result.returncode, result.stdout.strip()


def verify_uos20_deb(deb: Path, expected_version: str, extract_root: Path) -> dict[str, object]:
    if not deb.is_file():
        raise ValueError(f"deb package is missing: {deb}")
    if extract_root.exists() and any(extract_root.iterdir()):
        raise ValueError(f"deb extraction root must be empty: {extract_root}")
    extract_root.mkdir(parents=True, exist_ok=True)

    package = deb_field(deb, "Package")
    version = deb_field(deb, "Version")
    architecture = deb_field(deb, "Architecture")
    depends = deb_field(deb, "Depends")
    dependency_set = dependency_names(depends)
    errors: list[str] = []
    if package != "drpa-next":
        errors.append(f"deb package must be drpa-next, got {package}")
    if version != expected_version:
        errors.append(f"deb version must be {expected_version}, got {version}")
    if architecture != "amd64":
        errors.append(f"deb architecture must be amd64, got {architecture}")
    missing_dependencies = sorted(REQUIRED_DEPENDENCIES - dependency_set)
    if missing_dependencies:
        errors.append(f"UOS base dependencies are incomplete: {', '.join(missing_dependencies)}")
    forbidden_dependencies = sorted(FORBIDDEN_DEPENDENCIES & dependency_set)
    if forbidden_dependencies:
        errors.append(
            "UOS package depends on libraries that must be private: "
            + ", ".join(forbidden_dependencies)
        )
    if "libc6 (>= 2.28)" not in depends:
        errors.append("UOS package must declare libc6 (>= 2.28)")

    subprocess.run(["dpkg-deb", "-x", str(deb), str(extract_root)], check=True)
    app_root = extract_root / INSTALL_ROOT
    private_root = extract_root / PRIVATE_RUNTIME
    private_files = (
        sorted(
            path.relative_to(private_root).as_posix()
            for path in private_root.rglob("*")
            if path.is_file()
        )
        if private_root.is_dir()
        else []
    )
    jcode_candidates = [
        path
        for path in app_root.rglob("jcode")
        if path.is_file() and path.parent.name == "jcode"
    ]
    if len(jcode_candidates) != 1:
        errors.append(f"UOS deb must contain exactly one Linux JCode sidecar, found {len(jcode_candidates)}")
        jcode = None
    else:
        jcode = jcode_candidates[0]
        if not jcode.stat().st_mode & 0o111:
            errors.append("UOS Linux JCode sidecar is not executable")
        jcode_binary = jcode.with_name("jcode.bin")
        if not jcode_binary.is_file() or not jcode_binary.stat().st_mode & 0o111:
            errors.append("UOS JCode native binary is missing or not executable")
    private_names = {Path(path).name for path in private_files}
    missing_private = sorted(REQUIRED_PRIVATE_RUNTIME_FILES - private_names)
    if missing_private:
        errors.append(f"private UOS runtime is incomplete: {', '.join(missing_private)}")
    private_gbm = private_root / "libgbm.so.1"
    if (
        private_gbm.is_file()
        and b"gbm_bo_create_with_modifiers2" not in private_gbm.read_bytes()
    ):
        errors.append("private libgbm.so.1 lacks gbm_bo_create_with_modifiers2")
    private_drm = private_root / "libdrm.so.2"
    if (
        private_drm.is_file()
        and b"drmGetFormatModifierName" not in private_drm.read_bytes()
    ):
        errors.append("private libdrm.so.2 lacks drmGetFormatModifierName")
    private_dri = extract_root / PRIVATE_DRI
    for driver in ("swrast_dri.so", "kms_swrast_dri.so"):
        if not (private_dri / driver).is_file():
            errors.append(f"private Mesa software driver is missing: {driver}")
    private_egl_vendor = extract_root / PRIVATE_EGL_VENDOR
    if not private_egl_vendor.is_file():
        errors.append("private Mesa EGL vendor manifest is missing")
    elif "libEGL_mesa.so.0" not in private_egl_vendor.read_text(encoding="utf-8"):
        errors.append("private Mesa EGL vendor manifest does not select libEGL_mesa.so.0")

    provenance_path = app_root / "uos-runtime-manifest.json"
    if not provenance_path.is_file():
        errors.append("UOS runtime provenance manifest is missing")
        provenance: dict[str, object] = {}
    else:
        provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
        if provenance.get("target") != "uos20-x86_64":
            errors.append("UOS runtime provenance target is invalid")
        if provenance.get("minimumSystemGlibc") != "2.28":
            errors.append("UOS runtime provenance minimum glibc must be 2.28")
        if provenance.get("privateGlibcVersion") != "2.35":
            errors.append("UOS runtime provenance private glibc must be 2.35")
        if provenance.get("renderingMode") != "private-mesa-llvmpipe":
            errors.append("UOS runtime must declare the private Mesa llvmpipe renderer")
        if provenance.get("privateDriPath") != f"/{PRIVATE_DRI.as_posix()}":
            errors.append("UOS runtime private DRI path is invalid")
        if provenance.get("privateEglVendorManifest") != f"/{PRIVATE_EGL_VENDOR.as_posix()}":
            errors.append("UOS runtime private EGL vendor manifest is invalid")

    expected_interpreter = f"/{PRIVATE_LOADER.as_posix()}"
    patched_elfs = 0
    interpreter_elfs = 0
    runpath_elfs: list[str] = []
    for path in sorted(app_root.rglob("*")):
        if is_within(path, private_root) or not is_x86_64_elf(path):
            continue
        needed_status, _ = patchelf_value(path, "--print-needed")
        if needed_status != 0:
            continue
        patched_elfs += 1
        relative = path.relative_to(app_root).as_posix()
        _, rpath = patchelf_value(path, "--print-rpath")
        for required in PRIVATE_LIBRARY_PATHS:
            if required not in rpath.split(":"):
                errors.append(f"ELF private RPATH is incomplete: {relative}")
                break
        dynamic = subprocess.run(
            ["readelf", "-d", str(path)],
            check=False,
            capture_output=True,
            text=True,
        ).stdout
        if "(RUNPATH)" in dynamic or "(RPATH)" not in dynamic:
            runpath_elfs.append(relative)
        interpreter_status, interpreter = patchelf_value(path, "--print-interpreter")
        if interpreter_status == 0 and interpreter:
            interpreter_elfs += 1
            if interpreter != expected_interpreter:
                errors.append(f"ELF uses a system interpreter: {relative}: {interpreter}")
    if runpath_elfs:
        errors.append(
            "ELFs must use transitive DT_RPATH, not DT_RUNPATH: " + ", ".join(runpath_elfs[:10])
        )
    if patched_elfs != provenance.get("patchedElfCount"):
        errors.append("UOS runtime patched ELF inventory does not match package contents")
    if interpreter_elfs != provenance.get("interpreterElfCount"):
        errors.append("UOS runtime interpreter ELF inventory does not match package contents")

    launcher = extract_root / "usr/bin/drpa-next"
    if not launcher.is_file() or not launcher.stat().st_mode & 0o111:
        errors.append("deb does not install executable /usr/bin/drpa-next")
    else:
        launcher_text = launcher.read_text(encoding="utf-8")
        if "LD_LIBRARY_PATH" in launcher_text:
            errors.append("UOS launcher must not export LD_LIBRARY_PATH to system child processes")
        if f"/{INSTALL_ROOT.as_posix()}/usr/bin/drpa-desktop" not in launcher_text:
            errors.append("UOS launcher does not execute the fixed-root desktop binary")
        if 'cd "$APPDIR/usr"' not in launcher_text:
            errors.append("UOS launcher does not resolve relocated WebKit helper paths")
        if "WEBKIT_EXEC_PATH" in launcher_text:
            errors.append("production WebKitGTK ignores WEBKIT_EXEC_PATH")
        required_rendering_settings = (
            "WEBKIT_DISABLE_DMABUF_RENDERER=1",
            "WEBKIT_DISABLE_COMPOSITING_MODE=1",
            "LIBGL_ALWAYS_SOFTWARE=1",
            "DRPA_UI_REDUCED_EFFECTS=1",
            "GALLIUM_DRIVER=",
            'LIBGL_DRIVERS_PATH="$APPDIR/uos-runtime/dri"',
            '__EGL_VENDOR_LIBRARY_FILENAMES="$APPDIR/uos-runtime/egl_vendor.d/50_mesa.json"',
        )
        for setting in required_rendering_settings:
            if setting not in launcher_text:
                errors.append(f"UOS launcher is missing software-rendering setting: {setting}")
        required_gtk_isolation_settings = (
            'GTK_PATH="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0"',
            "unset GTK_MODULES GTK3_MODULES",
            "NO_AT_BRIDGE=1",
            "GTK_IM_MODULE=gtk-im-context-simple",
            "GDK_CORE_DEVICE_EVENTS=1",
        )
        for setting in required_gtk_isolation_settings:
            if setting not in launcher_text:
                errors.append(f"UOS launcher is missing private-GTK isolation setting: {setting}")
        if 'GTK_PATH="$APPDIR/usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/' in launcher_text:
            errors.append("UOS launcher must not load target-system GTK modules")

    runtime_manifests = [
        path for path in app_root.rglob("manifest.json") if path.parent.name == "runtime"
    ]
    if len(runtime_manifests) != 1:
        errors.append(f"UOS deb must contain exactly one sealed runtime, found {len(runtime_manifests)}")
        runtime_manifest = None
    else:
        runtime_manifest = runtime_manifests[0]
        errors.extend(verify_runtime_layout(runtime_manifest.parent))
    if errors:
        raise ValueError("\n".join(errors))

    assert runtime_manifest is not None
    return {
        "schemaVersion": 1,
        "platform": "linux-x86_64-uos20",
        "package": package,
        "version": version,
        "architecture": architecture,
        "minimumSystemGlibc": "2.28",
        "privateGlibcVersion": "2.35",
        "renderingMode": "private-mesa-llvmpipe",
        "inputMethodMode": "gtk-im-context-simple",
        "inputEventMode": "x11-core",
        "depends": depends,
        "deb": {
            "filename": deb.name,
            "bytes": deb.stat().st_size,
            "sha256": sha256(deb),
        },
        "installRoot": f"/{INSTALL_ROOT.as_posix()}",
        "privateInterpreter": expected_interpreter,
        "patchedElfCount": patched_elfs,
        "interpreterElfCount": interpreter_elfs,
        "runtimeManifestPath": f"/{runtime_manifest.relative_to(extract_root).as_posix()}",
        "jcodePath": (
            f"/{jcode.relative_to(extract_root).as_posix()}" if jcode is not None else None
        ),
        "privateRuntimeFiles": private_files,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--deb", type=Path, required=True)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--extract-root", type=Path, required=True)
    parser.add_argument("--manifest-output", type=Path, required=True)
    args = parser.parse_args()
    try:
        manifest = verify_uos20_deb(
            args.deb.resolve(), args.expected_version, args.extract_root.resolve()
        )
    except (OSError, json.JSONDecodeError, subprocess.CalledProcessError, ValueError) as error:
        print(error)
        return 1
    args.manifest_output.parent.mkdir(parents=True, exist_ok=True)
    args.manifest_output.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"verified UOS 20 Debian package: {args.deb}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
