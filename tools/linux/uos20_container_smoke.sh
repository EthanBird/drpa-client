#!/bin/sh
set -eu

diagnostics="${DRPA_DIAGNOSTICS_DIR:-/diagnostics}"
mkdir -p "$diagnostics"

system_glibc="$(getconf GNU_LIBC_VERSION)"
if [ "$system_glibc" != "glibc 2.28" ]; then
  echo "expected Debian 10 glibc 2.28, got $system_glibc" >&2
  exit 1
fi

if dpkg-query -W -f='${Status}' libwebkit2gtk-4.1-0 2>/dev/null | grep -Fq 'install ok installed'; then
  echo "system libwebkit2gtk-4.1-0 must not be installed" >&2
  exit 1
fi
if [ "${DRPA_ALLOW_SYSTEM_MESA_DRI:-0}" != "1" ] \
  && dpkg-query -W -f='${Status}' libgl1-mesa-dri 2>/dev/null | grep -Fq 'install ok installed'; then
  echo "system libgl1-mesa-dri must not be installed" >&2
  exit 1
fi

app_root=/opt/drpa-next-uos20
loader="$app_root/uos-runtime/ld-linux-x86-64.so.2"
desktop="$app_root/usr/bin/drpa-desktop"
test -x /usr/bin/drpa-next
test -x "$loader"
test -x "$desktop"
"$loader" --verify "$desktop"
"$loader" --list "$desktop" > /tmp/drpa-desktop-loader-list.txt
grep -Fq "$app_root/uos-runtime/libc.so.6" /tmp/drpa-desktop-loader-list.txt
grep -Fq "$app_root/uos-runtime/libstdc++.so.6" /tmp/drpa-desktop-loader-list.txt
grep -Fq "$app_root/uos-runtime/libX11.so.6" /tmp/drpa-desktop-loader-list.txt
grep -Fq "$app_root/uos-runtime/libfribidi.so.0" /tmp/drpa-desktop-loader-list.txt
grep -Fq "$app_root/uos-runtime/libgbm.so.1" /tmp/drpa-desktop-loader-list.txt
grep -aFq gbm_bo_create_with_modifiers2 "$app_root/uos-runtime/libgbm.so.1"
grep -Fq "$app_root/uos-runtime/libdrm.so.2" /tmp/drpa-desktop-loader-list.txt
grep -aFq drmGetFormatModifierName "$app_root/uos-runtime/libdrm.so.2"
grep -Fq "$app_root/uos-runtime/libEGL.so.1" /tmp/drpa-desktop-loader-list.txt
test -f "$app_root/uos-runtime/libEGL_mesa.so.0"
test -f "$app_root/uos-runtime/libGLdispatch.so.0"
test -f "$app_root/uos-runtime/dri/swrast_dri.so"
test -f "$app_root/uos-runtime/dri/kms_swrast_dri.so"
grep -Fq libEGL_mesa.so.0 "$app_root/uos-runtime/egl_vendor.d/50_mesa.json"

runtime_manifest="$(find "$app_root" -type f -path '*/runtime/manifest.json' -print -quit)"
test -n "$runtime_manifest"
runtime_root="$(dirname "$runtime_manifest")"
runtime_python="$(find "$runtime_root/python" -type f -name python3.11 -perm /111 -print -quit)"
test -n "$runtime_python"

"$runtime_python" -c \
  'import ctypes; libc=ctypes.CDLL("libc.so.6"); libc.gnu_get_libc_version.restype=ctypes.c_char_p; assert libc.gnu_get_libc_version()==b"2.35"'
env \
  PIP_NO_INDEX=1 \
  UV_OFFLINE=1 \
  UV_NO_MANAGED_PYTHON=1 \
  UV_PYTHON_DOWNLOADS=never \
  "$runtime_python" "$runtime_root/bootstrap_runtime.py" --environment /tmp/drpa-uos20-environment
/tmp/drpa-uos20-environment/bin/python -I -c \
  'import debugpy, drpa_runner, DrissionPage, ipykernel, jupyter_client, lxml, nbformat, numpy, pandas, psutil, rpds, tornado, zmq; print("DRPA_UOS20_PYTHON_OK")'

browser="$(find "$runtime_root/browser" -type f -name chrome -perm /111 -print -quit)"
test -n "$browser"
useradd --create-home --shell /bin/sh drpa-smoke
set +e
runuser -u drpa-smoke -- "$browser" \
  --headless \
  --disable-gpu \
  --no-sandbox \
  --dump-dom 'data:text/html,<title>DRPA_UOS20_CHROME_OK</title>' \
  > /tmp/drpa-uos20-chrome.log 2>&1
