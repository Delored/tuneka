#!/bin/sh
# Execute inside the gui-smoke image, with /evidence mounted to an isolated directory.
set -eu
mkdir -p /evidence
export TUNEKA_RUST_DATA_DIR=/evidence/state
export XDG_RUNTIME_DIR=$(mktemp -d)
trap 'rm -rf "$XDG_RUNTIME_DIR"' EXIT HUP INT TERM
chmod 700 "$XDG_RUNTIME_DIR"
xvfb-run -a -s '-screen 0 1280x900x24' dbus-run-session -- sh -eu <<'EOF'
openbox > /evidence/openbox.log 2>&1 &
wm=$!
tuneka-gui > /evidence/runtime.log 2>&1 &
app=$!
trap 'kill "$app" "$wm" 2>/dev/null || true' EXIT HUP INT TERM
window=''
for attempt in $(seq 1 30); do
    kill -0 "$app"
    window=$(xdotool search --onlyvisible --name '^Tuneka$' | head -n 1) || true
    test -z "$window" || break
    sleep 1
done
test -n "$window"
xdotool windowactivate --sync "$window"
sleep 3
xdotool type --clearmodifiers 'http://localhost/unsupported'
xdotool key ctrl+i
sleep 1
# Captures need visual inspection; this script does not assert the error text.
xwd -silent -id "$window" -out /evidence/validation-error.xwd
xdotool windowsize --sync "$window" 640 480
sleep 1
xwd -silent -id "$window" -out /evidence/compact.xwd
xdotool getwindowgeometry "$window"
xdotool key ctrl+q
wait "$app"
printf 'PASS: visible X11 window, input events sent, resize and cooperative quit; inspect captures for content\n'
EOF
