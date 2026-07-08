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

Platform directories:

```text
wheelhouse/linux-x86_64/
wheelhouse/windows-amd64/
```

Runtime behavior:

- During dependency installation, DRPA Client adds the matching platform wheelhouse directory to pip `--find-links`.
- Package-local wheels are still supported through `manifest.yaml -> dependencies.local`.
- If a script package uses `strategy: offline-only`, pip runs with `--no-index` and installs only from package-local wheels plus this global wheelhouse.

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
