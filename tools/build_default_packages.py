from __future__ import annotations

import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PACKAGES = {
    "bing_daily_image": ROOT / "src" / "drpa_client" / "resources" / "default_packages" / "bing_daily_image.rpaz",
}


def main() -> int:
    for source_name, target in DEFAULT_PACKAGES.items():
        source = ROOT / "examples" / source_name
        if not source.exists():
            raise SystemExit(f"missing package source: {source}")
        target.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(source.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(source))
        print(f"built {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
