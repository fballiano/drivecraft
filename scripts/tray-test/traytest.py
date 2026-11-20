"""Tests the Linux tray icon of DriveCraft over D-Bus, as a panel uses it.

    traytest.py wayland|x11 <drivecraft binary> <output folder>

run.sh starts the session (sway or Xvfb with Openbox) and then this script.
The script reads the properties of the tray item, sends left clicks
(Activate) and menu clicks (com.canonical.dbusmenu), and checks the window
and the process. It writes screenshots and the DriveCraft logs to the
output folder.
"""
import json
import os
import subprocess
import sys
import tempfile
import time

from gi.repository import Gio, GLib

MODE, BINARY, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
APP_ID = "io.github.fballiano.drivecraft"
ITEM = "org.kde.StatusNotifierItem"
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
failures = []


def log(msg):
    print(f"[{MODE}] {msg}", flush=True)


def check(ok, msg):
    log(("PASS " if ok else "FAIL ") + msg)
    if not ok:
        failures.append(msg)
    return ok


def call(dest, path, iface, method, args=None, reply=None):
    return bus.call_sync(dest, path, iface, method, args,
                         GLib.VariantType(reply) if reply else None,
                         Gio.DBusCallFlags.NONE, 5000, None).unpack()


def wait(cond, timeout=30, step=0.25):
    end = time.time() + timeout
    while time.time() < end:
        value = cond()
        if value:
            return value
        time.sleep(step)
    return cond()


def registered():
    try:
        (items,) = call("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
                        "org.freedesktop.DBus.Properties", "Get",
                        GLib.Variant("(ss)", ("org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems")),
                        "(v)")
        return items
    except GLib.Error:
        return []


def address(entry):
    name, _, path = entry.partition("/")
    return name, "/" + path if path else "/StatusNotifierItem"


def props(svc, path):
    (p,) = call(svc, path, "org.freedesktop.DBus.Properties", "GetAll", GLib.Variant("(s)", (ITEM,)), "(a{sv})")
    return p


def activate(svc, path):
    call(svc, path, ITEM, "Activate", GLib.Variant("(ii)", (0, 0)))


def menu_items(svc, menu):
    _, layout = call(svc, menu, "com.canonical.dbusmenu", "GetLayout",
                     GLib.Variant("(iias)", (0, -1, [])), "(u(ia{sv}av))")
    out = []
    for cid, cprops, _ in layout[2]:
        label = "---" if cprops.get("type") == "separator" else cprops.get("label", "")
        out.append((cid, label, cprops.get("enabled", True)))
    return out


def click(svc, menu, cid):
    call(svc, menu, "com.canonical.dbusmenu", "Event",
         GLib.Variant("(isvu)", (cid, "clicked", GLib.Variant("s", ""), int(time.time()))))


def sway_window():
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    stack = [tree]
    while stack:
        node = stack.pop()
        if node.get("app_id") == APP_ID:
            return node
        stack += node.get("nodes", []) + node.get("floating_nodes", [])
    return None


def visible():
    if MODE == "wayland":
        return sway_window() is not None
    r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^DriveCraft$"],
                       capture_output=True, text=True)
    return r.stdout.strip() != ""


def screenshot(name):
    path = os.path.join(OUT, f"{MODE}-{name}.png")
    cmd = ["grim", path] if MODE == "wayland" else ["import", "-window", "root", path]
    subprocess.run(cmd, check=False)


def start(*args):
    """Starts DriveCraft with an empty home folder, and waits for its tray item."""
    home = tempfile.mkdtemp(prefix="home-")
    env = dict(os.environ, HOME=home, XDG_CONFIG_HOME=f"{home}/.config", XDG_DATA_HOME=f"{home}/.local/share")
    name = "drivecraft" + "".join(args).replace("-", "_")
    logf = open(os.path.join(OUT, f"{MODE}-{name}.log"), "w")
    proc = subprocess.Popen([BINARY, *args], env=env, stdout=logf, stderr=subprocess.STDOUT)
    items = wait(registered, 60)
    return proc, items


def stop(proc):
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(10)
        except subprocess.TimeoutExpired:
            proc.kill()


