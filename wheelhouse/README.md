# DRPA Wheelhouse

This directory stores offline wheels for common DRPA script package dependencies.

Target Python version:

```text
Python 3.11 / CPython cp311
```

Primary packages included:

- DrissionPage
- requests
- pandas
- openpyxl

The wheelhouse also includes transitive dependencies required by those packages.
Windows-specific conditional dependencies are included as well, for example:

- colorama, required by click on Windows

Platform directories:

```text
wheelhouse/linux-x86_64/
wheelhouse/windows-amd64/
```

Runtime behavior:

- Dependency installation is offline-only. DRPA Client always runs pip with `--no-index`.
- During dependency installation, DRPA Client adds the matching platform wheelhouse directory to pip `--find-links`.
- Package-local wheels are still supported through `manifest.yaml -> dependencies.local`.
- Wheel priority is global install-directory wheelhouse first, package-local wheels second.
- DRPA Client generates a constraints file from this global wheelhouse so global wheel versions win over same-name wheels embedded in a `.rpaz`.

Regenerate command:

```bash
mkdir -p wheelhouse/linux-x86_64 wheelhouse/windows-amd64

python3 -m pip download \
  --dest wheelhouse/linux-x86_64 \
  --only-binary=:all: \
  --platform manylinux2014_x86_64 \
  --implementation cp \
  --python-version 311 \
  --abi cp311 \
  DrissionPage requests pandas openpyxl

python3 -m pip download \
  --dest wheelhouse/windows-amd64 \
  --only-binary=:all: \
  --platform win_amd64 \
  --implementation cp \
  --python-version 311 \
  --abi cp311 \
  DrissionPage requests pandas openpyxl
```
