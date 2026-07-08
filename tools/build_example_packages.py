from __future__ import annotations

import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXAMPLE_PACKAGES = ("hello_web_bot", "bing_daily_image", "bilibili_search")


def main() -> int:
    for package_id in EXAMPLE_PACKAGES:
        source = ROOT / "examples" / package_id
        if not source.exists():
            continue
        target = ROOT / "examples" / f"{package_id}.rpaz"
        if target.exists():
            target.unlink()
        with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(source.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(source))
        print(f"built {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
