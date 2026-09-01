from __future__ import annotations

import base64
import hashlib
import json
import re
import shutil
import stat
import tarfile
import urllib.request
import zipfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SPEC_PATH = ROOT / "offline" / "rpa-for-python.json"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_rpa_spec(path: Path = SPEC_PATH) -> dict[str, Any]:
    spec = json.loads(path.read_text(encoding="utf-8"))
    if spec.get("schema") != 1 or not spec.get("sourceDistributions"):
        raise RuntimeError("RPA for Python offline specification is invalid")
    return spec


def source_build_names(spec: dict[str, Any]) -> set[str]:
    return {str(item["name"]).lower().replace("_", "-") for item in spec["sourceDistributions"]}


def write_binary_requirements(source: Path, target: Path, spec: dict[str, Any]) -> None:
    excluded = source_build_names(spec)
    found: set[str] = set()
    output: list[str] = []
    for raw_line in source.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        name = line.split(";", 1)[0].split("==", 1)[0].strip().lower().replace("_", "-")
        if line and not line.startswith("#") and name in excluded:
            found.add(name)
            continue
        output.append(raw_line)
    missing = excluded - found
    if missing:
        raise RuntimeError(f"source-built packages are absent from runtime requirements: {sorted(missing)}")
    target.write_text("\n".join(output) + "\n", encoding="utf-8")


def _download_verified(url: str, target: Path, expected_sha256: str, expected_bytes: int | None = None) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.is_file() and (expected_bytes is None or target.stat().st_size == expected_bytes) and sha256(target) == expected_sha256:
        return
    request = urllib.request.Request(url, headers={"User-Agent": "DRPA-offline-builder/1"})
    with urllib.request.urlopen(request, timeout=180) as response, target.open("wb") as output:
        shutil.copyfileobj(response, output, length=1024 * 1024)
    if expected_bytes is not None and target.stat().st_size != expected_bytes:
        raise RuntimeError(f"downloaded asset size mismatch: {target.name}")
    actual = sha256(target)
    if actual != expected_sha256:
        raise RuntimeError(f"downloaded asset hash mismatch: {target.name} ({actual})")


def _safe_extract_tar(archive: Path, destination: Path) -> None:
    destination = destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:*") as source:
        for member in source.getmembers():
            target = (destination / member.name).resolve()
            if target != destination and destination not in target.parents:
                raise RuntimeError(f"source archive path escapes destination: {member.name}")
            if member.issym() or member.islnk() or member.isdev():
                raise RuntimeError(f"source archive contains unsupported entry: {member.name}")
        source.extractall(destination, filter="data")


def _safe_extract_zip(archive: Path, destination: Path) -> None:
    destination = destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive) as source:
        for item in source.infolist():
            target = (destination / item.filename).resolve()
            if target != destination and destination not in target.parents:
                raise RuntimeError(f"engine archive path escapes destination: {item.filename}")
            source.extract(item, destination)
            mode = (item.external_attr >> 16) & 0o777
            if mode and target.exists():
                target.chmod(mode)


def patch_tagui_source(source: str) -> str:
    location_pattern = re.compile(
        r"if platform\.system\(\) == 'Windows':\r?\n"
        r"\s+_tagui_location = os\.environ\['APPDATA'\]\r?\n"
        r"else:\r?\n"
        r"\s+_tagui_location = os\.path\.expanduser\('~'\)"
    )
    location_replacement = (
        "if platform.system() == 'Windows':\n"
        "    _tagui_default_location = os.environ['APPDATA']\n"
        "else:\n"
        "    _tagui_default_location = os.path.expanduser('~')\n"
        "_tagui_location = os.environ.get('DRPA_RPA_HOME', _tagui_default_location)"
    )
    source, location_count = location_pattern.subn(location_replacement, source, count=1)
    setup_marker = "    if not os.path.isfile('rpa_python.zip'):"
    offline_setup = (
        "    drpa_offline_bundle = os.environ.get('DRPA_RPA_BUNDLE', '')\n"
        "    if os.path.isfile(drpa_offline_bundle):\n"
        "        import shutil\n"
        "        shutil.copyfile(drpa_offline_bundle, 'rpa_python.zip')\n"
        + setup_marker
    )
    if setup_marker not in source:
        raise RuntimeError("RPA for Python setup hook was not found")
    source = source.replace(setup_marker, offline_setup, 1)
    if location_count != 1:
        raise RuntimeError("RPA for Python location hook was not found")
    return source


def _record_digest(content: bytes) -> str:
    return base64.urlsafe_b64encode(hashlib.sha256(content).digest()).decode("ascii").rstrip("=")


def _zip_info(name: str, mode: int = 0o644) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, date_time=(2023, 7, 7, 0, 0, 0))
    info.create_system = 3
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = (stat.S_IFREG | mode) << 16
    return info


