from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tempfile
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

from stage_component_packs import PackDefinition, build_pack


PYTHON_VERSION = "3.14.7"
PYTHON_ARCHIVE = f"python-{PYTHON_VERSION}-embed-amd64.zip"
PYTHON_URL = f"https://www.python.org/ftp/python/{PYTHON_VERSION}/{PYTHON_ARCHIVE}"
PYTHON_SHA256 = "d297e5ff019966817ad8502465176139f2d3d840fa4ed84b13bed399a6ab1f15"
COMPONENT_ID = "org.drpa.python-runtime.py314-minimal"
FEATURES = (
    "agent.python",
    "rpaz.python",
    "studio.kernel",
    "plugins.python",
    "stdlib",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def fetch_archive(cache: Path, source: Path | None = None) -> Path:
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / PYTHON_ARCHIVE
    if source is not None:
        source = source.resolve()
        if not source.is_file():
            raise ValueError(f"Python archive does not exist: {source}")
        if source != archive:
            shutil.copy2(source, archive)
    if archive.is_file() and sha256(archive) == PYTHON_SHA256:
        return archive
    archive.unlink(missing_ok=True)
    temporary = archive.with_suffix(".download")
    temporary.unlink(missing_ok=True)
    request = urllib.request.Request(PYTHON_URL, headers={"User-Agent": "DRPA-offline-builder/1"})
    try:
        with urllib.request.urlopen(request, timeout=120) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output, length=1024 * 1024)
        actual = sha256(temporary)
        if actual != PYTHON_SHA256:
            raise ValueError(f"Python archive SHA-256 mismatch: expected {PYTHON_SHA256}, got {actual}")
        os.replace(temporary, archive)
    finally:
        temporary.unlink(missing_ok=True)
    return archive


def extract_archive(archive: Path, destination: Path) -> None:
    with zipfile.ZipFile(archive) as source:
        for entry in source.infolist():
            relative = PurePosixPath(entry.filename)
            if relative.is_absolute() or any(part in {"", ".", ".."} for part in relative.parts):
                raise ValueError(f"unsafe Python archive member: {entry.filename}")
            target = destination.joinpath(*relative.parts)
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with source.open(entry) as input_file, target.open("wb") as output_file:
                shutil.copyfileobj(input_file, output_file)


def configure_embedded_search_path(stage: Path) -> None:
    pth_files = sorted(stage.glob("python*._pth"))
    if len(pth_files) != 1:
        raise ValueError(f"expected one embedded Python ._pth file, found {len(pth_files)}")
    lines = pth_files[0].read_text(encoding="utf-8").splitlines()
    normalized = [line for line in lines if line.strip() not in {"vendor", "#import site", "import site"}]
    normalized.append("vendor")
    pth_files[0].write_text("\n".join(normalized) + "\n", encoding="utf-8")


def copy_runtime_adapter(repository: Path, stage: Path) -> None:
    source = repository / "runtime" / "python" / "src" / "drpa_runner"
    if not source.is_dir():
        raise ValueError(f"DRPA Python adapter is missing: {source}")
    destination = stage / "vendor" / "drpa_runner"
    shutil.copytree(
        source,
        destination,
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "*.pyo"),
    )


def write_runtime_manifest(stage: Path, component_version: str) -> None:
    manifest = {
        "schema": 2,
        "bundleVersion": component_version,
        "displayName": "Python 3.14 Minimal",
        "platform": "windows-x86_64",
        "pythonVersion": PYTHON_VERSION,
        "pythonExecutable": "python.exe",
        "browserExecutable": "",
        "environmentMode": "frozen",
        "features": list(FEATURES),
    }
    (stage / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def build_python314_minimal(
    repository: Path,
    output: Path,
    component_version: str,
    cache: Path,
    source_archive: Path | None = None,
) -> Path:
    archive = fetch_archive(cache.resolve(), source_archive)
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="drpa-python314-", dir=output.parent) as temporary:
        stage = Path(temporary) / "runtime"
        stage.mkdir()
        extract_archive(archive, stage)
        copy_runtime_adapter(repository.resolve(), stage)
        configure_embedded_search_path(stage)
        write_runtime_manifest(stage, component_version)
        build_pack(
            PackDefinition(
                component_id=COMPONENT_ID,
                display_name="Python 3.14 Minimal",
                source_root=stage,
                provides=("runtime.python", "runtime.rpaz", "runtime.python.3.14.minimal"),
                entrypoints={"python": "python.exe"},
            ),
            component_version,
            "windows-x86_64",
            output,
        )
    update_component_catalog(output.parent, component_version)
    return output


def update_component_catalog(output_root: Path, component_version: str) -> None:
    catalog_path = output_root / "catalog.json"
    if not catalog_path.is_file():
        return
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    packs = sorted(output_root.glob("*.drpac"), key=lambda path: path.name)
    catalog["version"] = str(catalog.get("version") or component_version)
    catalog["components"] = [path.name for path in packs]
    catalog["artifacts"] = [
        {"filename": path.name, "bytes": path.stat().st_size, "sha256": sha256(path)}
        for path in packs
    ]
    catalog_path.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build the frozen Python 3.14 Minimal DRPA component")
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--component-version", required=True)
    parser.add_argument("--cache", type=Path, required=True, help="Non-system cache directory for the official archive")
    parser.add_argument("--source-archive", type=Path, help="Use a pre-downloaded official archive")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    output = build_python314_minimal(
        args.repository,
        args.output,
        args.component_version,
        args.cache,
        args.source_archive,
    )
    print(output)


if __name__ == "__main__":
    main()
