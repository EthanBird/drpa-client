from __future__ import annotations

import glob
import json
import os
import platform
import shutil
import subprocess
import sys
import venv
from collections.abc import Callable
from pathlib import Path

from packaging.specifiers import SpecifierSet
from packaging.version import Version

from .models import DependencySpec, PackageManifest
from .settings import RuntimeMode


class RuntimeErrorDetails(RuntimeError):
    """Raised when a script runtime cannot be prepared."""


class RuntimeManager:
    def __init__(self, base_python: Path | None = None):
        self.base_python = base_python or Path(sys.executable)

    def prepare_environment(
        self,
        package_dir: Path,
        manifest: PackageManifest,
        package_root_dir: Path,
        runtime_mode: RuntimeMode = "new_venv",
        existing_venv_path: str = "",
        install_dependencies: bool = True,
        log: Callable[[str], None] | None = None,
    ) -> tuple[Path, Path | None, bool]:
        if manifest.runtime.isolation != "venv" or runtime_mode == "shared":
            python = self.base_python
            _log(log, f"使用共享 Python 环境：{python}")
            self._validate_python_version(manifest.runtime.python)
            if install_dependencies:
                self.install_dependencies(python, package_dir, manifest.dependencies, log=log)
            return python, None, False

        if runtime_mode == "existing_venv":
            if not existing_venv_path:
                raise RuntimeErrorDetails("已选择使用已有 venv，但未配置 venv 路径")
            venv_dir = Path(existing_venv_path).expanduser().resolve()
            python = self.python_executable(venv_dir)
            if not python.exists():
                raise RuntimeErrorDetails(f"已有 venv 中找不到 Python：{python}")
            _log(log, f"使用已有 venv：{venv_dir}")
            self._validate_python_executable_version(python, manifest.runtime.python)
            if install_dependencies:
                self.install_dependencies(python, package_dir, manifest.dependencies, log=log)
            return python, venv_dir, False

        venv_dir = package_root_dir / "venv"
        python = self.ensure_environment(
            package_dir=package_dir,
            manifest=manifest,
            venv_dir=venv_dir,
            install_dependencies=install_dependencies,
            log=log,
        )
        return python, venv_dir, True

    def ensure_environment(
        self,
        package_dir: Path,
        manifest: PackageManifest,
        venv_dir: Path,
        install_dependencies: bool = True,
        log: Callable[[str], None] | None = None,
    ) -> Path:
        self._validate_python_version(manifest.runtime.python)
        needs_pip = install_dependencies and self._requires_pip(package_dir, manifest.dependencies)
        if not venv_dir.exists():
            try:
                _log(log, f"创建虚拟环境：{venv_dir}")
                venv.EnvBuilder(with_pip=needs_pip, clear=False).create(venv_dir)
            except venv.Error as exc:
                raise RuntimeErrorDetails(
                    "无法创建脚本包虚拟环境。Linux 系统请确认安装包内置 Python 支持 venv，"
                    "或系统已安装 python3-venv/ensurepip。"
                ) from exc

        python = self.python_executable(venv_dir)
        if needs_pip:
            self.install_dependencies(python, package_dir, manifest.dependencies, log=log)
        elif install_dependencies:
            _log(log, "脚本包没有声明额外依赖，跳过 pip 安装")
        else:
            _log(log, "已按用户选择跳过依赖安装")
        return python

    def install_dependencies(
        self,
        python: Path,
        package_dir: Path,
        dependencies: DependencySpec,
        log: Callable[[str], None] | None = None,
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
            _log(log, f"安装 requirements：{requirements_path}")
            self._run_pip(
                python,
                ["install", *index_args, *wheel_args, "-r", str(requirements_path)],
                package_dir,
                log=log,
            )

        if dependencies.pip:
            _log(log, f"安装 manifest 依赖：{', '.join(dependencies.pip)}")
            self._run_pip(
                python,
                ["install", *index_args, *wheel_args, *dependencies.pip],
                package_dir,
                log=log,
            )

    def python_executable(self, venv_dir: Path | None) -> Path:
        if venv_dir is None:
            return self.base_python
        if platform.system().lower() == "windows":
            return venv_dir / "Scripts" / "python.exe"
        return venv_dir / "bin" / "python"

    def detect_python_environments(self) -> list[dict[str, str]]:
        candidates: list[Path] = [self.base_python]
        env_venv = os.getenv("VIRTUAL_ENV")
        if env_venv:
            candidates.append(self.python_executable(Path(env_venv)))
        for command in ("python3.11", "python3", "python", "python3.12"):
            found = shutil.which(command)
            if found:
                candidates.append(Path(found))

        seen: set[str] = set()
        results: list[dict[str, str]] = []
        for candidate in candidates:
            key = str(candidate)
            if key in seen or not candidate.exists():
                continue
            seen.add(key)
            info = self._inspect_python(candidate)
            if info:
                results.append(info)
        return results

    def _validate_python_version(self, spec: str) -> None:
        try:
            specifier = SpecifierSet(spec)
        except Exception as exc:  # noqa: BLE001 - packaging raises several subclasses
            raise RuntimeErrorDetails(f"Python 版本约束无效：{spec}") from exc
        current = Version(f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")
        if current not in specifier:
            raise RuntimeErrorDetails(f"当前 Python {current} 不满足脚本包要求：{spec}")

    def _validate_python_executable_version(self, python: Path, spec: str) -> None:
        info = self._inspect_python(python)
        if not info:
            raise RuntimeErrorDetails(f"无法检测 Python 版本：{python}")
        try:
            specifier = SpecifierSet(spec)
        except Exception as exc:  # noqa: BLE001
            raise RuntimeErrorDetails(f"Python 版本约束无效：{spec}") from exc
        version = Version(info["version"])
        if version not in specifier:
            raise RuntimeErrorDetails(f"{python} 的版本 {version} 不满足脚本包要求：{spec}")

    def _inspect_python(self, python: Path) -> dict[str, str] | None:
        script = (
            "import json,sys,sysconfig;"
            "print(json.dumps({'executable':sys.executable,"
            "'version':'.'.join(map(str,sys.version_info[:3])),"
            "'prefix':sys.prefix,'base_prefix':getattr(sys,'base_prefix',sys.prefix),"
            "'is_venv':sys.prefix!=getattr(sys,'base_prefix',sys.prefix)}))"
        )
        try:
            result = subprocess.run(
                [str(python), "-c", script],
                text=True,
                capture_output=True,
                timeout=5,
                check=False,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        if result.returncode != 0:
            return None
        try:
            raw = json.loads(result.stdout)
        except json.JSONDecodeError:
            return None
        return {key: str(value) for key, value in raw.items()}

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

    def _requires_pip(self, package_dir: Path, dependencies: DependencySpec) -> bool:
        requirements_path = package_dir / (dependencies.requirements or "requirements.txt")
        return bool(dependencies.pip) or requirements_path.exists()

    def _run_pip(
        self,
        python: Path,
        args: list[str],
        cwd: Path,
        log: Callable[[str], None] | None = None,
    ) -> None:
        env = os.environ.copy()
        env.setdefault("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        command = [str(python), "-m", "pip", *args]
        _log(log, f"执行：{' '.join(command)}")
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        output: list[str] = []
        assert process.stdout is not None
        for line in process.stdout:
            text = line.rstrip()
            output.append(text)
            _log(log, text)
        exit_code = process.wait()
        if exit_code != 0:
            raise RuntimeErrorDetails(
                "依赖安装失败：\n"
                f"命令：{shutil.which(str(python)) or python} -m pip {' '.join(args)}\n"
                f"输出：\n{chr(10).join(output)}"
            )


def _log(callback: Callable[[str], None] | None, message: str) -> None:
    if callback is not None:
        callback(message)
