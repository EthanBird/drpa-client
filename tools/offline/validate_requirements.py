from __future__ import annotations

import argparse
import re
from pathlib import Path


EXACT_PIN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*\[[A-Za-z0-9_,.-]+\]==[^;\s]+(?:\s*;.+)?$|^[A-Za-z0-9][A-Za-z0-9._-]*==[^;\s]+(?:\s*;.+)?$")
FORBIDDEN = ("://", "git+", "hg+", "svn+", "bzr+", " @ ", "--index", "--extra-index", "--find-links", "-e ")


def validate_requirements(path: Path) -> list[str]:
    errors: list[str] = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        lowered = line.lower()
        if any(token in lowered for token in FORBIDDEN):
            errors.append(f"{path}:{number}: network, VCS and index directives are forbidden")
        elif line.startswith(("/", "\\", ".")) or not EXACT_PIN.fullmatch(line):
            errors.append(f"{path}:{number}: dependency must use an exact == pin: {line}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("requirements", nargs="+", type=Path)
    args = parser.parse_args()
    errors = [error for path in args.requirements for error in validate_requirements(path)]
    if errors:
        print("\n".join(errors))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
