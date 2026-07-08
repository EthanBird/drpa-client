from __future__ import annotations

import json
import shutil
import zipfile
from dataclasses import asdict
from datetime import UTC, datetime
from pathlib import Path

from .manifest import ManifestError, load_manifest
from .models import InstalledPackage
from .paths import ensure_data_layout
from .runtime_manager import RuntimeManager


class PackageInstallError(RuntimeError):
    """Raised when a package cannot be installed."""


class PackageManager:
    def __init__(self, data_dir: Path | None = None, runtime_manager: RuntimeManager | None = None):
        self.data_dir = ensure_data_layout(data_dir)
        self.packages_dir = self.data_dir / "packages"
        self.runtime_manager = runtime_manager or RuntimeManager()

    def install_archive(self, archive_path: Path, install_dependencies: bool = True) -> InstalledPackage:
        if not archive_path.exists():
            raise PackageInstallError(f"脚本包不存在：{archive_path}")
        if archive_path.suffix.lower() not in {".rpaz", ".zip"}:
            raise PackageInstallError("脚本包格式必须是 .rpaz 或 .zip")

        staging_dir = self.data_dir / "cache" / f"install-{datetime.now(UTC).timestamp():.0f}"
        staging_dir.mkdir(parents=True, exist_ok=False)
        try:
            with zipfile.ZipFile(archive_path) as archive:
                _safe_extract(archive, staging_dir)
            manifest = load_manifest(staging_dir)

            package_root = self.packages_dir / manifest.id
            package_dir = package_root / manifest.version / "package"
            if package_dir.exists():
                shutil.rmtree(package_dir.parent)
            package_dir.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(staging_dir), str(package_dir))

            venv_dir = None
            if manifest.runtime.isolation == "venv":
                venv_dir = package_dir.parent / "venv"
                self.runtime_manager.ensure_environment(
                    package_dir=package_dir,
                    manifest=manifest,
                    venv_dir=venv_dir,
                    install_dependencies=install_dependencies,
                )

            installed = InstalledPackage(
                manifest=manifest,
                root_dir=package_dir.parent,
                package_dir=package_dir,
                venv_dir=venv_dir,
                installed_at=datetime.now(UTC).isoformat(),
            )
            self._write_install_lock(installed)
            return installed
        except (zipfile.BadZipFile, ManifestError, OSError) as exc:
            raise PackageInstallError(str(exc)) from exc
        finally:
            if staging_dir.exists():
                shutil.rmtree(staging_dir, ignore_errors=True)

    def list_installed(self) -> list[InstalledPackage]:
        installed: list[InstalledPackage] = []
        if not self.packages_dir.exists():
            return installed

        for lock_path in sorted(self.packages_dir.glob("*/*/install.lock")):
            try:
                installed.append(self._read_install_lock(lock_path))
            except (OSError, KeyError, ManifestError, json.JSONDecodeError):
                continue
        return installed

    def find(self, package_id: str, version: str | None = None) -> InstalledPackage | None:
        candidates = [pkg for pkg in self.list_installed() if pkg.manifest.id == package_id]
        if version is not None:
            candidates = [pkg for pkg in candidates if pkg.manifest.version == version]
        if not candidates:
            return None
        return sorted(candidates, key=lambda item: item.installed_at)[-1]

    def _write_install_lock(self, installed: InstalledPackage) -> None:
        payload = {
            "manifest": asdict(installed.manifest),
            "root_dir": str(installed.root_dir),
            "package_dir": str(installed.package_dir),
            "venv_dir": str(installed.venv_dir) if installed.venv_dir else None,
            "installed_at": installed.installed_at,
        }
        with (installed.root_dir / "install.lock").open("w", encoding="utf-8") as fp:
            json.dump(payload, fp, ensure_ascii=False, indent=2)

    def _read_install_lock(self, lock_path: Path) -> InstalledPackage:
        with lock_path.open("r", encoding="utf-8") as fp:
            payload = json.load(fp)
        manifest = load_manifest(Path(payload["package_dir"]))
        venv_dir = Path(payload["venv_dir"]) if payload.get("venv_dir") else None
        return InstalledPackage(
            manifest=manifest,
            root_dir=Path(payload["root_dir"]),
            package_dir=Path(payload["package_dir"]),
            venv_dir=venv_dir,
            installed_at=payload["installed_at"],
        )


def _safe_extract(archive: zipfile.ZipFile, target_dir: Path) -> None:
    target_root = target_dir.resolve()
    for member in archive.infolist():
        destination = (target_root / member.filename).resolve()
        if target_root != destination and target_root not in destination.parents:
            raise PackageInstallError(f"脚本包包含非法路径：{member.filename}")
    archive.extractall(target_root)
