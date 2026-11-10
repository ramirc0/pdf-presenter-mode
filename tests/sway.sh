# shellcheck shell=bash
# Starts a headless sway for GUI tests and screenshots, so the user's session is untouched.
# Source it from a script that runs inside the dev shell and sets $work to a scratch directory.

# start_sway OUTPUTS CONFIG: starts sway with OUTPUTS virtual outputs set up by the sway CONFIG
# lines, and exports WAYLAND_DISPLAY and SWAYSOCK for it.
start_sway() {
  local outputs=$1 before sock
  # shellcheck disable=SC2154 # The sourcing script sets work.
  printf '%s\n' "$2" > "$work/sway.conf"
  before=$(ls "$XDG_RUNTIME_DIR"/wayland-? 2>/dev/null || true)
  env -u WAYLAND_DISPLAY -u DISPLAY -u SWAYSOCK WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS="$outputs" \
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
