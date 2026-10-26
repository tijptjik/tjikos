"""Private-bus SNI/DBusMenu/MPRIS fixture. Never run against the desktop bus."""

import json, os, sys
from pathlib import Path
import dbus, dbus.service, dbus.mainloop.glib
from gi.repository import GLib

if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
    raise RuntimeError("private test bus required")
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
output = Path(sys.argv[1])
control = Path(sys.argv[2])
events = []


def record(event):
    events.append(event)
    output.write_text(json.dumps(events))


class Properties(dbus.service.Object):
    @dbus.service.method(
        "org.freedesktop.DBus.Properties", in_signature="s", out_signature="a{sv}"
    )
    def GetAll(self, interface):
        return self.props(interface)

    @dbus.service.method(
        "org.freedesktop.DBus.Properties", in_signature="ss", out_signature="v"
    )
    def Get(self, interface, name):
        return self.props(interface)[name]

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ssv")
    def Set(self, interface, name, value):
        record("set:" + name)

    @dbus.service.signal("org.freedesktop.DBus.Properties", signature="sa{sv}as")
    def PropertiesChanged(self, interface, values, invalidated):
        pass


class Menu(Properties):
    def props(self, interface):
        return dict(
            Version=dbus.UInt32(3),
            TextDirection="ltr",
            Status="normal",
            IconThemePath=dbus.Array([], signature="s"),
        )

    @dbus.service.method(
        "com.canonical.dbusmenu", in_signature="iias", out_signature="u(ia{sv}av)"
    )
    def GetLayout(self, parent, depth, names):
        return 1, (
            0,
            {"children-display": "submenu"},
            dbus.Array(
                [
                    dbus.Struct(
                        (
                            1,
                            dbus.Dictionary(
                                {
                                    "label": "Synthetic action",
                                    "enabled": True,
                                    "visible": True,
                                },
                                signature="sv",
                            ),
                            dbus.Array([], signature="v"),
                        ),
                        signature="ia{sv}av",
                        variant_level=1,
                    )
                ],
                signature="v",
            ),
        )

    @dbus.service.method(
        "com.canonical.dbusmenu", in_signature="aias", out_signature="a(ia{sv})"
    )
    def GetGroupProperties(self, ids, names):
        return [
            (i, {"label": "Synthetic action", "enabled": True, "visible": True})
            for i in ids
        ]

    @dbus.service.method("com.canonical.dbusmenu", in_signature="i", out_signature="b")
    def AboutToShow(self, id):
        record("menu")
        return False

    @dbus.service.method("com.canonical.dbusmenu", in_signature="isvu")
    def Event(self, id, event, data, timestamp):
        record("menu:" + event)

    @dbus.service.method(
        "com.canonical.dbusmenu", in_signature="a(isvu)", out_signature="ai"
    )
    def EventGroup(self, events):
        return []

    @dbus.service.signal("com.canonical.dbusmenu", signature="ui")
    def LayoutUpdated(self, revision, parent):
        pass

    @dbus.service.signal("com.canonical.dbusmenu", signature="a(ia{sv})a(ias)")
    def ItemsPropertiesUpdated(self, updated, removed):
        pass


class Tray(Properties):
    def __init__(self, path, title):
        self.fixture_bus = dbus.bus.BusConnection(
            os.environ["DBUS_SESSION_BUS_ADDRESS"]
        )
        self.name = dbus.service.BusName(
            "org.example.Bartjik" + path.removeprefix("/"), self.fixture_bus
        )
        self.menu = Menu(self.fixture_bus, "/Menu")
        super().__init__(self.fixture_bus, "/StatusNotifierItem")
        self.title = title
        self.icon = "folder"
        self.app_id = "same-app-id"
        self.pixmaps = dbus.Array([], signature="(iiay)")

    def props(self, interface):
        return {
            "Id": self.app_id,
            "Title": self.title,
            "Category": "ApplicationStatus",
            "Status": "Active",
            "WindowId": dbus.UInt32(0),
            "IconName": self.icon,
            "IconPixmap": self.pixmaps,
            "AttentionIconName": "",
            "AttentionIconPixmap": dbus.Array([], signature="(iiay)"),
            "OverlayIconName": "",
            "OverlayIconPixmap": dbus.Array([], signature="(iiay)"),
            "ToolTip": dbus.Struct(
                (
                    self.icon,
                    dbus.Array([], signature="(iiay)"),
                    self.title,
                    "Synthetic fixture",
                ),
                signature="sa(iiay)ss",
            ),
            "ItemIsMenu": False,
            "Menu": dbus.ObjectPath("/Menu"),
        }

    @dbus.service.method("org.kde.StatusNotifierItem", in_signature="ii")
    def Activate(self, x, y):
        record(self.title + ":activate")

    @dbus.service.method("org.kde.StatusNotifierItem", in_signature="ii")
    def SecondaryActivate(self, x, y):
        record(self.title + ":secondary")

    @dbus.service.method("org.kde.StatusNotifierItem", in_signature="ii")
    def ContextMenu(self, x, y):
        record(self.title + ":context")

    @dbus.service.method("org.kde.StatusNotifierItem", in_signature="is")
    def Scroll(self, delta, orientation):
        record(self.title + ":scroll:" + str(delta))

    @dbus.service.signal("org.kde.StatusNotifierItem")
    def NewIcon(self):
        pass

    @dbus.service.signal("org.kde.StatusNotifierItem")
    def NewToolTip(self):
        pass

    @dbus.service.signal("org.kde.StatusNotifierItem", signature="s")
    def NewStatus(self, status):
        pass


