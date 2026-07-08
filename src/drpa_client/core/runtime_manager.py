from __future__ import annotations

import glob
import os
import platform
import shutil
import subprocess
import sys
import venv
from pathlib import Path

from packaging.specifiers import SpecifierSet
from packaging.version import Version

from .models import DependencySpec, PackageManifest


class RuntimeErrorDetails(RuntimeError):
    """Raised when a script runtime cannot be prepared."""


class RuntimeManager:
    def __init__(self, base_python: Path | None = None):
        self.base_python = base_python or Path(sys.executable)

    def ensure_environment(
        self,
        package_dir: Path,
        manifest: PackageManifest,
        venv_dir: Path,
        install_dependencies: bool = True,
    ) -> Path:
        self._validate_python_version(manifest.runtime.python)
        if not venv_dir.exists():
            venv.EnvBuilder(with_pip=True, clear=False).create(venv_dir)

        python = self.python_executable(venv_dir)
        if install_dependencies:
            self.install_dependencies(python, package_dir, manifest.dependencies)
        return python

    def install_dependencies(
        self,
        python: Path,
        package_dir: Path,
        dependencies: DependencySpec,
    ) -> None:
        find_links = self._collect_find_links(package_dir, dependencies)
        requirements_path = package_dir / (dependencies.requirements or "requirements.txt")
        has_requirements = requirements_path.exists()

        wheel_args = [arg for link in find_links for arg in ("--find-links", link)]

        if dependencies.strategy == "offline-only":
            index_args = ["--no-index"]
        else:
            index_args = []

        if has_requirements:
            self._run_pip(
                python,
                ["install", *index_args, *wheel_args, "-r", str(requirements_path)],
                package_dir,
            )

        if dependencies.pip:
            self._run_pip(
                python,
                ["install", *index_args, *wheel_args, *dependencies.pip],
                package_dir,
            )

    def python_executable(self, venv_dir: Path | None) -> Path:
        if venv_dir is None:
            return self.base_python
        if platform.system().lower() == "windows":
            return venv_dir / "Scripts" / "python.exe"
        return venv_dir / "bin" / "python"

    def _validate_python_version(self, spec: str) -> None:
        try:
            specifier = SpecifierSet(spec)
        except Exception as exc:  # noqa: BLE001 - packaging raises several subclasses
            raise RuntimeErrorDetails(f"Python 版本约束无效：{spec}") from exc
        current = Version(f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")
        if current not in specifier:
            raise RuntimeErrorDetails(f"当前 Python {current} 不满足脚本包要求：{spec}")

    def _collect_find_links(self, package_dir: Path, dependencies: DependencySpec) -> list[str]:
        patterns = list(dependencies.local_common)
        system = platform.system().lower()
        if system == "windows":
            patterns.extend(dependencies.local_windows)
        elif system == "linux":
            patterns.extend(dependencies.local_linux)

        directories: set[str] = set()
        for pattern in patterns:
            matches = glob.glob(str(package_dir / pattern))
            for match in matches:
                path = Path(match)
                directories.add(str(path if path.is_dir() else path.parent))
        return sorted(directories)

    def _run_pip(self, python: Path, args: list[str], cwd: Path) -> None:
        env = os.environ.copy()
        env.setdefault("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        result = subprocess.run(
            [str(python), "-m", "pip", *args],
            cwd=cwd,
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeErrorDetails(
                "依赖安装失败：\n"
                f"命令：{shutil.which(str(python)) or python} -m pip {' '.join(args)}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
