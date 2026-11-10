#!/usr/bin/env bash
# Takes the README screenshots of ppm presenting docs/demo.typ in a headless sway.
# Usage: docs/screenshots.sh PPM_BINARY. Run inside the dev shell.
set -euo pipefail

ppm=$(realpath "$1")
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
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

# wtype makes a new keyboard per call, so wait for its keymap, then press and release.
press() { wtype -s 300 "$@"; }
key() { press -P "$1" -s 50 -p "$1"; }
alt() { press -M alt -P "$1" -s 50 -p "$1" -m alt; }
shot() { sleep 1; grim -o HEADLESS-1 "$root/docs/$1.png"; echo "docs/$1.png"; }

# ppm runs with HOME set to $work, so the start page shows the deck under ~ instead of a temporary path.
mkdir -p "$work/talks"
typst compile "$root/docs/demo.typ" "$work/talks/tides.pdf"
# A laptop panel, and a projector to its right for the audience.
start_sway 2 $'output HEADLESS-1 mode 2400x1500 scale 1.5 pos 0 0
output HEADLESS-2 mode 1920x1080 pos 1600 0
default_border none'
swaymsg focus output HEADLESS-1 >/dev/null
HOME=$work nixGLIntel "$ppm" "$work/talks/tides.pdf" --duration 20 > "$work/ppm.log" 2>&1 &
sleep 2

key Right
key Right
shot presenter
alt v
shot notes-first
alt v
key Tab
key Right
shot overview
key Escape
key question
shot shortcuts
key Escape
key q
key q
sleep 1

# The start page, with the deck just presented among the recent ones.
HOME=$work nixGLIntel "$ppm" >> "$work/ppm.log" 2>&1 &
sleep 2
# shellcheck disable=SC2088 # ppm expands the ~ itself, as typed.
press "~/talks/tides.pdf"
key Tab
shot start
