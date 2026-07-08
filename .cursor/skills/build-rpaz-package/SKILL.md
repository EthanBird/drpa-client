---
name: build-rpaz-package
description: Build, validate, and package a DRPA Python RPA script package as .rpaz for AI-assisted development.
---

# Build DRPA `.rpaz` Script Package

Use this skill when an AI agent needs to create or update a DRPA Client script package.

## Required Python Version

Use **Python 3.11.9** for package development and validation.

The project currently targets Python 3.11+ runtime behavior, but AI-generated script packages should be authored and validated against **Python 3.11.9** unless the user explicitly requests another version.

Recommended command prefix in Linux cloud environments:

```bash
python3
```

If multiple Python versions are available, prefer:

```bash
python3.11
```

## Package Layout

Create packages under:

```text
examples/<package_id>/
```

Required files:

```text
examples/<package_id>/
  manifest.yaml
  main.py
  README.md
  wheels/
    common/
    windows/
    linux/
```

Optional files:

```text
assets/
requirements.txt
recorder_notes.md
recording.json
selectors.json
```

## Manifest Requirements

`manifest.yaml` must contain:

```yaml
id: example_package
name: Example Package
version: 0.1.0
entry: main.py
description: Describe what this RPA package does.
author: drpa-ai

runtime:
  python: ">=3.11,<3.12"
  isolation: venv

dependencies:
  strategy: offline-first
  pip: []
  local:
    common:
      - wheels/common/*.whl
    windows:
      - wheels/windows/*.whl
    linux:
      - wheels/linux/*.whl

params:
  - name: headless
    label: 无头浏览器
    type: boolean
    required: false
    default: true
```

When adding browser automation, prefer `DrissionPage` and declare it under `dependencies.pip` if it is not already expected from the DRPA runtime.

## Script Entry

`main.py` must define:

```python
from __future__ import annotations


def main(ctx):
    ctx.log.info("任务开始")
    # Use ctx.params, ctx.output_file(), ctx.progress(), ctx.browser()
```

Do not run code at import time.

Do not read secrets from source code.

Use `ctx.params` for user-configurable values.

Use `ctx.output_file()` for generated artifacts.

## Build Command

From repository root:

```bash
python3 - <<'PY'
import zipfile
from pathlib import Path

package_id = "example_package"
source = Path("examples") / package_id
target = Path("examples") / f"{package_id}.rpaz"

if target.exists():
    target.unlink()

with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(source.rglob("*")):
        if path.is_file():
            archive.write(path, path.relative_to(source))

print(target)
PY
```

Replace `example_package` with the actual package id.

## Validation

Run syntax checks:

```bash
python3 -m compileall examples/<package_id>
```

Validate manifest parsing and package installation without dependencies when possible:

```bash
PYTHONPATH=src python3 - <<'PY'
import os
import shutil
from pathlib import Path

from drpa_client.core.package_manager import PackageManager

package_id = "example_package"
archive = Path("examples") / f"{package_id}.rpaz"
data_dir = Path("/tmp") / f"drpa-validate-{package_id}"
shutil.rmtree(data_dir, ignore_errors=True)
os.environ["DRPA_DATA_DIR"] = str(data_dir)

package = PackageManager().install_archive(archive, install_dependencies=False)
print(package.display_name)
PY
```

For packages with no external dependencies, run an end-to-end task:

```bash
PYTHONPATH=src python3 - <<'PY'
import os
import shutil
import time
from pathlib import Path

from drpa_client.core.database import RunStore
from drpa_client.core.package_manager import PackageManager
from drpa_client.core.task_runner import TaskRunner

package_id = "example_package"
archive = Path("examples") / f"{package_id}.rpaz"
data_dir = Path("/tmp") / f"drpa-run-{package_id}"
shutil.rmtree(data_dir, ignore_errors=True)
os.environ["DRPA_DATA_DIR"] = str(data_dir)

manager = PackageManager()
package = manager.install_archive(archive, install_dependencies=True)
store = RunStore()
events = []
task = TaskRunner(run_store=store).start(package, {}, events.append)

while task.process.poll() is None:
    time.sleep(0.1)

runs = store.list_runs()
print(runs[0].status if runs else "missing-run")
PY
```

## Quality Checklist

Before committing:

- `manifest.yaml` has stable `id`, `name`, `version`, and `entry`.
- Python runtime is declared as `">=3.11,<3.12"` unless there is a reason to broaden it.
- `main.py` defines only `main(ctx)` and helper functions.
- No hard-coded passwords, tokens, cookies, or local machine paths.
- Outputs use `ctx.output_file()`.
- Logs use `ctx.log`.
- Long-running steps call `ctx.progress()`.
- Browser scripts use `ctx.browser()` rather than constructing unrelated browser drivers.
- Generated `.rpaz` is not committed unless it is an intentional bundled default package.
