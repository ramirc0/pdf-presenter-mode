#!/usr/bin/env bash
# GUI tests in a headless sway with virtual outputs, so the user's session is untouched.
# Usage: tests/gui.sh PPM_BINARY. Run inside the dev shell.
set -euo pipefail

ppm=$(realpath "$1")
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
failures=0

cleanup() {
  pkill -x -P "$$" ppm 2>/dev/null || true
  [ -n "${SWAYSOCK:-}" ] && swaymsg exit >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

start_sway() {
  local before sock
  before=$(ls "$XDG_RUNTIME_DIR"/wayland-? 2>/dev/null || true)
  printf 'output HEADLESS-1 mode 1920x1200\noutput HEADLESS-2 mode 1920x1080\n' > "$work/sway.conf"
  env -u WAYLAND_DISPLAY -u DISPLAY -u SWAYSOCK WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=2 \
    WLR_LIBINPUT_NO_DEVICES=1 nixGLIntel sway -c "$work/sway.conf" > "$work/sway.log" 2>&1 &
  local pid=$!
  for _ in $(seq 100); do
    sock=$(comm -13 <(echo "$before") <(ls "$XDG_RUNTIME_DIR"/wayland-? 2>/dev/null) | head -1)
    if [ -n "$sock" ] && [ -S "$XDG_RUNTIME_DIR/sway-ipc.$UID.$pid.sock" ]; then
      export WAYLAND_DISPLAY=${sock##*/} SWAYSOCK=$XDG_RUNTIME_DIR/sway-ipc.$UID.$pid.sock
      return
    fi
    sleep 0.1
  done
  echo "sway did not start:"; cat "$work/sway.log"; exit 1
}

# Prints where the audience window is, such as "HEADLESS-3 fullscreen", or "none".
# Prints "no presenter" instead when the presenter window is missing.
windows() {
  swaymsg -t get_tree -r | jq -r '[.. | objects | select(.type == "output") as $o | $o | ..
      | objects | select(.app_id? == "ppm") | {title: .name, output: $o.name, full: (.fullscreen_mode == 1)}]
    | if (map(select(.title == "ppm")) | length) == 0 then "no presenter"
      else (map(select(.title == "ppm audience"))
        | if length == 0 then "none" else map("\(.output)\(if .full then " fullscreen" else "" end)") | join(", ") end)
      end'
}

# Waits up to 5 s for the windows to match, then reports.
expect() {
  local name=$1 want=$2 got=""
  for _ in $(seq 50); do
    got=$(windows)
    [ "$got" = "$want" ] && { echo "ok: $name"; return; }
    sleep 0.1
  done
  echo "FAIL: $name: want [$want], got [$got]"
  failures=$((failures + 1))
}

start_sway
"$ppm" "$root/fixtures/basic.pdf" --audience HEADLESS-3 > "$work/ppm.log" 2>&1 &
expect "no audience while its monitor is missing" "none"
swaymsg create_output >/dev/null
expect "hotplug opens the audience fullscreen" "HEADLESS-3 fullscreen"
swaymsg output HEADLESS-3 unplug >/dev/null
expect "unplug closes the audience" "none"
pkill -x -P "$$" ppm; sleep 0.5

# The audience covers the presenter, so eframe stops running the presenter's ui.
swaymsg focus output HEADLESS-2 >/dev/null
"$ppm" "$root/fixtures/basic.pdf" --audience HEADLESS-2 >> "$work/ppm.log" 2>&1 &
expect "the audience covers the presenter's output" "HEADLESS-2 fullscreen"
swaymsg output HEADLESS-2 unplug >/dev/null
expect "unplug closes an audience that covers the presenter" "none"

if [ "$failures" -gt 0 ]; then
  echo "ppm log:"; cat "$work/ppm.log"
  exit 1
fi
