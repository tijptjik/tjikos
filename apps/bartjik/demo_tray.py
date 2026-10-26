"""Synthetic SNI items for real Waybar tray rendering on a private bus only."""
import os
from pathlib import Path


def fixtures():
    return [
        ("TelegramDesktop", "Telegram", "/usr/share/icons/breeze/status/24/telegram-panel.svg"),
        ("ZapZap", "WhatsApp", "/usr/share/icons/hicolor/scalable/apps/com.rtosta.zapzap.svg"),
        ("vlc", "VLC", str(Path.home() / ".local/share/icons/Neuwaita/scalable/apps/org.videolan.VLC.svg")),
    ]


def main():
    import dbus
    import dbus.service
    import dbus.mainloop.glib
    from gi.repository import GLib

    address = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
    if not address.startswith("unix:path=/tmp/bartjik-bus-"):
        raise RuntimeError("a private demo bus is required")
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()

    class Item(dbus.service.Object):
        def __init__(self, index, app_id, title, icon):
            self.fixture_bus = dbus.bus.BusConnection(address)
            self.name = dbus.service.BusName("org.example.BartjikDemo" + str(index), self.fixture_bus)
            self.app_id, self.title, self.icon = app_id, title, icon
            if not Path(icon).is_file():
                raise FileNotFoundError(icon)
            super().__init__(self.fixture_bus, "/StatusNotifierItem")

        @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="s", out_signature="a{sv}")
        def GetAll(self, interface):
            empty = dbus.Array([], signature="(iiay)")
            return dict(Id=self.app_id, Title=self.title, Category="ApplicationStatus",
                        Status="Active", WindowId=dbus.UInt32(0), IconName=self.icon,
                        IconPixmap=empty, OverlayIconName="", OverlayIconPixmap=empty,
                        AttentionIconName="", AttentionIconPixmap=empty, ItemIsMenu=False,
                        ToolTip=dbus.Struct((self.icon, empty, self.title, ""), signature="sa(iiay)ss"))

        @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ss", out_signature="v")
        def Get(self, interface, name):
            return self.GetAll(interface)[name]

    items = [Item(index, *fixture) for index, fixture in enumerate(fixtures())]
    owner = None

    def register():
        nonlocal owner
        try:
            current = bus.get_name_owner("org.kde.StatusNotifierWatcher")
            if current != owner:
                watcher = dbus.Interface(bus.get_object(current, "/StatusNotifierWatcher"), "org.kde.StatusNotifierWatcher")
                for item in items:
                    watcher.RegisterStatusNotifierItem(item.name.get_name())
                owner = current
        except dbus.DBusException:
            owner = None
        return True

    GLib.timeout_add(100, register)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
