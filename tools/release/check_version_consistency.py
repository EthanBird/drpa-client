from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def json_value(relative: str, key: str) -> str:
    value = json.loads((ROOT / relative).read_text(encoding="utf-8"))[key]
    return str(value)


def match_value(relative: str, pattern: str) -> str:
    source = (ROOT / relative).read_text(encoding="utf-8")
    match = re.search(pattern, source, re.MULTILINE)
    if not match:
        raise ValueError(f"version pattern missing in {relative}")
    return match.group(1)


def collect_versions() -> dict[str, str]:
    package_lock = json.loads((ROOT / "package-lock.json").read_text(encoding="utf-8"))
    return {
        "Cargo workspace": match_value(
            "Cargo.toml", r'(?ms)^\[workspace\.package\].*?^version = "([^"]+)"'
        ),
        "npm root": json_value("package.json", "version"),
        "npm desktop": json_value("apps/desktop/package.json", "version"),
        "npm lock root": str(package_lock["packages"][""]["version"]),
        "npm lock desktop": str(package_lock["packages"]["apps/desktop"]["version"]),
        "Tauri": json_value("apps/desktop/src-tauri/tauri.conf.json", "version"),
        "NSIS": match_value("installer/windows/drpa-next.nsi", r'^!define APP_VERSION "([^"]+)"'),
        "Python runtime": match_value(
            "runtime/python/pyproject.toml", r'(?ms)^\[project\].*?^version = "([^"]+)"'
        ),
        "Python runtime lock": match_value(
            "runtime/python/uv.lock",
            r'(?ms)^\[\[package\]\]\s+name = "drpa-runtime-python"\s+version = "([^"]+)"',
        ),
        "runtime bundle": match_value("offline/runtime-spec.json", r'"bundleVersion": "([^-]+)-dev"'),
        "runtime bootstrap": match_value(
            "offline/bootstrap/bootstrap_runtime.py", r'"drpa-runtime-python==([^"]+)"'
        ),
        "Bing example": match_value("examples/bing_daily_image/manifest.yaml", r'^version:\s*(\S+)'),
        "desktop release": match_value(
            ".github/workflows/desktop-release.yml", r'^\s+DRPA_VERSION:\s*([^\s]+)'
        ),
        "offline release": match_value(
            ".github/workflows/offline-runtime.yml", r'offline-runtime-v([^-]+)-dev-'
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--expected", required=True)
    args = parser.parse_args()
    versions = collect_versions()
    mismatches = {name: value for name, value in versions.items() if value != args.expected}
    if mismatches:
        for name, value in mismatches.items():
            print(f"VERSION_MISMATCH {name}: expected {args.expected}, got {value}")
        return 1
    print(f"VERSION_CONSISTENCY_OK {args.expected} ({len(versions)} sources)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
