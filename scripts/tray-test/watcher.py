"""A minimal StatusNotifierWatcher, for the X11 session of the tray test.

A desktop panel (KDE Plasma, swaybar, the AppIndicator extension of GNOME)
does this work. Xvfb with Openbox has no panel, so this script takes its
place. It removes an item when its program leaves the bus, like a panel.
"""
from gi.repository import Gio, GLib

XML = """
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
    <method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>
"""
PATH = "/StatusNotifierWatcher"
IFACE = "org.kde.StatusNotifierWatcher"
items = []  # "bus name/object path"


def method(conn, sender, path, iface, name, params, invocation):
    if name == "RegisterStatusNotifierItem":
        (service,) = params.unpack()
        item = sender + service if service.startswith("/") else service + "/StatusNotifierItem"
        if item not in items:
            items.append(item)
            conn.emit_signal(None, PATH, IFACE, "StatusNotifierItemRegistered", GLib.Variant("(s)", (item,)))
    invocation.return_value(None)


def prop(conn, sender, path, iface, name):
    return {
        "RegisteredStatusNotifierItems": GLib.Variant("as", items),
        "IsStatusNotifierHostRegistered": GLib.Variant("b", True),
        "ProtocolVersion": GLib.Variant("i", 0),
    }[name]


def owner_changed(conn, sender, path, iface, signal, params):
    name, _old, new = params.unpack()
    if new:
        return
    for item in [i for i in items if i.split("/", 1)[0] == name]:
        items.remove(item)
        conn.emit_signal(None, PATH, IFACE, "StatusNotifierItemUnregistered", GLib.Variant("(s)", (item,)))


def acquired(conn, name):
    info = Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0]
    conn.register_object(PATH, info, method, prop, None)
    conn.signal_subscribe("org.freedesktop.DBus", "org.freedesktop.DBus", "NameOwnerChanged",
                          "/org/freedesktop/DBus", None, Gio.DBusSignalFlags.NONE, owner_changed)


Gio.bus_own_name(Gio.BusType.SESSION, "org.kde.StatusNotifierWatcher", 0, acquired, None, None)
GLib.MainLoop().run()
