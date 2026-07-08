from __future__ import annotations

from importlib import resources
from pathlib import Path

from .paths import get_data_dir


def default_package_path(filename: str) -> Path:
    cache_dir = get_data_dir() / "cache" / "default-packages"
    cache_dir.mkdir(parents=True, exist_ok=True)
    target = cache_dir / filename
    package_resource = resources.files("drpa_client.resources.default_packages").joinpath(filename)
    with package_resource.open("rb") as source, target.open("wb") as destination:
        destination.write(source.read())
    return target
