# DRPA sealed offline runtimes

An offline runtime is a platform-specific, immutable release asset. It is not a source checkout, pip cache, or shared virtual environment.

## Supported matrix

| Bundle | Native GitHub runner | Contents |
| --- | --- | --- |
| `windows-x86_64` | `windows-latest` | CPython, uv, wheels, DRPA adapter, Chrome for Testing |
| `linux-x86_64` | `ubuntu-22.04` | CPython, uv, wheels, DRPA adapter, Chrome for Testing |
| `macos-arm64` | `macos-15` | CPython, uv, wheels, DRPA adapter, Chrome for Testing |
| `macos-x86_64` | `macos-15-intel` | CPython, uv, wheels, DRPA adapter, Chrome for Testing |

Every version is declared in `runtime-spec.json`. Python dependencies are exact-pinned in `requirements/runtime.txt`; direct URLs, VCS dependencies, editable installs and index overrides fail validation.

## Bundle contract

```text
drpa-runtime-<version>-<platform>/
├── python/                 Relocatable managed CPython
├── tools/uv[.exe]          Pinned standalone installer/resolver
├── wheelhouse/             Complete target-native wheel closure
├── browser/                Pinned Chrome for Testing
├── locks/runtime.txt       Exact top-level and transitive pins
├── bootstrap_runtime.py    Offline, idempotent environment creation
├── prepare-runtime.*       Platform launcher
├── manifest.json           Per-file size and SHA-256 inventory
└── SHA256SUMS              Human/tool-verifiable checksums
```

The generated environment is deliberately excluded from the archive. It is created at the final install location, using only the bundled interpreter, uv and wheelhouse. This avoids absolute paths and non-relocatable virtual environments.

## Proof of completeness

The native runner performs the following before upload:

1. Download only binary wheels for the exact dependency set.
2. Build the DRPA Python adapter wheel.
3. Install managed CPython and copy the pinned uv executable.
4. Download the pinned Chrome for Testing build.
5. Create a new environment with an empty uv cache and `UV_OFFLINE=1`.
6. Install with `--offline --no-index --find-links`.
7. Run `uv pip check`.
8. Launch bundled Chrome through DrissionPage against a local HTML file.
9. Import the runtime, browser, data and Excel modules.
10. Generate checksums, archive, upload as a GitHub Actions artifact, then publish all four archives to a prerelease.

Any missing wheel, wrong ABI, missing executable, broken browser or invalid pin fails the job before upload.

## Dependency policy

The sealed runtime is the curated baseline, not all of PyPI. Future capabilities are separate signed packs, for example OCR or Windows desktop automation. A `.rpaz` package may provide additional wheels only when its lock and hashes are complete for every declared target platform.
