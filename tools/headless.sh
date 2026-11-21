#!/bin/sh
# Run the client on an invisible display (gamescope's headless backend), so
# screenshots and self-tests don't pop up windows. Uses the real GPU.
#
#   tools/headless.sh --quickstart "aurora basin" --screenshot shot.png
#   tools/headless.sh --selftest
#
# SIZE=WxH picks the virtual screen size (default 1600x900).
set -e
cd "$(dirname "$0")/.."
size=${SIZE:-1600x900}
w=${size%x*}
h=${size#*x}
# Always build: a stale binary would test the code as it was, not as it is.
cargo build --release -p client >&2
bin=${CARGO_TARGET_DIR:-target}/release/snowballz
log=$(mktemp)
code=$(mktemp)
# gamescope doesn't pass on the child's exit status, so save it ourselves.
# The game must open on gamescope's own X server, never the desktop's
# Wayland session: winit would pick Wayland if it could see it.
gamescope --backend headless -W "$w" -H "$h" -w "$w" -h "$h" -- \
    env -u WAYLAND_DISPLAY sh -c '"$0" "$@"; echo $? >"'"$code"'"' "$bin" "$@" >"$log" 2>&1 || true
status=$(cat "$code" 2>/dev/null)
# Show the game's own output, not gamescope/Xwayland chatter.
grep -v -E '^\[gamescope\]|CAP_SYS_NICE|Performance will be affected|xkbcomp|^>|Errors from xkbcomp|failed to read Wayland|^\s*$|^The XKEYBOARD' "$log" || true
rm -f "$log" "$code"
exit "${status:-1}"
