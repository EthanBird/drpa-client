from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


class BootstrapError(RuntimeError):
    pass


def bundle_root() -> Path:
    return Path(__file__).resolve().parent


def find_bundled_python(root: Path) -> Path:
    candidates = (
        list((root / "python").glob("*/python.exe"))
        + list((root / "python").glob("*/bin/python3.11"))
        + list((root / "python").glob("*/bin/python3"))
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise BootstrapError("bundled CPython executable is missing")


def environment_python(environment: Path) -> Path:
    return environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def prepare(root: Path | None = None, environment_override: Path | None = None) -> Path:
    root = (root or bundle_root()).resolve()
    manifest = root / "manifest.json"
    requirements = root / "locks" / "runtime.txt"
    wheelhouse = root / "wheelhouse"
    uv = root / "tools" / ("uv.exe" if os.name == "nt" else "uv")
    for required in (manifest, requirements, wheelhouse, uv):
        if not required.exists():
            raise BootstrapError(f"offline bundle is incomplete: {required.relative_to(root)}")

    environment = (environment_override or (root / "environment")).resolve()
    environment.parent.mkdir(parents=True, exist_ok=True)
    marker = environment / ".drpa-runtime.json"
    expected = {
        "manifestSha256": digest_file(manifest),
        "requirementsSha256": digest_file(requirements),
    }
    if marker.exists() and environment_python(environment).exists():
        try:
            if json.loads(marker.read_text(encoding="utf-8")) == expected:
                return environment_python(environment)
        except (OSError, json.JSONDecodeError):
            pass

    if environment.exists():
        shutil.rmtree(environment)

    python = find_bundled_python(root)
    offline_env = os.environ.copy()
    offline_env.update(
        {
            "PIP_NO_INDEX": "1",
            "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            "UV_OFFLINE": "1",
            "UV_NO_MANAGED_PYTHON": "1",
            "UV_PYTHON_DOWNLOADS": "never",
            "UV_CACHE_DIR": str(environment.parent / ".drpa-uv-cache"),
        }
    )
    subprocess.run(
        [str(uv), "venv", str(environment), "--python", str(python), "--no-project"],
        cwd=root,
        env=offline_env,
        check=True,
    )
    subprocess.run(
        [
            str(uv),
            "pip",
            "install",
            "--python",
            str(environment_python(environment)),
            "--offline",
            "--no-index",
            "--find-links",
            str(wheelhouse),
            "--requirement",
            str(requirements),
            "drpa-runtime-python==0.2.0",
        ],
        cwd=root,
        env=offline_env,
        check=True,
    )
    subprocess.run(
        [str(uv), "pip", "check", "--python", str(environment_python(environment))],
        cwd=root,
        env=offline_env,
        check=True,
    )
    marker.write_text(json.dumps(expected, indent=2) + "\n", encoding="utf-8")
    return environment_python(environment)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--environment",
        type=Path,
        help="Writable destination for the generated virtual environment",
    )
    args = parser.parse_args()
    try:
        python = prepare(environment_override=args.environment)
    except (BootstrapError, OSError, subprocess.CalledProcessError) as error:
        print(f"[DRPA offline] runtime preparation failed: {error}", file=sys.stderr)
        return 1
    print(python)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
