from __future__ import annotations

import glob
import json
import os
import platform
import subprocess
from collections.abc import Callable
from pathlib import Path

from packaging.specifiers import SpecifierSet
from packaging.utils import canonicalize_name, parse_wheel_filename
from packaging.version import Version

from .models import DependencySpec, PackageManifest
from .paths import (
    get_project_python,
    get_project_root,
    get_project_venv_dir,
    require_project_python,
    resolve_uv_executable,
)


class RuntimeErrorDetails(RuntimeError):
    """Raised when a script runtime cannot be prepared."""


class RuntimeManager:
    def __init__(self, base_python: Path | None = None):
        self.base_python = base_python or get_project_python()

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
            raise RuntimeErrorDetails(
                f"项目运行环境不存在：{venv_dir}\n"
                "请运行 scripts/run-drpa-windows.bat 或 scripts/run-drpa.sh，"
                "或在项目目录执行 uv sync。"
            )

        python = self.python_executable(venv_dir)
        if needs_pip:
            try:
                resolve_uv_executable()
            except RuntimeError as exc:
                raise RuntimeErrorDetails(str(exc)) from exc
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
            raise RuntimeErrorDetails("脚本包必须使用项目统一 .venv 运行")
        expected = get_project_venv_dir()
        if venv_dir.resolve() != expected.resolve():
            raise RuntimeErrorDetails(f"仅支持项目统一 venv：{expected}")
        python = self._venv_python_path(venv_dir)
        if not python.exists():
            raise RuntimeErrorDetails(
                f"项目 Python 不存在：{python}\n"
                "请运行 scripts/run-drpa-windows.bat 或 scripts/run-drpa.sh，"
                "或在项目目录执行 uv sync。"
            )
        return python

    def _venv_python_path(self, venv_dir: Path) -> Path:
        if platform.system().lower() == "windows":
            return venv_dir / "Scripts" / "python.exe"
        return venv_dir / "bin" / "python"

    def _validate_python_version(self, spec: str) -> None:
        python = require_project_python()
        self._validate_python_executable_version(python, spec)

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
                encoding="utf-8",
                errors="replace",
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
        env.setdefault("UV_NO_PROGRESS", "1")
        env.setdefault("PYTHONIOENCODING", "utf-8")
        env.setdefault("PYTHONUTF8", "1")
        command = _dependency_install_command(python, args)
        _log(log, f"执行：{' '.join(command)}")
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
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
                f"命令：{' '.join(command)}\n"
                f"输出：\n{chr(10).join(output)}"
            )


def _log(callback: Callable[[str], None] | None, message: str) -> None:
    if callback is not None:
        callback(message)


def _dependency_install_command(python: Path, args: list[str]) -> list[str]:
    if not args or args[0] != "install":
        raise RuntimeErrorDetails("依赖安装仅支持 pip install，且必须通过 uv 执行")
    try:
        uv = resolve_uv_executable()
    except RuntimeError as exc:
        raise RuntimeErrorDetails(str(exc)) from exc
    return [uv, "pip", "install", "--python", str(python), *args[1:]]


def _candidate_install_roots(_start: Path) -> list[Path]:
    return [get_project_root().resolve()]


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
