#!/bin/sh
set -eux

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
system_dri=/usr/lib/x86_64-linux-gnu/dri
if [ "${DRPA_ALLOW_SYSTEM_MESA_DRI:-0}" != "1" ] && [ -d "$system_dri" ]; then
  mv "$system_dri" /tmp/drpa-disabled-system-dri
  echo "disabled system Mesa DRI modules for the private-runtime rendering test"
fi
if [ "${DRPA_ALLOW_SYSTEM_MESA_DRI:-0}" != "1" ]; then
  test ! -e "$system_dri"
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
font_match="$(runuser -u drpa-smoke -- fc-match --format '%{family}\n' 'sans-serif:lang=zh-cn')"
printf 'font_match=%s\n' "$font_match" >"$diagnostics/drpa-uos20-font-match.txt"
# Font families belong to the host desktop. Keep the match in diagnostics, but
# do not require one distro-specific family before exercising the actual UI.
font_available=0
if [ -n "$font_match" ]; then
  font_available=1
fi
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
input_ready=/tmp/drpa-uos20-input-ready.json
gui_log="$diagnostics/drpa-uos20-gui.log"
screenshot="$diagnostics/drpa-uos20-ui.png"
processes="$diagnostics/drpa-uos20-processes.txt"
window_tree="$diagnostics/drpa-uos20-window-tree.txt"
rm -f "$ui_ready" "$input_ready" "$screenshot"

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
  XMODIFIERS=@im=fcitx \
  DRPA_DATA_DIR=/tmp/drpa-uos20-data \
  DRPA_UI_READY_FILE="$ui_ready" \
  DRPA_UI_INPUT_READY_FILE="$input_ready" \
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
grep -Eq '"reactMounted"[[:space:]]*:[[:space:]]*true' "$ui_ready"
grep -Eq '"ipcRoundTrip"[[:space:]]*:[[:space:]]*true' "$ui_ready"
cp "$ui_ready" "$diagnostics/drpa-uos20-ui-ready.json"

# Exercise the native GTK/WebKit input-method path with real X11 events. Opening
# the command palette focuses an input; the explicit click and typing reproduce
# the UOS freeze that only appeared after the first editable field received focus.
export DISPLAY=:99
# report_ui_ready can arrive while the 880x550 splash window is still visible.
# Wait for the configured main window (minimum 1024x640) so subsequent events do
# not target a window that the startup hand-off is about to destroy.
window_id=""
for _ in $(seq 1 100); do
  for candidate in $(xdotool search --onlyvisible --name '^(DRPA Next|drpa-desktop)$' 2>/dev/null || true); do
    geometry="$(xdotool getwindowgeometry --shell "$candidate" 2>/dev/null || true)"
    width="$(printf '%s\n' "$geometry" | sed -n 's/^WIDTH=//p')"
    height="$(printf '%s\n' "$geometry" | sed -n 's/^HEIGHT=//p')"
    if [ "${width:-0}" -ge 1024 ] && [ "${height:-0}" -ge 640 ]; then
      window_id="$candidate"
      break 2
    fi
  done
  sleep 0.1
done
test -n "$window_id"
xwininfo -display :99 -root -tree >"$window_tree" 2>&1 || true
xdotool windowfocus --sync "$window_id"
xdotool key --clearmodifiers ctrl+k
# The command palette input owns autofocus. Typing directly keeps this smoke
# independent of page-header density and dashboard layout coordinates while
# still exercising WebKit's native keyboard/input-method event path.
sleep 0.6
xdotool type --delay 20 --clearmodifiers 'drpa-input-smoke'
sleep 0.2
xwd -display :99 -root -silent -out /tmp/drpa-uos20-input-stage.xwd
convert /tmp/drpa-uos20-input-stage.xwd "$diagnostics/drpa-uos20-input-stage.png"
xdotool key --clearmodifiers Escape
sleep 0.2
xdotool mousemove --sync --window "$window_id" 100 155 click 1
xwd -display :99 -root -silent -out /tmp/drpa-uos20-post-input-click.xwd
convert /tmp/drpa-uos20-post-input-click.xwd "$diagnostics/drpa-uos20-post-input-click.png"
input_responsive=0
for _ in $(seq 1 100); do
  if [ -s "$input_ready" ]; then
    input_responsive=1
    break
  fi
  if ! kill -0 "$desktop_pid" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
if [ "$input_responsive" -ne 1 ]; then
  cat "$gui_log"
  exit 1
fi
grep -Eq '"nativeInputTyped"[[:space:]]*:[[:space:]]*true' "$input_ready"
grep -Eq '"postInputClick"[[:space:]]*:[[:space:]]*true' "$input_ready"
grep -Eq '"ipcRoundTrip"[[:space:]]*:[[:space:]]*true' "$input_ready"
cp "$input_ready" "$diagnostics/drpa-uos20-input-ready.json"

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
header=/tmp/drpa-uos20-header.png
convert "$screenshot" -crop '1100x100+150+0' +repage "$header"
header_dark_fraction="$(convert "$header" -colorspace Gray -threshold 30% -format '%[fx:1-mean]' info:)"
header_components="$(convert "$header" -colorspace Gray -threshold 30% \
  -define connected-components:verbose=true -connected-components 8 null: 2>&1 | \
  awk 'NR > 1 && $1 != "0:" { count += 1; area = $4 + 0; if (area > largest) largest = area } END { printf "%d %d", count, largest }')"
set -- $header_components
header_component_count="$1"
header_largest_component="$2"
python3 -c 'import sys; colors=int(sys.argv[1]); deviation=float(sys.argv[2]); fraction=float(sys.argv[3]); components=int(sys.argv[4]); largest=int(sys.argv[5]); has_fonts=bool(int(sys.argv[6])); assert colors >= 32, (colors, deviation); assert deviation >= (0.03 if has_fonts else 0.01), (colors, deviation, has_fonts); assert not has_fonts or fraction >= 0.002, (fraction, components, largest); assert not has_fonts or components >= 5, (fraction, components, largest); assert not has_fonts or largest <= 500, (fraction, components, largest)' \
  "$colors" "$standard_deviation" "$header_dark_fraction" "$header_component_count" "$header_largest_component" "$font_available"
printf 'colors=%s\nstandard_deviation=%s\nfont_available=%s\nheader_dark_fraction=%s\nheader_dark_components=%s\nheader_largest_dark_component=%s\n' \
  "$colors" "$standard_deviation" "$font_available" "$header_dark_fraction" "$header_component_count" "$header_largest_component" \
  >"$diagnostics/drpa-uos20-visual-metrics.txt"

cleanup
trap - EXIT INT TERM

dpkg --remove drpa-next
test -f /tmp/drpa-uos20-data/user-data-sentinel
echo DRPA_UOS20_GLIBC228_INSTALL_RUNTIME_CHROME_REACT_IPC_PIXELS_OK