class Player(Properties):
    def __init__(self):
        self.name = dbus.service.BusName("org.mpris.MediaPlayer2.bartjikFixture", bus)
        super().__init__(bus, "/org/mpris/MediaPlayer2")
        self.title = "Synthetic title"
        self.can_seek = False
        self.position = 0
        self.shuffle = False

    def props(self, interface):
        if interface == "org.mpris.MediaPlayer2":
            return dict(
                CanQuit=True,
                CanRaise=True,
                HasTrackList=False,
                Identity="BartjikFixture",
                DesktopEntry="bartjikFixture",
                SupportedUriSchemes=dbus.Array([], signature="s"),
                SupportedMimeTypes=dbus.Array([], signature="s"),
            )
        return dict(
            PlaybackStatus="Playing",
            LoopStatus="None",
            Rate=1.0,
            Shuffle=self.shuffle,
            Metadata=dbus.Dictionary(
                {
                    "mpris:trackid": dbus.ObjectPath("/track/1"),
                    "mpris:artUrl": (Path(__file__).resolve().parents[1] / "qml/icons/demo-album.png").as_uri(),
                    "xesam:album": "Synthetic album",
                    "xesam:title": self.title,
                    "xesam:artist": dbus.Array(["Synthetic artist"], signature="s"),
                },
                signature="sv",
            ),
            Volume=0.5,
            Position=dbus.Int64(self.position),
            MinimumRate=1.0,
            MaximumRate=1.0,
            CanGoNext=True,
            CanGoPrevious=True,
            CanPlay=True,
            CanPause=True,
            CanSeek=self.can_seek,
            CanControl=True,
        )

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ssv")
    def Set(self, interface, name, value):
        if name == 'Shuffle':
            self.shuffle = bool(value)
            self.PropertiesChanged(interface, {'Shuffle': self.shuffle}, [])
            record('player:shuffle:' + str(self.shuffle))

    @dbus.service.signal("org.mpris.MediaPlayer2.Player", signature="x")
    def Seeked(self, position):
        pass

    @dbus.service.method("org.mpris.MediaPlayer2.Player", in_signature="ox")
    def SetPosition(self, track, position):
        self.position = int(position)
        self.Seeked(dbus.Int64(self.position))
        record('player:restart:' + str(self.position))

    @dbus.service.method("org.mpris.MediaPlayer2.Player")
    def PlayPause(self):
        record("player:toggle")

    @dbus.service.method("org.mpris.MediaPlayer2.Player")
    def Pause(self):
        record("player:toggle")

    @dbus.service.method("org.mpris.MediaPlayer2.Player")
    def Play(self):
        record("player:toggle")

    @dbus.service.method("org.mpris.MediaPlayer2.Player")
    def Next(self):
        record("player:next")

    @dbus.service.method("org.mpris.MediaPlayer2.Player")
    def Previous(self):
        record("player:previous")

    @dbus.service.method("org.mpris.MediaPlayer2")
    def Quit(self):
        record("player:quit")


menu = Menu(bus, "/Menu")
trays = [
    Tray("/TrayOne", "Synthetic One"),
    Tray("/TrayTwo", "Synthetic Two"),
    Tray("/InputMethod", "Input Method"),
    Tray("/Solaar", "Solaar"),
]
player = Player()
registered = False
previous = ""


def tick():
    global registered, previous
    if not registered:
        try:
            watcher = dbus.Interface(
                bus.get_object(
                    "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher"
                ),
                "org.kde.StatusNotifierWatcher",
            )
            for t in trays:
                watcher.RegisterStatusNotifierItem(t.fixture_bus.get_unique_name() if t is trays[0] else t.name.get_name())
            registered = True
        except dbus.DBusException:
            pass
    if control.exists():
        cmd = control.read_text()
        if cmd != previous:
            previous = cmd
            if cmd in ("unread", "read"):
                trays[0].app_id = "TelegramDesktop"
                trays[1].app_id = "ZapZap"
                trays[0].icon = "org.telegram.desktop-attention-symbolic" if cmd == "unread" else "org.telegram.desktop-symbolic"
                pixel = bytes([255, 255, 0, 0] if cmd == "unread" else [255, 255, 255, 255])
                trays[1].pixmaps = dbus.Array([(8, 8, dbus.ByteArray(pixel * 64))], signature="(iiay)")
                for t in trays[:2]:
                    t.PropertiesChanged("org.kde.StatusNotifierItem", {"Id":t.app_id}, [])
                    t.NewIcon()
            if cmd == "seekable":
                player.can_seek = True
                player.position = 30000000
                player.PropertiesChanged('org.mpris.MediaPlayer2.Player', {'CanSeek':True}, [])
                player.Seeked(dbus.Int64(player.position))
            if cmd == "update":
                trays[0].icon = "preferences-system"
                trays[0].NewIcon()
                player.title = "Changed synthetic title"
                player.PropertiesChanged(
                    "org.mpris.MediaPlayer2.Player",
                    {
                        "Metadata": player.props("org.mpris.MediaPlayer2.Player")[
                            "Metadata"
                        ]
                    },
                    [],
                )
    return True


GLib.timeout_add(100, tick)
GLib.MainLoop().run()
