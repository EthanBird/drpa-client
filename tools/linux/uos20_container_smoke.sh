#!/bin/sh
set -eu

system_glibc="$(getconf GNU_LIBC_VERSION)"
if [ "$system_glibc" != "glibc 2.28" ]; then
  echo "expected Debian 10 glibc 2.28, got $system_glibc" >&2
  exit 1
fi

if dpkg-query -W -f='${Status}' libwebkit2gtk-4.1-0 2>/dev/null | grep -Fq 'install ok installed'; then
  echo "system libwebkit2gtk-4.1-0 must not be installed" >&2
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
set +e
runuser -u drpa-smoke -- timeout 20s xvfb-run -a env \
  DRPA_DATA_DIR=/tmp/drpa-uos20-data \
  /usr/bin/drpa-next > /tmp/drpa-uos20-gui.log 2>&1
status=$?
set -e
if [ "$status" -ne 124 ]; then
  cat /tmp/drpa-uos20-gui.log
  exit "$status"
fi

dpkg --remove drpa-next
test -f /tmp/drpa-uos20-data/user-data-sentinel
echo DRPA_UOS20_GLIBC228_INSTALL_RUNTIME_CHROME_GUI_OK
