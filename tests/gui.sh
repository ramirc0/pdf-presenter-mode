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
# egui's clipboard can fall back to X11. Keep ppm off the user's Xwayland.
unset DISPLAY

cleanup() {
  pkill -x -P "$$" ppm 2>/dev/null || true
  [ -n "${copier:-}" ] && kill "$copier" 2>/dev/null || true
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

# Prints where the presenter window is, such as "HEADLESS-1 fullscreen".
presenter() {
  swaymsg -t get_tree -r | jq -r '[.. | objects | select(.type == "output") as $o | $o | ..
      | objects | select(.app_id? == "ppm" and .name == "ppm")
      | "\($o.name)\(if .fullscreen_mode == 1 then " fullscreen" else "" end)"] | join(", ")'
}

# Waits up to 5 s for PROBE, by default the audience's place, to print WANT, then reports.
expect() {
  local name=$1 want=$2 probe=${3:-windows} got=""
  for _ in $(seq 50); do
    got=$($probe)
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
pkill -x -P "$$" ppm; sleep 0.5

# The presenter opens on the focused new output, then moves to the one it asks for.
swaymsg create_output >/dev/null
second=$(swaymsg -t get_outputs -r | jq -r '.[-1].name')
swaymsg focus output "$second" >/dev/null
nixGLIntel "$ppm" "$root/fixtures/basic.pdf" --presenter HEADLESS-1 >> "$work/ppm.log" 2>&1 &
expect "--presenter puts the presenter fullscreen there" "HEADLESS-1 fullscreen" presenter
expect "the audience skips the presenter's monitor" "$second fullscreen"
pkill -x -P "$$" ppm; sleep 0.5

# Without a deck, the start page asks for one. Typing a path and Enter starts the talk.
swaymsg focus output HEADLESS-1 >/dev/null
nixGLIntel "$ppm" >> "$work/ppm.log" 2>&1 &
expect "the start page opens without an audience" "none"
expect "the start page is a normal window" "HEADLESS-1" presenter
wtype -s 300 "$root/fixtures/basic.pdf"
wtype -s 100 -P Return -s 50 -p Return
expect "Enter starts the talk with the presenter where the page was" "HEADLESS-1 fullscreen" presenter
expect "Enter starts the talk with the audience outside" "$second fullscreen"
pkill -x -P "$$" ppm; sleep 0.5

# A path copied in another app pastes into the start page.
nixGLIntel "$ppm" >> "$work/ppm.log" 2>&1 &
expect "the start page opens again" "none"
wl-copy --foreground "$root/fixtures/basic.pdf" &
copier=$!
wtype -s 300 -M ctrl -P v -s 50 -p v -m ctrl
wtype -s 100 -P Return -s 50 -p Return
expect "a pasted path starts the talk" "$second fullscreen"
kill "$copier"
pkill -x -P "$$" ppm; sleep 0.5

# Alt+m moves the audience along the monitors, past the presenter's.
swaymsg create_output >/dev/null
third=$(swaymsg -t get_outputs -r | jq -r '.[-1].name')
swaymsg focus output HEADLESS-1 >/dev/null
nixGLIntel "$ppm" "$root/fixtures/basic.pdf" --audience "$second" >> "$work/ppm.log" 2>&1 &
expect "the audience starts where it asks" "$second fullscreen"
wtype -s 300 -M alt -P m -s 50 -p m -m alt
expect "Alt+m moves the audience to the next monitor" "$third fullscreen"
wtype -s 300 -M alt -P m -s 50 -p m -m alt
expect "Alt+m skips the presenter's monitor" "$second fullscreen"

if [ "$failures" -gt 0 ]; then
  echo "ppm log:"; cat "$work/ppm.log"
  exit 1
fi
