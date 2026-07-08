from __future__ import annotations

import json
import shutil
import zipfile
from collections.abc import Callable
from dataclasses import asdict
from datetime import UTC, datetime
from pathlib import Path

from .manifest import ManifestError, load_manifest
from .models import InstalledPackage
from .paths import ensure_data_layout
from .runtime_manager import RuntimeManager
from .settings import SettingsStore


class PackageInstallError(RuntimeError):
    """Raised when a package cannot be installed."""


class PackageManager:
    def __init__(
        self,
        data_dir: Path | None = None,
        runtime_manager: RuntimeManager | None = None,
        settings_store: SettingsStore | None = None,
    ):
        self.data_dir = ensure_data_layout(data_dir)
        self.packages_dir = self.data_dir / "packages"
        self.runtime_manager = runtime_manager or RuntimeManager()
        self.settings_store = settings_store or SettingsStore(self.data_dir)

    def install_archive(
        self,
        archive_path: Path,
        install_dependencies: bool = True,
        log: Callable[[str], None] | None = None,
    ) -> InstalledPackage:
        if not archive_path.exists():
            raise PackageInstallError(f"脚本包不存在：{archive_path}")
        if archive_path.suffix.lower() not in {".rpaz", ".zip"}:
            raise PackageInstallError("脚本包格式必须是 .rpaz 或 .zip")

        staging_dir = self.data_dir / "cache" / f"install-{datetime.now(UTC).timestamp():.0f}"
        staging_dir.mkdir(parents=True, exist_ok=False)
        try:
            _log(log, f"解压脚本包：{archive_path}")
            with zipfile.ZipFile(archive_path) as archive:
                _safe_extract(archive, staging_dir)
            return self._install_staging(staging_dir, install_dependencies=install_dependencies, log=log)
        except (zipfile.BadZipFile, ManifestError, OSError) as exc:
            raise PackageInstallError(str(exc)) from exc
        finally:
            if staging_dir.exists():
                shutil.rmtree(staging_dir, ignore_errors=True)

    def install_python_file(
        self,
        file_path: Path,
        install_dependencies: bool = False,
        log: Callable[[str], None] | None = None,
    ) -> InstalledPackage:
        if not file_path.exists():
            raise PackageInstallError(f"Python 文件不存在：{file_path}")
        if file_path.suffix.lower() != ".py":
            raise PackageInstallError("单文件导入只支持 .py 文件")

        package_id = _slug(file_path.stem)
        staging_dir = self.data_dir / "cache" / f"single-file-{datetime.now(UTC).timestamp():.0f}"
        staging_dir.mkdir(parents=True, exist_ok=False)
        try:
            _log(log, f"导入单文件脚本：{file_path}")
            shutil.copy2(file_path, staging_dir / "main.py")
            (staging_dir / "manifest.yaml").write_text(
                _single_file_manifest(package_id, file_path.stem),
                encoding="utf-8",
            )
            return self._install_staging(staging_dir, install_dependencies=install_dependencies, log=log)
        except (ManifestError, OSError) as exc:
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

    def uninstall(self, package: InstalledPackage) -> None:
        if package.root_dir.exists():
            shutil.rmtree(package.root_dir)

    def rebuild_environment(
        self,
        package: InstalledPackage,
        install_dependencies: bool = True,
        log: Callable[[str], None] | None = None,
    ) -> InstalledPackage:
        if package.manifest.runtime.isolation != "venv" or package.venv_dir is None:
            _log(log, "当前脚本包使用 shared runtime，无需重建 venv")
            return package
        if not package.venv_owned:
            _log(log, f"当前脚本包使用已有 venv，不会删除外部环境：{package.venv_dir}")
            self.runtime_manager.install_dependencies(
                self.runtime_manager.python_executable(package.venv_dir),
                package.package_dir,
                package.manifest.dependencies,
                log=log,
            )
            return package
        if package.venv_dir.exists():
            _log(log, f"删除旧虚拟环境：{package.venv_dir}")
            shutil.rmtree(package.venv_dir)
        self.runtime_manager.ensure_environment(
            package_dir=package.package_dir,
            manifest=package.manifest,
            venv_dir=package.venv_dir,
            install_dependencies=install_dependencies,
            log=log,
        )
        _log(log, "虚拟环境重建完成")
        return package

    def _write_install_lock(self, installed: InstalledPackage) -> None:
        payload = {
            "manifest": asdict(installed.manifest),
            "root_dir": str(installed.root_dir),
            "package_dir": str(installed.package_dir),
            "venv_dir": str(installed.venv_dir) if installed.venv_dir else None,
            "runtime_mode": installed.runtime_mode,
            "venv_owned": installed.venv_owned,
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
            runtime_mode=payload.get("runtime_mode", "new_venv"),
            venv_owned=bool(payload.get("venv_owned", True)),
            installed_at=payload["installed_at"],
        )

    def _install_staging(
        self,
        staging_dir: Path,
        install_dependencies: bool,
        log: Callable[[str], None] | None = None,
    ) -> InstalledPackage:
        _log(log, "读取 manifest.yaml")
        manifest = load_manifest(staging_dir)
        settings = self.settings_store.load()

        package_root = self.packages_dir / manifest.id
        package_dir = package_root / manifest.version / "package"
        if package_dir.exists():
            _log(log, f"覆盖已安装版本：{manifest.id} {manifest.version}")
            shutil.rmtree(package_dir.parent)
        package_dir.parent.mkdir(parents=True, exist_ok=True)
        _log(log, f"复制脚本包到：{package_dir}")
        shutil.move(str(staging_dir), str(package_dir))

        _, venv_dir, venv_owned = self.runtime_manager.prepare_environment(
            package_dir=package_dir,
            manifest=manifest,
            package_root_dir=package_dir.parent,
            runtime_mode=settings.runtime_mode,
            existing_venv_path=settings.existing_venv_path,
            install_dependencies=install_dependencies,
            log=log,
        )

        installed = InstalledPackage(
            manifest=manifest,
            root_dir=package_dir.parent,
            package_dir=package_dir,
            venv_dir=venv_dir,
            runtime_mode=settings.runtime_mode,
            venv_owned=venv_owned,
            installed_at=datetime.now(UTC).isoformat(),
        )
        self._write_install_lock(installed)
        _log(log, "写入 install.lock")
        return installed


def _safe_extract(archive: zipfile.ZipFile, target_dir: Path) -> None:
    target_root = target_dir.resolve()
    for member in archive.infolist():
        destination = (target_root / member.filename).resolve()
        if target_root != destination and target_root not in destination.parents:
            raise PackageInstallError(f"脚本包包含非法路径：{member.filename}")
    archive.extractall(target_root)


def _log(callback: Callable[[str], None] | None, message: str) -> None:
    if callback is not None:
        callback(message)


def _slug(value: str) -> str:
    safe = "".join(ch.lower() if ch.isalnum() else "_" for ch in value).strip("_")
    return safe or "single_file_script"


def _single_file_manifest(package_id: str, name: str) -> str:
    return f"""id: {package_id}
name: {name}
version: 0.1.0
entry: main.py
description: 由单个 Python 文件直接导入生成的脚本包。
author: single-file-import

runtime:
  python: ">=3.11"
  isolation: venv

dependencies:
  strategy: offline-first
  pip: []
  local:
    common:
      - wheels/common/*.whl
    windows:
      - wheels/windows/*.whl
    linux:
      - wheels/linux/*.whl

params: []
"""
