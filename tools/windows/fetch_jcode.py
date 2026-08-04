from __future__ import annotations

import argparse
import hashlib
import shutil
import tarfile
import tempfile
import time
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
VERSION = "0.67.1"
ARCHIVE_NAME = "jcode-windows-x86_64.tar.gz"
ARCHIVE_MEMBER = "jcode-windows-x86_64.exe"
ARCHIVE_SHA256 = "84a6537225acf7caed1de40c8a16b7258a9a62b6f25703150c50eb9bc5404b92"
DOWNLOAD_URL = f"https://github.com/1jehuang/jcode/releases/download/v{VERSION}/{ARCHIVE_NAME}"
LICENSE = ROOT / "installer" / "third-party" / "jcode-LICENSE.txt"
OUTPUT_FILES = {"jcode.exe", "LICENSE.txt", "VERSION.txt"}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def download(url: str, destination: Path) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "DRPA-Next-release-builder/2.0"})
    with urllib.request.urlopen(request, timeout=120) as response, destination.open("wb") as output:
        shutil.copyfileobj(response, output)


def download_verified(url: str, destination: Path, attempts: int = 3) -> None:
    last_error: Exception | None = None
    for attempt in range(1, attempts + 1):
        try:
            download(url, destination)
            actual = digest(destination)
            if actual.lower() == ARCHIVE_SHA256.lower():
                return
            last_error = ValueError(f"JCode archive checksum mismatch: {actual}")
        except Exception as error:  # noqa: BLE001 - release download is retried as one operation
            last_error = error
        destination.unlink(missing_ok=True)
        if attempt < attempts:
            time.sleep(2 ** (attempt - 1))
    raise RuntimeError(f"Unable to download pinned JCode after {attempts} attempts: {last_error}")


def install(archive: Path, output_dir: Path, expected_sha256: str = ARCHIVE_SHA256) -> Path:
    actual = digest(archive)
    if actual.lower() != expected_sha256.lower():
        raise ValueError(f"JCode archive checksum mismatch: {actual}")

    output_dir.mkdir(parents=True, exist_ok=True)
    unexpected = sorted(
        path.name for path in output_dir.iterdir() if path.name not in OUTPUT_FILES
    )
    if unexpected:
        raise ValueError(
            "JCode output directory contains unmanaged files: " + ", ".join(unexpected)
        )
    executable = output_dir / "jcode.exe"
    with tarfile.open(archive, mode="r:gz") as bundle:
        matches = [member for member in bundle.getmembers() if member.name == ARCHIVE_MEMBER]
        if len(matches) != 1 or not matches[0].isfile():
            raise ValueError(f"JCode archive must contain exactly one {ARCHIVE_MEMBER}")
        source = bundle.extractfile(matches[0])
        if source is None:
            raise ValueError("JCode executable could not be read from archive")
        with source, executable.open("wb") as target:
            shutil.copyfileobj(source, target)

    shutil.copy2(LICENSE, output_dir / "LICENSE.txt")
    (output_dir / "VERSION.txt").write_text(f"jcode v{VERSION}\n", encoding="utf-8")
    return executable


def main() -> int:
    parser = argparse.ArgumentParser(description="Fetch the pinned JCode Windows developer-agent sidecar.")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--archive", type=Path, help="Use an existing archive instead of downloading it.")
    args = parser.parse_args()

    if args.archive:
        executable = install(args.archive.resolve(), args.output_dir.resolve())
    else:
        with tempfile.TemporaryDirectory(prefix="drpa-jcode-") as temporary:
            archive = Path(temporary) / ARCHIVE_NAME
            download_verified(DOWNLOAD_URL, archive)
            executable = install(archive, args.output_dir.resolve())
    print(f"staged JCode v{VERSION}: {executable}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
