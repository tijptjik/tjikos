"""Read app-specific SNI unread cues without consuming desktop notifications.

Telegram changes IconName while Status remains Active. ZapZap's Qt tray exposes
only a pixmap: its shipped unread counter is a red rectangle over a green or
monochrome logo. Restrict that adapter to ZapZap and require a substantial badge.
"""
import json

SNI = "org.kde.StatusNotifierItem"
WATCHER = "org.kde.StatusNotifierWatcher"
PROPS = "org.freedesktop.DBus.Properties"


def unread(app_id, props):
    if props.get("Status") == "NeedsAttention":
        return True
    if app_id == "TelegramDesktop":
        return "attention" in str(props.get("IconName", "")).lower()
    if app_id != "ZapZap":
        return False
    for width, height, pixels in props.get("IconPixmap", []):
        if width <= 0 or height <= 0 or len(pixels) != width * height * 4:
            continue
        # SNI uses network-order ARGB, not Qt's in-memory BGRA layout.
        red = sum(1 for i in range(0, len(pixels), 4)
                  if pixels[i] > 128 and pixels[i + 1] > 180
                  and pixels[i + 2] < 80 and pixels[i + 3] < 80)
        if red >= max(4, width * height * 0.02):
            return True
    return False


def main():
    import dbus
    import dbus.mainloop.glib
    from gi.repository import GLib

    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    records = {}
    previous = None
    pending = False

    def properties(service, path):
        return dbus.Interface(bus.get_object(service, path, introspect=False), PROPS)

    def refresh():
        nonlocal previous, pending
        pending = False
        states = {}
        for (service, path), app_id in list(records.items()):
            try:
                props = properties(service, path).GetAll(SNI, byte_arrays=True, timeout=1)
                states[app_id] = states.get(app_id, False) or unread(app_id, props)
            except dbus.DBusException:
                records.pop((service, path), None)
        message = json.dumps(states, sort_keys=True)
        if message != previous:
            print(message, flush=True)
            previous = message
        return False

    def schedule():
        nonlocal pending
        if not pending:
            pending = True
            GLib.timeout_add(50, refresh)

    def roster(*args):
        try:
            items = properties(WATCHER, '/StatusNotifierWatcher').Get(
                WATCHER, 'RegisteredStatusNotifierItems', timeout=1)
        except dbus.DBusException:
            items = []
        current = {}
        for item in items:
            service, _, tail = str(item).partition('/')
            path = '/' + tail if tail else '/StatusNotifierItem'
            try:
                if not service.startswith(':'):
                    service = str(bus.get_name_owner(service))
                app_id = str(properties(service, path).Get(SNI, 'Id', timeout=1))
                if app_id in ('TelegramDesktop', 'ZapZap'):
                    current[(service, path)] = app_id
            except dbus.DBusException:
                pass
        records.clear()
        records.update(current)
        schedule()
        return True

    def changed(*args, sender=None, path=None, **kwargs):
        if (sender, path) in records:
            schedule()

    bus.add_signal_receiver(changed, dbus_interface=SNI,
                            sender_keyword='sender', path_keyword='path')
    bus.add_signal_receiver(changed, signal_name='PropertiesChanged', dbus_interface=PROPS,
                            sender_keyword='sender', path_keyword='path')
    bus.add_signal_receiver(roster, dbus_interface=WATCHER)
    roster()
    GLib.timeout_add_seconds(15, roster)  # Recover app/watcher restarts, too.
    GLib.MainLoop().run()


if __name__ == '__main__':
    main()
