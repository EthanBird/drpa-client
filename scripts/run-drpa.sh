#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

RUNTIME_ROOT="$ROOT/runtimes/linux"
VENV_PY="$RUNTIME_ROOT/.venv/bin/python"
VENV_APP="$RUNTIME_ROOT/.venv/bin/drpa-client"
export UV_PYTHON_INSTALL_DIR="$ROOT/.tools/python/linux"

PY_HOME="$(find "$ROOT/.tools/python/linux" -maxdepth 1 -type d -name 'cpython-3.11.9-*' 2>/dev/null | head -1 || true)"
if [[ -z "$PY_HOME" ]]; then
  PY_HOME="$(find "$ROOT/.tools/python" -maxdepth 1 -type d -name 'cpython-3.11.9-*linux*' 2>/dev/null | head -1 || true)"
fi

if [[ -n "$PY_HOME" && -x "$PY_HOME/bin/python3" ]]; then
  "$PY_HOME/bin/python3" "$ROOT/tools/fix_offline_runtime.py" >/dev/null 2>&1 || true
fi

if [[ -x "$VENV_PY" && -f "$VENV_APP" ]]; then
  if "$VENV_PY" -c "import drpa_client" >/dev/null 2>&1; then
    echo "[DRPA] Starting DRPA Client (offline mode, Linux)..."
    exec "$VENV_APP"
  fi
fi

if [[ "${DRPA_ALLOW_ONLINE:-}" != "1" ]]; then
  echo "[DRPA] Offline startup failed on Linux."
  echo "Expected runtime: $RUNTIME_ROOT/.venv"
  echo "Expected python: $ROOT/.tools/python/linux/"
  echo "Set DRPA_ALLOW_ONLINE=1 only when developing with network access."
  exit 1
fi

UV_BIN="$ROOT/.tools/uv/uv"
if [[ ! -x "$UV_BIN" ]]; then
  echo "[DRPA] uv not found. Downloading standalone uv..."
  mkdir -p "$ROOT/.tools/uv"
  OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
  ARCH="$(uname -m)"
  case "$OS-$ARCH" in
    linux-x86_64) UV_URL="https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz" ;;
    linux-aarch64|linux-arm64) UV_URL="https://github.com/astral-sh/uv/releases/latest/download/uv-aarch64-unknown-linux-gnu.tar.gz" ;;
    darwin-x86_64) UV_URL="https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-apple-darwin.tar.gz" ;;
    darwin-arm64) UV_URL="https://github.com/astral-sh/uv/releases/latest/download/uv-aarch64-apple-darwin.tar.gz" ;;
    *) echo "[DRPA] Unsupported platform: $OS $ARCH"; exit 1 ;;
  esac
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT
  curl -fsSL "$UV_URL" | tar -xz -C "$TMP_DIR"
  FOUND="$(find "$TMP_DIR" -name uv -type f | head -1)"
  cp "$FOUND" "$UV_BIN"
  chmod +x "$UV_BIN"
fi

echo "[DRPA] Online mode: syncing runtime via uv..."
"$UV_BIN" sync --reinstall-package drpa-client
echo "[DRPA] Starting DRPA Client..."
exec "$UV_BIN" run drpa-client
