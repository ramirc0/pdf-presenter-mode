#!/usr/bin/env bash
# GUI tests in a headless sway with virtual outputs, so the user's session is untouched.
# Usage: tests/gui.sh PPM_BINARY. Run inside the dev shell, without nixGL around it.
set -euo pipefail

ppm=$(realpath "$1")
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
failures=0
# eframe saves settings under XDG_DATA_HOME. Keep the user's out of reach.
export XDG_DATA_HOME=$work/data

cleanup() {
  pkill -x -P "$$" ppm 2>/dev/null || true
  [ -n "${SWAYSOCK:-}" ] && swaymsg exit >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT
# shellcheck source=tests/sway.sh
source "$root/tests/sway.sh"

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

start_sway 2 $'output HEADLESS-1 mode 1920x1200\noutput HEADLESS-2 mode 1920x1080'
cargo run --quiet --manifest-path "$root/Cargo.toml" -- "$root/fixtures/basic.pdf" --audience HEADLESS-3 \
  > "$work/ppm.log" 2>&1 &
expect "cargo run opens the presenter" "none"
# cargo execs ppm, so ppm is this shell's child.
pkill -x -P "$$" ppm || true
wait $! || true

nixGLIntel "$ppm" "$root/fixtures/basic.pdf" --audience HEADLESS-3 >> "$work/ppm.log" 2>&1 &
expect "no audience while its monitor is missing" "none"
swaymsg create_output >/dev/null
expect "hotplug opens the audience fullscreen" "HEADLESS-3 fullscreen"
swaymsg output HEADLESS-3 unplug >/dev/null
expect "unplug closes the audience" "none"
pkill -x -P "$$" ppm; sleep 0.5

# The audience covers the presenter, so eframe stops running the presenter's ui.
swaymsg focus output HEADLESS-2 >/dev/null
nixGLIntel "$ppm" "$root/fixtures/basic.pdf" --audience HEADLESS-2 >> "$work/ppm.log" 2>&1 &
expect "the audience covers the presenter's output" "HEADLESS-2 fullscreen"
swaymsg output HEADLESS-2 unplug >/dev/null
expect "unplug closes an audience that covers the presenter" "none"

if [ "$failures" -gt 0 ]; then
  echo "ppm log:"; cat "$work/ppm.log"
  exit 1
fi
