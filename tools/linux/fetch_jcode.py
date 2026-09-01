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
VERSION = "0.74.0"
ARCHIVE_NAME = "jcode-linux-x86_64.tar.gz"
ARCHIVE_MEMBER = "jcode-linux-x86_64"
ARCHIVE_BINARY_MEMBER = "jcode-linux-x86_64.bin"
ARCHIVE_SHA256 = "1cc5104113be0478e62d9f234f379a46cb2f9307c105d40080f8a9c21e8892e9"
DOWNLOAD_URL = f"https://github.com/1jehuang/jcode/releases/download/v{VERSION}/{ARCHIVE_NAME}"
LICENSE = ROOT / "installer" / "third-party" / "jcode-LICENSE.txt"
OUTPUT_FILES = {"jcode", "jcode.bin", "LICENSE.txt", "VERSION.txt", ".gitkeep"}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def download(url: str, destination: Path) -> None:
    request = urllib.request.Request(
        url, headers={"User-Agent": "DRPA-Next-release-builder/2.1"}
    )
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
        except Exception as error:  # noqa: BLE001 - one verified release download operation
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
    unexpected = sorted(path.name for path in output_dir.iterdir() if path.name not in OUTPUT_FILES)
    if unexpected:
        raise ValueError("JCode output directory contains unmanaged files: " + ", ".join(unexpected))

    executable = output_dir / "jcode"
    binary = output_dir / "jcode.bin"
    with tarfile.open(archive, mode="r:gz") as bundle:
        members = bundle.getmembers()
        wrappers = [member for member in members if member.name == ARCHIVE_MEMBER]
        binaries = [member for member in members if member.name == ARCHIVE_BINARY_MEMBER]
        if len(wrappers) != 1 or not wrappers[0].isfile():
            raise ValueError(f"JCode archive must contain exactly one {ARCHIVE_MEMBER}")
        if len(binaries) != 1 or not binaries[0].isfile():
            raise ValueError(f"JCode archive must contain exactly one {ARCHIVE_BINARY_MEMBER}")
        wrapper_source = bundle.extractfile(wrappers[0])
        binary_source = bundle.extractfile(binaries[0])
        if wrapper_source is None or binary_source is None:
            raise ValueError("JCode executable files could not be read from archive")
        wrapper = wrapper_source.read().replace(
            ARCHIVE_BINARY_MEMBER.encode("utf-8"), b"jcode.bin"
        )
        if b'exec "$self_dir/jcode.bin" "$@"' not in wrapper:
            raise ValueError("JCode launcher does not contain the expected relative binary entry")
        executable.write_bytes(wrapper)
        with binary_source, binary.open("wb") as target:
            shutil.copyfileobj(binary_source, target)

    executable.chmod(0o755)
    binary.chmod(0o755)
    shutil.copy2(LICENSE, output_dir / "LICENSE.txt")
    (output_dir / "VERSION.txt").write_text(f"jcode v{VERSION}\n", encoding="utf-8")
    return executable


def main() -> int:
    parser = argparse.ArgumentParser(description="Fetch the pinned JCode Linux agent sidecar.")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--archive", type=Path, help="Use an existing archive instead of downloading it.")
    args = parser.parse_args()

    if args.archive:
        executable = install(args.archive.resolve(), args.output_dir.resolve())
    else:
        with tempfile.TemporaryDirectory(prefix="drpa-jcode-linux-") as temporary:
            archive = Path(temporary) / ARCHIVE_NAME
            download_verified(DOWNLOAD_URL, archive)
            executable = install(archive, args.output_dir.resolve())
    print(f"staged JCode v{VERSION}: {executable}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
