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
from packaging.utils import canonicalize_name, parse_wheel_filename
from packaging.version import Version

from .models import DependencySpec, PackageManifest
from .paths import get_project_venv_dir


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
        runtime_mode: str = "project_venv",
        existing_venv_path: str = "",
        install_dependencies: bool = True,
        log: Callable[[str], None] | None = None,
    ) -> tuple[Path, Path | None, bool]:
        venv_dir = get_project_venv_dir()
        _log(log, f"使用项目统一 venv：{venv_dir}")
        python = self.ensure_environment(
            package_dir=package_dir,
            manifest=manifest,
            venv_dir=venv_dir,
            install_dependencies=install_dependencies,
            log=log,
        )
        return python, venv_dir, False

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
            except Exception as exc:
                raise RuntimeErrorDetails(
                    "无法创建项目虚拟环境。请确认当前 Python 支持 venv/ensurepip。"
                    f"原始错误：{exc}"
                ) from exc

        python = self.python_executable(venv_dir)
        if needs_pip and not self._has_pip(python):
            _log(log, f"项目 venv 缺少 pip，重建虚拟环境：{venv_dir}")
            shutil.rmtree(venv_dir, ignore_errors=True)
            try:
                venv.EnvBuilder(with_pip=True, clear=True).create(venv_dir)
            except Exception as exc:
                raise RuntimeErrorDetails(
                    "无法创建带 pip 的项目虚拟环境。请确认当前 Python 支持 ensurepip。"
                    f"原始错误：{exc}"
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
        if not find_links and (dependencies.pip or self._requirements_path(package_dir, dependencies).exists()):
            raise RuntimeErrorDetails("未找到本地 wheelhouse，禁止联网安装依赖")
        requirements_path = package_dir / (dependencies.requirements or "requirements.txt")
        has_requirements = requirements_path.exists()

        wheel_args = [arg for link in find_links for arg in ("--find-links", link)]
        index_args = ["--no-index"]
        constraint_args = self._global_constraints_args(package_dir)
        _log(log, "离线安装模式：已禁用 pip 联网索引 (--no-index)")
        if find_links:
            _log(log, "wheelhouse 查找顺序：" + " -> ".join(find_links))

        if has_requirements:
            _log(log, f"安装 requirements：{requirements_path}")
            self._run_pip(
                python,
                ["install", *index_args, *wheel_args, *constraint_args, "-r", str(requirements_path)],
                package_dir,
                log=log,
            )

        if dependencies.pip:
            _log(log, f"安装 manifest 依赖：{', '.join(dependencies.pip)}")
            self._run_pip(
                python,
                ["install", *index_args, *wheel_args, *constraint_args, *dependencies.pip],
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

    def _has_pip(self, python: Path) -> bool:
        result = subprocess.run(
            [str(python), "-m", "pip", "--version"],
            text=True,
            capture_output=True,
            check=False,
        )
        return result.returncode == 0

    def _collect_find_links(self, package_dir: Path, dependencies: DependencySpec) -> list[str]:
        directories: list[Path] = []
        seen: set[Path] = set()
        for path in self._global_wheelhouse_dirs():
            if path not in seen:
                directories.append(path)
                seen.add(path)

        patterns = list(dependencies.local_common)
        system = platform.system().lower()
        if system == "windows":
            patterns.extend(dependencies.local_windows)
        elif system == "linux":
            patterns.extend(dependencies.local_linux)

        for pattern in patterns:
            matches = sorted(glob.glob(str(package_dir / pattern)))
            for match in matches:
                path = Path(match)
                directory = path if path.is_dir() else path.parent
                if directory not in seen:
                    directories.append(directory)
                    seen.add(directory)
        return [str(path) for path in directories]

    def _global_wheelhouse_dirs(self) -> list[Path]:
        system = platform.system().lower()
        platform_dir = "windows-amd64" if system == "windows" else "linux-x86_64"
        directories: list[Path] = []
        seen: set[Path] = set()
        for root in _candidate_install_roots(Path(__file__).resolve()):
            for path in (root / "wheelhouse" / "common", root / "wheelhouse" / platform_dir):
                if path.exists() and path not in seen:
                    directories.append(path)
                    seen.add(path)
        return directories

    def _requires_pip(self, package_dir: Path, dependencies: DependencySpec) -> bool:
        return bool(dependencies.pip) or self._requirements_path(package_dir, dependencies).exists()

    def _requirements_path(self, package_dir: Path, dependencies: DependencySpec) -> Path:
        return package_dir / (dependencies.requirements or "requirements.txt")

    def _global_constraints_args(self, package_dir: Path) -> list[str]:
        constraints = _global_wheel_constraints(self._global_wheelhouse_dirs())
        if not constraints:
            return []
        path = package_dir / ".drpa-wheelhouse-constraints.txt"
        path.write_text("\n".join(constraints) + "\n", encoding="utf-8")
        return ["-c", str(path)]

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


def _candidate_install_roots(start: Path) -> list[Path]:
    candidates: list[Path] = []
    raw_candidates = [
        Path.cwd(),
        Path(sys.executable).resolve().parent,
        Path(sys.argv[0]).resolve().parent if sys.argv and sys.argv[0] else None,
        start,
        *start.parents,
    ]
    seen: set[Path] = set()
    for item in raw_candidates:
        if item is None:
            continue
        path = item.resolve()
        if path.is_file():
            path = path.parent
        if path not in seen:
            candidates.append(path)
            seen.add(path)
    return candidates


def _global_wheel_constraints(wheel_dirs: list[Path]) -> list[str]:
    versions: dict[str, Version] = {}
    names: dict[str, str] = {}
    for directory in wheel_dirs:
        for wheel in sorted(directory.glob("*.whl")):
            try:
                name, version, _, _ = parse_wheel_filename(wheel.name)
            except Exception:
                continue
            normalized = canonicalize_name(str(name))
            if normalized not in versions or version > versions[normalized]:
                versions[normalized] = version
                names[normalized] = str(name)
    return [f"{names[key]}=={versions[key]}" for key in sorted(versions)]
