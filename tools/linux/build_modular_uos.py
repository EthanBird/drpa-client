from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import subprocess
from pathlib import Path


CORE_PACKAGE = "drpa-next-core"
SYSTEM_INSTALL_ROOT = Path("/opt/drpa-next-uos20")
CORE_PROGRAM_ROOT = Path("/usr/lib/drpa-next")
PLATFORM = "linux-x86_64"
DESKTOP_COMPONENT_ID = "org.drpa.desktop-ui"
PYTHON_COMPONENT_ID = "org.drpa.python-runtime"


def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=True, text=True, **kwargs)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def installed_size_kib(root: Path) -> int:
    size = sum(
        path.stat().st_size
        for path in root.rglob("*")
        if path.is_file() and not path.is_symlink() and "DEBIAN" not in path.parts
    )
    return max(1, (size + 1023) // 1024)


def executable(path: Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def write_executable(path: Path, source: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(source, encoding="utf-8")
    executable(path)


def core_environment() -> str:
    return """DRPA_INSTALL_ROOT=/opt/drpa-next-uos20
export DRPA_INSTALL_ROOT
if [ -n "${XDG_DATA_HOME:-}" ]; then
    DRPA_DATA_ROOT="$XDG_DATA_HOME/drpa-next"
else
    DRPA_DATA_ROOT="$HOME/.local/share/drpa-next"
fi
export DRPA_DATA_ROOT
export DRPA_CORE_CLI=/usr/lib/drpa-next/drpa
export DRPA_COMPONENT_INSTALLER=/usr/lib/drpa-next/drpa-component-installer
export DRPA_LAUNCHER=/usr/bin/drpa-next
LIBGL_ALWAYS_SOFTWARE=${LIBGL_ALWAYS_SOFTWARE:-1}
GALLIUM_DRIVER=${GALLIUM_DRIVER:-llvmpipe}
EGL_PLATFORM=${EGL_PLATFORM:-x11}
export LIBGL_ALWAYS_SOFTWARE GALLIUM_DRIVER EGL_PLATFORM
"""


def install_icons(workspace: Path, package_root: Path) -> None:
    icon_root = workspace / "apps/desktop/src-tauri/icons"
    for size, name in ((32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png")):
        source = icon_root / name
        if not source.is_file():
            continue
        for icon_name in ("drpa-next.png", "drpa-desktop.png"):
            target = (
                package_root
                / "usr/share/icons/hicolor"
                / (str(size) + "x" + str(size))
                / ("apps/" + icon_name)
            )
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(str(source), str(target))


def build_core_deb(
    workspace: Path,
    binaries: Path,
    output: Path,
    package_version: str,
    work_dir: Path,
) -> Path:
    required = ("drpa", "drpa-launcher", "drpa-component-installer")
    missing = [name for name in required if not (binaries / name).is_file()]
    if missing:
        raise ValueError("missing Linux Core binaries: " + ", ".join(missing))
    if work_dir.exists():
        shutil.rmtree(str(work_dir))
    package_root = work_dir / "package"
    program_root = package_root / CORE_PROGRAM_ROOT.relative_to("/")
    program_root.mkdir(parents=True, exist_ok=True)
    for name in required:
        target = program_root / name
        shutil.copy2(str(binaries / name), str(target))
        executable(target)

    bin_root = package_root / "usr/bin"
    environment = core_environment()
    write_executable(
        bin_root / "drpa",
        "#!/bin/sh\nset -eu\n" + environment + 'exec /usr/lib/drpa-next/drpa "$@"\n',
    )
    write_executable(
        bin_root / "drpa-next",
        "#!/bin/sh\nset -eu\n"
        + environment
        + 'exec /usr/lib/drpa-next/drpa-launcher "$@"\n',
    )
    write_executable(
        bin_root / "drpa-component-installer",
        "#!/bin/sh\nset -eu\n"
        + environment
        + 'exec /usr/lib/drpa-next/drpa-component-installer "$@"\n',
    )

    applications = package_root / "usr/share/applications"
    applications.mkdir(parents=True, exist_ok=True)
    applications.joinpath("drpa-next.desktop").write_text(
        """[Desktop Entry]
Type=Application
Name=DRPA Next
Name[zh_CN]=DRPA Next
Comment=Data · Runtime · Process · AI
Comment[zh_CN]=数据 · 运行 · 流程 · AI
Exec=/usr/bin/drpa-next
TryExec=/usr/bin/drpa-next
Icon=drpa-next
StartupWMClass=drpa-desktop
X-GNOME-WMClass=drpa-desktop
Terminal=false
Categories=Development;Utility;
StartupNotify=true
""",
        encoding="utf-8",
    )
    applications.joinpath("drpa-component-installer.desktop").write_text(
        """[Desktop Entry]
Type=Application
Name=DRPA Component Setup
Name[zh_CN]=DRPA 组件安装向导
Comment=Install, repair or remove offline DRPA components
Comment[zh_CN]=安装、修复或卸载离线 DRPA 组件
Exec=/usr/bin/drpa-component-installer
TryExec=/usr/bin/drpa-component-installer
Icon=drpa-next
Terminal=false
Categories=Development;Utility;
StartupNotify=true
""",
        encoding="utf-8",
    )
    install_icons(workspace, package_root)

    documentation = package_root / "usr/share/doc" / CORE_PACKAGE
    documentation.mkdir(parents=True, exist_ok=True)
    documentation.joinpath("README.UOS").write_text(
        "DRPA Core provides the drpa CLI, stable drpa-next launcher and a native "
        "egui component guide. Large desktop, Python, browser and JCode runtimes "
        "are installed independently from .drpac files. Components live under "
        "/opt/drpa-next-uos20 and may require PolicyKit authorization; user data "
        "lives under the XDG data directory.\n",
        encoding="utf-8",
    )

    control_root = package_root / "DEBIAN"
    control_root.mkdir(parents=True, exist_ok=True)
    control_root.joinpath("control").write_text(
        "\n".join(
            [
                "Package: " + CORE_PACKAGE,
                "Version: " + package_version,
                "Section: devel",
                "Priority: optional",
                "Architecture: amd64",
                "Installed-Size: " + str(installed_size_kib(package_root)),
                "Depends: libc6 (>= 2.28), libgcc1, libx11-6, libxcb1, libxcursor1, libxi6, libxrandr2, libxkbcommon0, libwayland-client0, libwayland-cursor0, libgl1, libegl1, policykit-1, xdg-utils",
                "Conflicts: drpa-next",
                "Replaces: drpa-next (<= 2.1.1-1+uos20.6)",
                "Provides: drpa-next",
                "Maintainer: DRPA Next maintainers",
                "Description: DRPA Next lightweight Core for UOS 20",
                " CLI-first runtime, stable desktop launcher and native component guide.",
                " Large GUI and automation runtimes are distributed as independent .drpac files.",
                "",
            ]
        ),
        encoding="utf-8",
    )
    write_executable(
        control_root / "postinst",
        """#!/bin/sh
set -e
ROOT=/opt/drpa-next-uos20
install -d -m 0755 "$ROOT" "$ROOT/components" "$ROOT/state" /var/lib/drpa-next/locator
if [ ! -f "$ROOT/.drpa-install.json" ]; then
    DRPA_LOCATOR_HOME=/var/lib/drpa-next/locator \
        /usr/lib/drpa-next/drpa --install-root "$ROOT" install init "$ROOT" --channel stable >/dev/null
fi
chmod 0755 "$ROOT" "$ROOT/components" "$ROOT/state"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q /usr/share/icons/hicolor || true
exit 0
""",
    )

    output.parent.mkdir(parents=True, exist_ok=True)
    environment_vars = os.environ.copy()
    environment_vars.setdefault("XZ_OPT", "-T0")
    run(
        [
            "dpkg-deb",
            "--root-owner-group",
            "-Zxz",
            "-z6",
            "--build",
            str(package_root),
            str(output),
        ],
        env=environment_vars,
    )
    return output


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


def patchelf(path: Path, option: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["patchelf", option, str(path)],
        check=False,
        capture_output=True,
        text=True,
    )


def patch_desktop_component(root: Path, version: str) -> dict[str, object]:
    fixed_root = SYSTEM_INSTALL_ROOT / "components" / DESKTOP_COMPONENT_ID / version
    apprun = root / "AppRun"
    source = apprun.read_text(encoding="utf-8")
    anchor = 'source "$this_dir"/apprun-hooks/"linuxdeploy-plugin-gtk.sh"\n'
    if anchor not in source:
        raise ValueError("cannot inject the modular desktop runtime environment into AppRun")
    environment = r'''
LIBGL_ALWAYS_SOFTWARE=${LIBGL_ALWAYS_SOFTWARE:-1}
GALLIUM_DRIVER=${GALLIUM_DRIVER:-llvmpipe}
EGL_PLATFORM=${EGL_PLATFORM:-x11}
LIBGL_DRIVERS_PATH=${LIBGL_DRIVERS_PATH:-$this_dir/uos-runtime/dri}
export LIBGL_ALWAYS_SOFTWARE GALLIUM_DRIVER EGL_PLATFORM LIBGL_DRIVERS_PATH

# Preserve the DDE session input method. The old package forced GTK's simple
# context, which disabled Chinese composition even while Fcitx was running.
DRPA_REQUESTED_GTK_IM_MODULE=${DRPA_GTK_IM_MODULE:-${GTK_IM_MODULE:-}}
case "$DRPA_REQUESTED_GTK_IM_MODULE" in
  fcitx|fcitx5|ibus)
    DRPA_SYSTEM_GTK_IM_CACHE=/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache
    if [ -r "$DRPA_SYSTEM_GTK_IM_CACHE" ] && grep -Fq "\"$DRPA_REQUESTED_GTK_IM_MODULE\"" "$DRPA_SYSTEM_GTK_IM_CACHE"; then
      GTK_IM_MODULE=$DRPA_REQUESTED_GTK_IM_MODULE
      GTK_IM_MODULE_FILE=$DRPA_SYSTEM_GTK_IM_CACHE
      GTK_PATH="$this_dir/usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0"
      export GTK_IM_MODULE GTK_IM_MODULE_FILE GTK_PATH
    else
      GTK_IM_MODULE=xim
      export GTK_IM_MODULE
    fi
    ;;
  xim)
    GTK_IM_MODULE=xim
    export GTK_IM_MODULE
    ;;
  *)
    GTK_IM_MODULE=gtk-im-context-simple
    export GTK_IM_MODULE
    ;;
esac
if [ "$DRPA_REQUESTED_GTK_IM_MODULE" = fcitx ] || [ "$DRPA_REQUESTED_GTK_IM_MODULE" = fcitx5 ]; then
  XMODIFIERS=${XMODIFIERS:-@im=fcitx}
  export XMODIFIERS
elif [ "$DRPA_REQUESTED_GTK_IM_MODULE" = ibus ]; then
  XMODIFIERS=${XMODIFIERS:-@im=ibus}
  export XMODIFIERS
fi
unset DRPA_REQUESTED_GTK_IM_MODULE DRPA_SYSTEM_GTK_IM_CACHE
'''
    apprun.write_text(source.replace(anchor, anchor + environment, 1), encoding="utf-8")

    provenance = {
        "schemaVersion": 1,
        "platform": PLATFORM,
        "component": DESKTOP_COMPONENT_ID,
        "version": version,
        "fixedInstallRoot": str(fixed_root),
        "legacyCompatibilityLinks": ["usr", "uos-runtime"],
        "patchedElfCount": 0,
        "interpreterElfCount": 0,
    }
    (root / "drpa-component-provenance.json").write_text(
        json.dumps(provenance, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return provenance


def patch_system_loader_component(
    root: Path, component_id: str, version: str
) -> dict[str, object]:
    fixed_root = SYSTEM_INSTALL_ROOT / "components" / component_id / version
    old_root = str(SYSTEM_INSTALL_ROOT)
    interpreter = Path("/lib64/ld-linux-x86-64.so.2")
    patched: list[str] = []
    interpreters: list[str] = []
    for path in sorted(root.rglob("*")):
        if not is_x86_64_elf(path):
            continue
        needed = patchelf(path, "--print-needed")
        if needed.returncode != 0:
            continue
        mode = path.stat().st_mode
        current_rpath = patchelf(path, "--print-rpath")
        if current_rpath.returncode == 0:
            relocated = ":".join(
                entry
                for entry in current_rpath.stdout.strip().split(":")
                if entry and not entry.startswith(old_root + "/")
            )
            if relocated:
                run(["patchelf", "--force-rpath", "--set-rpath", relocated, str(path)])
            elif current_rpath.stdout.strip():
                run(["patchelf", "--remove-rpath", str(path)])
        current_interpreter = patchelf(path, "--print-interpreter")
        if current_interpreter.returncode == 0 and current_interpreter.stdout.strip():
            run(["patchelf", "--set-interpreter", str(interpreter), str(path)])
            interpreters.append(path.relative_to(root).as_posix())
        path.chmod(mode)
        patched.append(path.relative_to(root).as_posix())

    provenance = {
        "schemaVersion": 1,
        "platform": PLATFORM,
        "component": component_id,
        "version": version,
        "fixedInstallRoot": str(fixed_root),
        "systemInterpreter": str(interpreter),
        "patchedElfCount": len(patched),
        "interpreterElfCount": len(interpreters),
    }
    (root / "drpa-component-provenance.json").write_text(
        json.dumps(provenance, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return provenance


def patch_python_component(root: Path, version: str) -> dict[str, object]:
    return patch_system_loader_component(root, PYTHON_COMPONENT_ID, version)


def copy_materialized(source: Path, target: Path) -> None:
    if target.exists():
        shutil.rmtree(str(target))
    shutil.copytree(str(source), str(target), symlinks=False)


def apply_python_wheel_overlay(root: Path, overlay: Path | None) -> list[str]:
    if overlay is None:
        return []
    if not overlay.is_dir():
        raise ValueError("Python wheel overlay directory does not exist: " + str(overlay))
    wheelhouse = root / "wheelhouse"
    installed: list[str] = []
    for source in sorted(overlay.glob("*.whl")):
        distribution = source.name.split("-", 1)[0]
        if not distribution:
            raise ValueError("invalid wheel overlay filename: " + source.name)
        normalized = distribution.replace("-", "_").lower()
        for existing in wheelhouse.glob("*.whl"):
            current = existing.name.split("-", 1)[0].replace("-", "_").lower()
            if current == normalized:
                existing.unlink()
        target = wheelhouse / source.name
        shutil.copy2(str(source), str(target))
        installed.append(source.name)
    if not installed:
        raise ValueError("Python wheel overlay does not contain any .whl files")
    (root / "drpa-wheel-overlay.json").write_text(
        json.dumps(
            {
                "schemaVersion": 1,
                "platform": "uos20-x86_64",
                "wheels": [
                    {"file": name, "sha256": sha256(wheelhouse / name)} for name in installed
                ],
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    return installed


def runtime_browser_entry(runtime: Path) -> str:
    manifest = json.loads((runtime / "manifest.json").read_text(encoding="utf-8"))
    relative = str(manifest.get("browserExecutable", "")).replace("\\", "/")
    if relative.startswith("browser/"):
        relative = relative[len("browser/") :]
    if not relative or not (runtime / "browser" / relative).is_file():
        candidates = list((runtime / "browser").rglob("chrome"))
        if len(candidates) != 1:
            raise ValueError("cannot resolve Chromium entrypoint from sealed runtime")
        relative = candidates[0].relative_to(runtime / "browser").as_posix()
    return relative


def pack_component(
    drpa: Path,
    source: Path,
    output: Path,
    component_id: str,
    version: str,
    name: str,
    provides: list[str],
    entrypoints: dict[str, str],
) -> Path:
    command = [
        str(drpa),
        "component",
        "pack",
        str(source),
        str(output),
        "--id",
        component_id,
        "--version",
        version,
        "--name",
        name,
        "--platform",
        PLATFORM,
    ]
    for capability in provides:
        command.extend(["--provide", capability])
    for key, path in entrypoints.items():
        command.extend(["--entry", key + "=" + path])
    run(command)
    run([str(drpa), "component", "inspect", str(output)])
    return output


def build_components(
    source_deb: Path,
    drpa: Path,
    output_dir: Path,
    version: str,
    work_dir: Path,
    python_wheel_overlay: Path | None = None,
) -> list[Path]:
    extract_root = work_dir / "legacy-extract"
    if extract_root.exists():
        shutil.rmtree(str(extract_root))
    extract_root.mkdir(parents=True)
    run(["dpkg-deb", "-x", str(source_deb), str(extract_root)])
    app_root = extract_root / SYSTEM_INSTALL_ROOT.relative_to("/")
    runtime = app_root / "usr/lib/DRPA Next/runtime"
    jcode = app_root / "usr/lib/DRPA Next/jcode"
    for required in (app_root / "AppRun", runtime / "manifest.json", jcode / "jcode"):
        if not required.is_file():
            raise ValueError("legacy UOS package is missing " + str(required))

    sources = work_dir / "component-sources"
    if sources.exists():
        shutil.rmtree(str(sources))
    sources.mkdir(parents=True)

    desktop_source = sources / "desktop"
    copy_materialized(app_root, desktop_source)
    shutil.rmtree(str(desktop_source / "usr/lib/DRPA Next/runtime"))
    shutil.rmtree(str(desktop_source / "usr/lib/DRPA Next/jcode"))
    patch_desktop_component(desktop_source, version)

    python_source = sources / "python-runtime"
    copy_materialized(runtime, python_source)
    shutil.rmtree(str(python_source / "browser"))
    apply_python_wheel_overlay(python_source, python_wheel_overlay)
    patch_python_component(python_source, version)

    browser_source = sources / "chromium"
    browser_entry = runtime_browser_entry(runtime)
    copy_materialized(runtime / "browser", browser_source)
    patch_system_loader_component(browser_source, "org.drpa.browser.chromium", version)

    jcode_source = sources / "jcode"
    copy_materialized(jcode, jcode_source)
    patch_system_loader_component(jcode_source, "org.drpa.jcode", version)

    output_dir.mkdir(parents=True, exist_ok=True)
    artifacts = [
        pack_component(
            drpa,
            desktop_source,
            output_dir / (DESKTOP_COMPONENT_ID + ".drpac"),
            DESKTOP_COMPONENT_ID,
            version,
            "DRPA Desktop UI for UOS 20",
            ["desktop.ui"],
            {"desktop": "AppRun"},
        ),
        pack_component(
            drpa,
            python_source,
            output_dir / "org.drpa.python-runtime.drpac",
            PYTHON_COMPONENT_ID,
            version,
            "Python / Jupyter / RPAZ Runtime for UOS 20",
            ["runtime.python"],
            {},
        ),
        pack_component(
            drpa,
            browser_source,
            output_dir / "org.drpa.browser.chromium.drpac",
            "org.drpa.browser.chromium",
            version,
            "Chromium for UOS 20",
            ["browser.chromium"],
            {"browser": browser_entry},
        ),
        pack_component(
            drpa,
            jcode_source,
            output_dir / "org.drpa.jcode.drpac",
            "org.drpa.jcode",
            version,
            "JCode for UOS 20",
            ["agent.jcode"],
            {"jcode": "jcode"},
        ),
    ]
    return artifacts


def write_bundle_manifest(
    output_dir: Path,
    version: str,
    package_version: str,
    source_deb: Path | None,
    artifacts: list[Path],
) -> Path:
    manifest = {
        "schemaVersion": 1,
        "platform": "linux-x86_64-uos20",
        "version": version,
        "corePackageVersion": package_version,
        "installRoot": str(SYSTEM_INSTALL_ROOT),
        "sourceDeb": (
            {
                "path": source_deb.name,
                "bytes": source_deb.stat().st_size,
                "sha256": sha256(source_deb),
            }
            if source_deb is not None
            else None
        ),
        "artifacts": [
            {"file": path.name, "bytes": path.stat().st_size, "sha256": sha256(path)}
            for path in artifacts
        ],
    }
    path = output_dir / ("drpa-modular-" + version + "-uos20-manifest.json")
    path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for artifact in artifacts:
        artifact.with_suffix(artifact.suffix + ".sha256").write_text(
            sha256(artifact) + "  " + artifact.name + "\n", encoding="utf-8"
        )
    return path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build UOS 20 DRPA Core deb and independent .drpac components"
    )
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--core-bin-dir", type=Path, required=True)
    parser.add_argument("--source-deb", type=Path)
    parser.add_argument(
        "--python-wheel-overlay",
        type=Path,
        help="directory containing UOS-built wheels that replace incompatible sealed wheels",
    )
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--version", default="3.0.1")
    parser.add_argument("--package-version", default="3.0.1-1+uos20.modular1")
    parser.add_argument("--core-only", action="store_true")
    parser.add_argument(
        "--manifest-only",
        action="store_true",
        help="reuse existing deb/drpac files and only refresh checksums and bundle manifest",
    )
    args = parser.parse_args()

    workspace = args.workspace.resolve()
    output_dir = args.output_dir.resolve()
    work_dir = args.work_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    core_output = output_dir / (
        "drpa-next-core-" + args.package_version + "-linux-x86_64.deb"
    )
    component_outputs = [
        output_dir / (DESKTOP_COMPONENT_ID + ".drpac"),
        output_dir / "org.drpa.python-runtime.drpac",
        output_dir / "org.drpa.browser.chromium.drpac",
        output_dir / "org.drpa.jcode.drpac",
    ]
    source_deb = args.source_deb.resolve() if args.source_deb else None
    if args.manifest_only:
        artifacts = [core_output] + component_outputs
        missing = [str(path) for path in artifacts if not path.is_file()]
        if missing:
            raise ValueError("cannot refresh bundle manifest; missing: " + ", ".join(missing))
        manifest = write_bundle_manifest(
            output_dir,
            args.version,
            args.package_version,
            source_deb,
            artifacts,
        )
        print("refreshed UOS modular bundle manifest: " + str(manifest))
        return 0
    artifacts = [
        build_core_deb(
            workspace,
            args.core_bin_dir.resolve(),
            core_output,
            args.package_version,
            work_dir / "core-deb",
        )
    ]
    if not args.core_only:
        if source_deb is None:
            raise ValueError("--source-deb is required unless --core-only is used")
        artifacts.extend(
            build_components(
                source_deb,
                args.core_bin_dir.resolve() / "drpa",
                output_dir,
                args.version,
                work_dir / "components",
                args.python_wheel_overlay.resolve() if args.python_wheel_overlay else None,
            )
        )
    manifest = write_bundle_manifest(
        output_dir,
        args.version,
        args.package_version,
        source_deb,
        artifacts,
    )
    print("built UOS modular bundle: " + str(manifest))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
