from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path

try:
    from tools.linux.build_bundled_deb import installed_size_kib
    from tools.linux.build_uos20_deb import (
        INSTALL_ROOT,
        PRIVATE_LIBRARY_PATHS,
        PRIVATE_LOADER,
        PRIVATE_RUNTIME,
        is_x86_64_elf,
        patchelf_output,
    )
except ModuleNotFoundError:
    from build_bundled_deb import installed_size_kib  # type: ignore[no-redef]
    from build_uos20_deb import (  # type: ignore[no-redef]
        INSTALL_ROOT,
        PRIVATE_LIBRARY_PATHS,
        PRIVATE_LOADER,
        PRIVATE_RUNTIME,
        is_x86_64_elf,
        patchelf_output,
    )


DESKTOP_BINARY = Path("usr/bin/drpa-desktop")
JCODE_DIRECTORY = Path("usr/lib/DRPA Next/jcode")
REQUIRED_JCODE_FILES = {"jcode", "jcode.bin", "LICENSE.txt", "VERSION.txt"}


def is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def validate_package_version(package_version: str) -> None:
    if not package_version or any(character.isspace() for character in package_version):
        raise ValueError(f"invalid Debian package version: {package_version!r}")


def validate_inputs(base_deb: Path, desktop_binary: Path, jcode_dir: Path) -> None:
    if not base_deb.is_file():
        raise ValueError(f"base UOS package is missing: {base_deb}")
    if not desktop_binary.is_file() or not is_x86_64_elf(desktop_binary):
        raise ValueError(f"desktop binary is not an x86_64 ELF: {desktop_binary}")
    if not jcode_dir.is_dir():
        raise ValueError(f"Linux JCode directory is missing: {jcode_dir}")
    staged = {path.name for path in jcode_dir.iterdir() if path.is_file()}
    missing = sorted(REQUIRED_JCODE_FILES - staged)
    if missing:
        raise ValueError(f"Linux JCode staging is incomplete: {', '.join(missing)}")
    unexpected = sorted(staged - REQUIRED_JCODE_FILES)
    if unexpected:
        raise ValueError(f"Linux JCode staging contains unmanaged files: {', '.join(unexpected)}")
    if not is_x86_64_elf(jcode_dir / "jcode.bin"):
        raise ValueError("Linux JCode native binary is not an x86_64 ELF")


def patch_application_elfs(app_root: Path) -> tuple[list[str], list[str]]:
    private_root = app_root / PRIVATE_RUNTIME.relative_to(INSTALL_ROOT)
    patched: list[str] = []
    interpreters: list[str] = []
    for path in sorted(app_root.rglob("*")):
        if is_within(path, private_root) or not is_x86_64_elf(path):
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
    if DESKTOP_BINARY.as_posix() not in interpreters:
        raise ValueError("DRPA desktop executable did not receive the private ELF interpreter")
    jcode_binary = (JCODE_DIRECTORY / "jcode.bin").as_posix()
    if jcode_binary not in interpreters:
        raise ValueError("JCode native executable did not receive the private ELF interpreter")
    return patched, interpreters


def replace_control_field(control: Path, field: str, value: str) -> None:
    lines = control.read_text(encoding="utf-8").splitlines()
    prefix = f"{field}:"
    matches = [index for index, line in enumerate(lines) if line.startswith(prefix)]
    if len(matches) != 1:
        raise ValueError(f"Debian control file must contain exactly one {field} field")
    lines[matches[0]] = f"{field}: {value}"
    control.write_text("\n".join(lines) + "\n", encoding="utf-8")


def repack_uos20_deb(
    base_deb: Path,
    desktop_binary: Path,
    jcode_dir: Path,
    package_version: str,
    output: Path,
    work_dir: Path,
) -> Path:
    base_deb = base_deb.resolve()
    desktop_binary = desktop_binary.resolve()
    jcode_dir = jcode_dir.resolve()
    output = output.resolve()
    work_dir = work_dir.resolve()
    validate_package_version(package_version)
    validate_inputs(base_deb, desktop_binary, jcode_dir)

    if work_dir.exists():
        shutil.rmtree(work_dir)
    package_root = work_dir / "package"
    package_root.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["dpkg-deb", "-R", str(base_deb), str(package_root)], check=True)

    app_root = package_root / INSTALL_ROOT
    private_loader = package_root / PRIVATE_LOADER
    provenance_path = app_root / "uos-runtime-manifest.json"
    if not private_loader.is_file() or not provenance_path.is_file():
        raise ValueError("base package is not a validated fixed-root UOS 20 package")

    desktop_target = app_root / DESKTOP_BINARY
    shutil.copy2(desktop_binary, desktop_target)
    desktop_target.chmod(0o755)

    jcode_target = app_root / JCODE_DIRECTORY
    if jcode_target.exists():
        shutil.rmtree(jcode_target)
    shutil.copytree(jcode_dir, jcode_target)
    (jcode_target / "jcode").chmod(0o755)
    (jcode_target / "jcode.bin").chmod(0o755)

    patched, interpreters = patch_application_elfs(app_root)
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    provenance["patchedElfCount"] = len(patched)
    provenance["interpreterElfCount"] = len(interpreters)
    provenance["patchedElfs"] = patched
    provenance["interpreterElfs"] = interpreters
    provenance_path.write_text(
        json.dumps(provenance, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )

    control = package_root / "DEBIAN/control"
    replace_control_field(control, "Version", package_version)
    replace_control_field(control, "Installed-Size", str(installed_size_kib(package_root)))

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
        description="Repack a validated UOS 20 package with a current DRPA host and Linux JCode"
    )
    parser.add_argument("--base-deb", type=Path, required=True)
    parser.add_argument("--desktop-binary", type=Path, required=True)
    parser.add_argument("--jcode-dir", type=Path, required=True)
    parser.add_argument("--package-version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = repack_uos20_deb(
            args.base_deb,
            args.desktop_binary,
            args.jcode_dir,
            args.package_version,
            args.output,
            args.work_dir,
        )
    except (OSError, json.JSONDecodeError, subprocess.CalledProcessError, ValueError) as error:
        print(error)
        return 1
    print(f"built UOS 20 Debian package: {result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
