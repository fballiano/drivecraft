#!/usr/bin/env bash
# Tests the Linux tray icon in a Wayland session (sway, headless) and in an
# X11 session (Xvfb with Openbox), each with its own D-Bus session bus.
#
#   scripts/tray-test/run.sh <drivecraft binary> <output folder> [wayland|x11]
#
# It needs sway, grim, Xvfb, openbox, wmctrl, xdotool, ImageMagick, and
# python3-gi (see the Dockerfile). On macOS, use scripts/tray-test.sh.
set -uo pipefail
bin=$(realpath "$1")
out=$(mkdir -p "$2" && realpath "$2")
here=$(cd "$(dirname "$0")" && pwd)
export bin out here
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-$(mktemp -d)}
chmod 700 "$XDG_RUNTIME_DIR"

wayland() {
    cat > "$out/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1280x800
bar {
    position top
    status_command while :; do echo DriveCraft tray test; sleep 60; done
}
EOF
    WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
        sway -c "$out/sway.conf" > "$out/wayland-sway.log" 2>&1 &
    for _ in $(seq 50); do
        ls "$XDG_RUNTIME_DIR"/sway-ipc.* >/dev/null 2>&1 && break
        sleep 0.2
    done
    WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-? | head -n 1)")
    SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.* | head -n 1)
    export WAYLAND_DISPLAY SWAYSOCK
    unset DISPLAY
    # swaybar is the StatusNotifierWatcher.
    for _ in $(seq 50); do
        gdbus introspect --session -d org.kde.StatusNotifierWatcher -o /StatusNotifierWatcher >/dev/null 2>&1 && break
        sleep 0.2
    done
    python3 "$here/traytest.py" wayland "$bin" "$out"
}

x11() {
    Xvfb :99 -screen 0 1280x800x24 > "$out/x11-xvfb.log" 2>&1 &
    export DISPLAY=:99
    unset WAYLAND_DISPLAY
    sleep 1
    openbox > "$out/x11-openbox.log" 2>&1 &
    python3 "$here/watcher.py" &
    sleep 1
    python3 "$here/traytest.py" x11 "$bin" "$out"
}
export -f wayland x11

status=0
for mode in ${3:-wayland x11}; do
    dbus-run-session -- bash -c "$mode" || status=1
done
exit $status