def main():
    # A normal start.
    proc, items = start()
    if not check(bool(items), f"the tray item registers with the watcher: {items}"):
        stop(proc)
        return
    svc, path = address(items[0])

    p = props(svc, path)
    check(p.get("Id") == APP_ID, f"Id is {p.get('Id')!r}")
    check(p.get("Title") == "DriveCraft", f"Title is {p.get('Title')!r}")
    check(p.get("Status") == "Active", f"Status is {p.get('Status')!r}")
    pix = p.get("IconPixmap") or []
    check([(w, h) for w, h, _ in pix] == [(32, 32), (64, 64)], "IconPixmap has the sizes 32 and 64")
    if pix:
        w, h, data = pix[0]
        data = bytes(data)
        check(len(data) == w * h * 4, "IconPixmap 32 has 4 bytes for each pixel")
        opaque = [data[i:i + 4] for i in range(0, len(data), 4) if data[i] == 255]
        check(bool(opaque), f"IconPixmap has {len(opaque)} opaque pixels")
        if opaque:
            _, r, g, b = opaque[0]
            check((r, g, b) == (0x8B, 0x7C, 0xF6), f"an opaque pixel in ARGB order is #{r:02X}{g:02X}{b:02X}")

    menu = p.get("Menu")
    entries = menu_items(svc, menu)
    labels = [label for _, label, _ in entries]
    log(f"menu: {labels}")
    check(labels[:2] == ["Open DriveCraft", "---"], "the menu starts with Open DriveCraft and a separator")
    check(labels[3:] == ["Sync now", "Open DriveCraft folder", "---", "Quit DriveCraft"],
          "the menu has Sync now, Open DriveCraft folder, and Quit DriveCraft")
    check([enabled for _, _, enabled in entries].count(False) == 1, "one menu item (the status) is disabled")
    ids = {label: cid for cid, label, _ in entries}

    check(wait(visible, 30), "the window shows at the start")
    # The engine reports its first status: the tooltip and the menu change.
    tip = wait(lambda: props(svc, path)["ToolTip"][3] != "Starting…" and props(svc, path)["ToolTip"][3], 20)
    check(bool(tip), f"the tooltip changes from Starting… to {tip!r}")
    check(tip in [label for _, label, _ in menu_items(svc, menu)], "the status menu item has the same text")
    time.sleep(1)
    screenshot("start")

    if MODE == "x11":
        subprocess.run(["xdotool", "search", "--name", "^DriveCraft$", "windowactivate", "--sync"], check=False)
        time.sleep(1)
        activate(svc, path)
        check(wait(lambda: not visible(), 10), "a left click on the window in front hides it")
        activate(svc, path)
        check(wait(visible, 10), "a left click on the hidden window shows it")
        subprocess.run(["wmctrl", "-c", "DriveCraft"], check=False)
        check(wait(lambda: not visible(), 10), "closing the window hides it")
        time.sleep(2)
        check(proc.poll() is None, "DriveCraft continues after closing the window")
        click(svc, menu, ids["Open DriveCraft"])
        check(wait(visible, 10), "the menu item Open DriveCraft shows the window")
    else:
        # winit cannot hide a window on Wayland: closing minimizes it. sway
        # has no minimized state, so only the process is checked.
        subprocess.run(["swaymsg", f'[app_id="{APP_ID}"] kill'], check=False, capture_output=True)
        time.sleep(3)
        check(proc.poll() is None, "DriveCraft continues after closing the window")
        activate(svc, path)
        click(svc, menu, ids["Open DriveCraft"])
        time.sleep(1)
        check(proc.poll() is None and sway_window() is not None,
              "after a left click and Open DriveCraft, DriveCraft runs and the window exists")

    click(svc, menu, ids["Sync now"])
    time.sleep(1)
    check(proc.poll() is None, "DriveCraft runs after Sync now")
    screenshot("end")

    click(svc, menu, ids["Quit DriveCraft"])
    try:
        proc.wait(15)
        check(True, "the menu item Quit DriveCraft stops DriveCraft")
    except subprocess.TimeoutExpired:
        check(False, "the menu item Quit DriveCraft stops DriveCraft")
        stop(proc)
    check(wait(lambda: not registered(), 10), "the tray item goes away after Quit")

    # A start at login.
    proc, items = start("--autostart")
    check(bool(items), "after --autostart, the tray item registers")
    time.sleep(5)
    check(proc.poll() is None, "DriveCraft runs after --autostart")
    if MODE == "x11" and items:
        check(not visible(), "after --autostart, the window stays hidden")
        activate(*address(items[0]))
        check(wait(visible, 10), "after --autostart, a left click shows the window")
    screenshot("autostart")
    stop(proc)


try:
    main()
except Exception as e:  # A D-Bus error or a missing menu item is a failure too.
    check(False, f"{type(e).__name__}: {e}")
log(f"{len(failures)} failure(s)")
for f in failures:
    log(f"  - {f}")
sys.exit(1 if failures else 0)