browser_status=$?
set -e
if [ "$browser_status" -ne 0 ]; then
  cat /tmp/drpa-uos20-chrome.log
  exit "$browser_status"
fi
grep -Fq DRPA_UOS20_CHROME_OK /tmp/drpa-uos20-chrome.log

install -d -o drpa-smoke -g drpa-smoke /tmp/drpa-uos20-data
touch /tmp/drpa-uos20-data/user-data-sentinel
ui_ready=/tmp/drpa-uos20-ui-ready.json
gui_log="$diagnostics/drpa-uos20-gui.log"
screenshot="$diagnostics/drpa-uos20-ui.png"
processes="$diagnostics/drpa-uos20-processes.txt"
window_tree="$diagnostics/drpa-uos20-window-tree.txt"
rm -f "$ui_ready" "$screenshot"

Xvfb :99 -screen 0 1280x800x24 -ac -nolisten tcp >"$diagnostics/xvfb.log" 2>&1 &
xvfb_pid=$!
desktop_pid=""
cleanup() {
  if [ -n "$desktop_pid" ]; then
    kill "$desktop_pid" 2>/dev/null || true
  fi
  pkill -u drpa-smoke -f drpa-desktop 2>/dev/null || true
  kill "$xvfb_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

display_ready=0
for _ in $(seq 1 50); do
  if xdpyinfo -display :99 >/dev/null 2>&1; then
    display_ready=1
    break
  fi
  sleep 0.1
done
if [ "$display_ready" -ne 1 ]; then
  cat "$diagnostics/xvfb.log"
  exit 1
fi

runuser -u drpa-smoke -- env \
  DISPLAY=:99 \
  DRPA_DATA_DIR=/tmp/drpa-uos20-data \
  DRPA_UI_READY_FILE="$ui_ready" \
  dbus-run-session -- /usr/bin/drpa-next >"$gui_log" 2>&1 &
desktop_pid=$!

ready=0
for _ in $(seq 1 900); do
  if [ -s "$ui_ready" ]; then
    ready=1
    break
  fi
  if ! kill -0 "$desktop_pid" 2>/dev/null; then
    break
  fi
  sleep 0.1
done

pgrep -a -f 'drpa-desktop|WebKitNetworkProcess|WebKitWebProcess|WebKitGPUProcess' >"$processes" || true
xwininfo -display :99 -root -tree >"$window_tree" 2>&1 || true
if [ "$ready" -ne 1 ]; then
  cat "$gui_log"
  cat "$processes"
  exit 1
fi
python3 -c 'import json,sys; value=json.load(open(sys.argv[1], encoding="utf-8")); assert value["reactMounted"] is True; assert value["ipcRoundTrip"] is True' "$ui_ready"
cp "$ui_ready" "$diagnostics/drpa-uos20-ui-ready.json"
grep -Fq WebKitWebProcess "$processes"
if grep -Eq 'EGL_NOT_INITIALIZED|Could not create .*EGL display|MESA-LOADER: failed to open swrast|Aborting' "$gui_log"; then
  cat "$gui_log"
  exit 1
fi

# The IPC marker proves that React executed. The pixel gate separately catches a
# mounted-but-white surface or a compositor path that never presents WebKit frames.
sleep 2
xwd -display :99 -root -silent -out /tmp/drpa-uos20-ui.xwd
convert /tmp/drpa-uos20-ui.xwd "$screenshot"
colors="$(identify -format '%k' "$screenshot")"
standard_deviation="$(convert "$screenshot" -colorspace Gray -format '%[fx:standard_deviation]' info:)"
python3 -c 'import sys; colors=int(sys.argv[1]); deviation=float(sys.argv[2]); assert colors >= 32, (colors, deviation); assert deviation >= 0.03, (colors, deviation)' "$colors" "$standard_deviation"
printf 'colors=%s\nstandard_deviation=%s\n' "$colors" "$standard_deviation" >"$diagnostics/drpa-uos20-visual-metrics.txt"

cleanup
trap - EXIT INT TERM

dpkg --remove drpa-next
test -f /tmp/drpa-uos20-data/user-data-sentinel
echo DRPA_UOS20_GLIBC228_INSTALL_RUNTIME_CHROME_REACT_IPC_PIXELS_OK