def _build_pure_module_wheel(
    module: Path,
    wheelhouse: Path,
    *,
    name: str,
    version: str,
    requires_dist: str | None = None,
) -> Path:
    normalized = name.replace("-", "_")
    wheel = wheelhouse / f"{normalized}-{version}-py3-none-any.whl"
    dist_info = f"{normalized}-{version}.dist-info"
    metadata_lines = [
        "Metadata-Version: 2.1",
        f"Name: {name}",
        f"Version: {version}",
        "Summary: RPA for Python is a Python package for robotic process automation",
        "Home-page: https://github.com/tebelorg/RPA-Python",
        "License: Apache License 2.0",
    ]
    if requires_dist:
        metadata_lines.append(f"Requires-Dist: {requires_dist}")
    files = {
        f"{normalized}.py": module.read_bytes(),
        f"{dist_info}/METADATA": ("\n".join(metadata_lines) + "\n").encode(),
        f"{dist_info}/WHEEL": (
            "Wheel-Version: 1.0\n"
            "Generator: DRPA sealed runtime builder\n"
            "Root-Is-Purelib: true\n"
            "Tag: py3-none-any\n"
        ).encode(),
        f"{dist_info}/top_level.txt": f"{normalized}\n".encode(),
    }
    record_path = f"{dist_info}/RECORD"
    record = [
        f"{path},sha256={_record_digest(content)},{len(content)}"
        for path, content in files.items()
    ]
    record.append(f"{record_path},,")
    files[record_path] = ("\n".join(record) + "\n").encode()
    with zipfile.ZipFile(wheel, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as output:
        for path, content in files.items():
            output.writestr(_zip_info(path), content)
    return wheel


def build_source_wheels(stage: Path, work_dir: Path, spec: dict[str, Any]) -> list[dict[str, Any]]:
    wheelhouse = stage / "wheelhouse"
    source_root = work_dir / "rpa-source-builds"
    if source_root.exists():
        shutil.rmtree(source_root)
    source_root.mkdir(parents=True)
    provenance: list[dict[str, Any]] = []
    for item in spec["sourceDistributions"]:
        archive = source_root / str(item["filename"])
        _download_verified(str(item["url"]), archive, str(item["sha256"]))
        extracted = source_root / f"extract-{item['name']}"
        _safe_extract_tar(archive, extracted)
        candidates = [path.parent for path in extracted.rglob("setup.py")]
        if len(candidates) != 1:
            raise RuntimeError(f"source archive has an ambiguous project root: {archive.name}")
        project = candidates[0]
        if str(item["name"]).lower() == "tagui":
            module = project / "tagui.py"
            module.write_text(patch_tagui_source(module.read_text(encoding="utf-8")), encoding="utf-8")
        module = project / f"{item['name']}.py"
        if not module.is_file():
            raise RuntimeError(f"source archive is missing {module.name}")
        wheel = _build_pure_module_wheel(
            module,
            wheelhouse,
            name=str(item["name"]),
            version=str(spec["version"]),
            requires_dist="tagui (>=1.50.0)" if item["name"] == "rpa" else None,
        )
        provenance.append(
            {
                "name": item["name"],
                "version": spec["version"],
                "sourceFilename": item["filename"],
                "sourceSha256": item["sha256"],
                "wheelFilename": wheel.name,
                "wheelSha256": sha256(wheel),
                "patch": "DRPA_RPA_HOME + DRPA_RPA_BUNDLE" if item["name"] == "tagui" else None,
            }
        )
    return provenance


def _overlay_engine_file(
    tagui_root: Path,
    work_dir: Path,
    commit: str,
    item: dict[str, Any],
) -> dict[str, Any]:
    source_name = str(item["source"])
    downloaded = work_dir / "rpa-delta" / source_name.replace("/", "__")
    _download_verified(
        f"https://raw.githubusercontent.com/tebelorg/Tump/{commit}/{source_name}",
        downloaded,
        str(item["sha256"]),
        int(item["bytes"]) if "bytes" in item else None,
    )
    target = tagui_root / str(item["target"])
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(downloaded, target)
    if item.get("executable"):
        target.chmod(target.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return {"source": source_name, "target": item["target"], "sha256": item["sha256"]}


def build_rpa_engine(stage: Path, work_dir: Path, platform_id: str, spec: dict[str, Any]) -> dict[str, Any] | None:
    asset = spec.get("engineAssets", {}).get(platform_id)
    if not asset:
        return None
    archive = work_dir / str(asset["filename"])
    _download_verified(str(asset["url"]), archive, str(asset["sha256"]), int(asset["bytes"]))
    extracted = work_dir / "rpa-engine"
    if extracted.exists():
        shutil.rmtree(extracted)
    _safe_extract_zip(archive, extracted)
    tagui_root = extracted / str(asset["taguiDirectory"])
    if not (tagui_root / "src" / "tagui").is_file():
        raise RuntimeError("TagUI engine archive does not contain src/tagui")

    commit = str(spec["deltaCommit"])
    overlays = [
        _overlay_engine_file(tagui_root, work_dir, commit, item)
        for item in spec["deltaFiles"]
    ]
    if asset.get("includeVcredist"):
        overlays.append(_overlay_engine_file(tagui_root, work_dir, commit, spec["vcredist"]))
    (tagui_root / f"rpa_python_{spec['version']}").write_text(
        "TagUI installation files used by RPA for Python\n",
        encoding="utf-8",
    )

    target = stage / "rpa" / "rpa_python.zip"
    target.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as output:
        for path in sorted(tagui_root.rglob("*")):
            if path.is_file():
                relative = path.relative_to(tagui_root).as_posix()
                mode = path.stat().st_mode & 0o777
                with path.open("rb") as source, output.open(_zip_info(relative, mode), "w") as destination:
                    shutil.copyfileobj(source, destination, length=1024 * 1024)
    lock = {
        "schema": 1,
        "rpaVersion": spec["version"],
        "platform": platform_id,
        "sourceAsset": {
            "filename": asset["filename"],
            "bytes": asset["bytes"],
            "sha256": asset["sha256"],
        },
        "deltaCommit": commit,
        "overlays": overlays,
        "bundle": {"filename": "rpa_python.zip", "bytes": target.stat().st_size, "sha256": sha256(target)},
    }
    (stage / "rpa" / "asset-lock.json").write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")
    return lock


__all__ = [
    "SPEC_PATH",
    "build_rpa_engine",
    "build_source_wheels",
    "load_rpa_spec",
    "patch_tagui_source",
    "source_build_names",
    "write_binary_requirements",
]
