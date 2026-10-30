"""Real native MPRIS and tray services on a private D-Bus session."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))

import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc
from render import wait_for


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("private bus required")
    with tempfile.TemporaryDirectory(prefix="bartjik-native-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        shell = temp / "qml/shell.qml"
        s = shell.read_text().replace(
            "case 'status':break;",
            "case 'fixture-action': native.act(req.action,req.id,panels.instances[0],{x:100,y:100,width:20,height:20});break;\n        case 'status':break;",
        )
        s = s.replace(
            "case 'status':break;",
            "case 'fixture-capture':panels.instances[0].capture(req.path);return {ok:true};\n        case 'fixture-menu':return {ok:true,visible:native.menuPopup.visible,entries:native.menuPopup.entries.map(e=>e.text)};\n        case 'fixture-menu-trigger':native.menuPopup.activate(0);return {ok:true};\n        case 'status':break;",
        )
        shell.write_text(s)
        # Exercise the QML focus process without ever focusing desktop apps.
        focus_events=temp/'focus-events.json'
        (temp/'qml/tray_focus.py').write_text(
            'import json,sys\nfrom pathlib import Path\np=Path('+repr(str(focus_events))+')\n'
            'rows=json.loads(p.read_text()) if p.exists() else []\nrows.append(sys.argv[1])\np.write_text(json.dumps(rows))\n')
        native_qml=temp/'qml/Native.qml'
        native_qml.write_text(native_qml.read_text().replace(
            "[Quickshell.env('BARTJIK_HOME')+'/bin/bartjikctl','helper','tray-focus',native.focusApp]",
            "['/usr/bin/python3','-B',"+repr(str(temp/'qml/tray_focus.py'))+",native.focusApp]"))
        cfg = {
            "theme": "rose-punk",
            "modules": {
                "tray": {"provider": "icontray", "exclude": ["input method", "solaar"]},
                "music": {"provider": "playerctl", "players": ["bartjikFixture"]},
            },
            "bars": [
                {
                    "id": "native",
                    "monitors": [1],
                    "islands": [
                        {"id": "tray", "modules": ["tray"]},
                        {"id": "music", "modules": ["music"]},
                    ],
                }
            ],
        }
        (temp / "config.json").write_text(json.dumps(cfg))
        os.environ.update(
            BARTJIK_CONFIG=str(temp / "config.json"), BARTJIK_QML=str(temp / "qml")
        )
        os.environ.pop("BARTJIK_DEMO", None)
        with (temp / "log").open("w") as log:
            p = subprocess.Popen([str(ROOT / "bin/bartjik")], stdout=log, stderr=log)
            fixture = None
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))
                assert (
                    next(
                        m
                        for m in ipc({"op": "modules"})["modules"]
                        if m["id"] == "music"
                    )["items"]
                    == []
                )

                def music_island():
                    return next(
                        i
                        for i in ipc({"op": "bars"})["bars"][0]["islands"]
                        if i["id"] == "music"
                    )

                wait_for(
                    lambda: not music_island()["visible"]
                    and music_island()["width"] == 0
                )
                fixture = subprocess.Popen(
                    [
                        "/usr/bin/python3",
                        str(ROOT / "tests/native_fixture.py"),
                        str(temp / "events.json"),
                        str(temp / "control"),
                    ],
                    stdout=log,
                    stderr=log,
                )

                def module(id):
                    return next(
                        m for m in ipc({"op": "modules"})["modules"] if m["id"] == id
                    )["items"]

                wait_for(lambda: len(module("tray")) == 2)
                wait_for(lambda: bool(module("music")))
                wait_for(
                    lambda: music_island()["visible"] and music_island()["width"] > 0
                )
                cfg["modules"]["music"]["players"] = ["not-selected"]
                (temp / "config.json").write_text(json.dumps(cfg))
                ipc({"op": "reload"})
                wait_for(lambda: module("music") == [])

                def no_media_gap():
                    bar = ipc({"op": "bars"})["bars"][0]
                    tray_island = next(i for i in bar["islands"] if i["id"] == "tray")
                    return abs(bar["geometry"]["width"] - tray_island["width"]) < 0.01

                wait_for(no_media_gap)
                cfg["modules"]["music"]["players"] = ["bartjikFixture"]
                (temp / "config.json").write_text(json.dumps(cfg))
                ipc({"op": "reload"})
                wait_for(lambda: bool(module("music")))
                tray = module("tray")
                assert tray[0]["id"] != tray[1]["id"], tray
                one = next(t for t in tray if "Synthetic One" in t["tooltip"])
                player = module("music")[0]
                assert player["media"]["artwork"].endswith("demo-album.png")
                assert player["artist"]["value"] == "Synthetic artist"
                assert player["track"]["value"] == "Synthetic title"
                assert not player["media"]["playlistAvailable"]
                for action in (
                    "tray.activate",
                    "tray.secondary",
                    "tray.up",
                    "tray.down",
                    "tray.menu",
                ):
                    ipc({"op": "fixture-action", "action": action, "id": one["id"]})
                wait_for(
                    lambda: ipc({"op": "fixture-menu"})["visible"]
                    and "Synthetic action" in ipc({"op": "fixture-menu"})["entries"]
                )
                for action in (
                    "player.toggle",
                    "player.next",
                    "player.previous",
                    "player.quit",
                ):
                    ipc({"op": "fixture-action", "action": action, "id": player["id"]})
                events = wait_for(
                    lambda: json.loads((temp / "events.json").read_text())
                    if (temp / "events.json").exists()
                    and len(json.loads((temp / "events.json").read_text())) >= 8
                    else None
                )
                for event in (
                    "Synthetic One:activate",
                    "Synthetic One:secondary",
                    "Synthetic One:scroll:120",
                    "Synthetic One:scroll:-120",
                    "player:toggle",
                    "player:next",
                    "player:previous",
                    "player:quit",
                ):
                    assert event in events, (event, events)
                (temp / "control").write_text("seekable")
                def player_state():return ipc({'op':'modules'})['native']['players'][0]
                wait_for(lambda: player_state()['canSeek'] and player_state()['position']>20)
                ipc({'op':'fixture-action','action':'player.previous','id':player['id']})
                wait_for(lambda: 'player:restart:0' in json.loads((temp/'events.json').read_text()))
                assert player_state()['position']<1
                previous_count=json.loads((temp/'events.json').read_text()).count('player:previous')
                ipc({'op':'fixture-action','action':'player.previous','id':player['id']})
                wait_for(lambda: json.loads((temp/'events.json').read_text()).count('player:previous')==previous_count+1)
                assert player_state()['shuffleSupported']
                for wanted in (True,False):
                    ipc({'op':'fixture-action','action':'player.shuffle','id':player['id']})
                    wait_for(lambda: player_state()['shuffle'] is wanted)
                (temp / "control").write_text("update")
                wait_for(
                    lambda: "Changed synthetic title"
                    in module("music")[0]["text"]["value"]
                )
                wait_for(
                    lambda: next(t for t in module("tray") if t["id"] == one["id"])[
                        "icon"
                    ]["value"]
                    != one["icon"]["value"]
                )
                assert {t["id"] for t in module("tray")} == {t["id"] for t in tray}
                assert ipc({"op": "fixture-menu"})["visible"]
                # Artwork overrides must not replace stable SNI identity or its open menu.
                cfg["modules"]["tray"]["iconOverrides"] = {
                    "Synthetic One":"builtin:googlechrome", "Synthetic Two":"builtin:mullvad",
                    "Input Method":"builtin:telegram", "Solaar":"builtin:whatsapp",
                }
                cfg["modules"]["tray"]["exclude"] = []
                (temp / "config.json").write_text(json.dumps(cfg))
                ipc({"op":"reload"})
                wait_for(lambda: len(module("tray")) == 4 and all(t["icon"]["kind"] == "image" for t in module("tray")))
                assert {t["id"] for t in tray}.issubset({t["id"] for t in module("tray")})
                assert ipc({"op":"fixture-menu"})["visible"]
                if len(sys.argv) > 1:
                    shot = Path(sys.argv[1]).resolve()
                    shot.parent.mkdir(parents=True,exist_ok=True)
                    time.sleep(.4)
                    ipc({"op":"fixture-capture", "path":str(shot)})
                    wait_for(lambda: shot.exists() and shot.read_bytes().endswith(b"IEND\xaeB`\x82"))
                cfg["modules"]["tray"]["exclude"] = ["input method", "solaar"]
                (temp / "config.json").write_text(json.dumps(cfg))
                ipc({"op":"reload"})
                wait_for(lambda: len(module("tray")) == 2)
                ipc({"op": "fixture-menu-trigger"})
                wait_for(
                    lambda: "menu:clicked"
                    in json.loads((temp / "events.json").read_text())
                )
                ipc({"op": "fixture-action", "action": "tray.menu", "id": one["id"]})
                wait_for(lambda: ipc({"op": "fixture-menu"})["visible"])
                # These clients keep Status=Active and publish unread via artwork.
                (temp / "control").write_text("unread")
                wait_for(lambda: all(t["state"]["urgent"] for t in module("tray")), timeout=25)
                if len(sys.argv) > 1:
                    unread_shot = shot.with_name(shot.stem + '-unread.png')
                    time.sleep(.4)
                    ipc({"op":"fixture-capture", "path":str(unread_shot)})
                    wait_for(lambda: unread_shot.exists() and unread_shot.read_bytes().endswith(b"IEND\xaeB`\x82"))
                (temp / "control").write_text("read")
                wait_for(lambda: all(not t["state"]["urgent"] for t in module("tray")))
                # A recognized tray app dispatches through the focus helper.
                ipc({"op":"fixture-action", "action":"tray.activate", "id":one["id"]})
                wait_for(lambda: focus_events.exists() and json.loads(focus_events.read_text())==['telegram'])
                ipc({"op":"fixture-action", "action":"tray.activate", "id":one["id"]})
                wait_for(lambda: json.loads(focus_events.read_text())==['telegram','telegram'])
                fixture.terminate()
                fixture.wait(timeout=3)
                fixture = None
                wait_for(lambda: not module("tray"))
                wait_for(lambda: not ipc({"op": "fixture-menu"})["visible"])
                wait_for(lambda: module("music") == [])
                wait_for(
                    lambda: not music_island()["visible"]
                    and music_island()["width"] == 0
                )
            finally:
                if fixture:
                    fixture.terminate()
                    fixture.wait(timeout=3)
                p.terminate()
                p.wait(timeout=5)
                log.flush()
                Path("/tmp/bartjik-native-last.log").write_text(
                    (temp / "log").read_text()
                )
        errors = [
            l
            for l in (temp / "log").read_text().splitlines()
            if any(s in l for s in ("WARN scene", "ERROR", "Traceback"))
            # This private fixture has no PipeWire daemon; audio is tested separately.
            and "Failed to connect pipewire context" not in l
        ]
        if errors:
            raise AssertionError("\n".join(errors))
    print(
        "Native tray discovery/exclusion/duplicate IDs/icon updates/activation/scroll/menu and MPRIS discovery/metadata/controls/disappearance passed"
    )


if __name__ == "__main__":
    main()
