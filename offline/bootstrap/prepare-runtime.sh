#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PYTHON=$(find "$ROOT/python" -type f \( -name python3.11 -o -name python3 \) | head -n 1)
if [ -z "$PYTHON" ]; then
  echo "[DRPA offline] bundled Python 3.11.9 is missing." >&2
  exit 1
fi
exec "$PYTHON" "$ROOT/bootstrap_runtime.py"
