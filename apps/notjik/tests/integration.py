"""Explicit Wayland acceptance run on a PRIVATE bus; never changes the desktop notification service.

Run: dbus-run-session -- python3 -B tests/integration.py
Shows synthetic cards briefly on the current Wayland compositor.
"""
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(os.environ.get("NOTJIK_TEST_ROOT", str(Path(__file__).resolve().parents[1])))
sys.path.insert(0, str(ROOT))
import notjik


def wait_for(predicate, seconds=5):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            last = predicate()
            if last:
                return last
        except (ValueError, AssertionError) as error:
            last = str(error)
        time.sleep(0.05)
    raise AssertionError(f"condition not met: {last}")


def run():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("Run only with dbus-run-session (private /tmp bus)")
    with tempfile.TemporaryDirectory(prefix="notjik-acceptance-") as temp:
        path = Path(temp)
        fixture = path / "runtime"
        fixture.mkdir()
        for source in ROOT.iterdir():
            if source.name == "qml":
                shutil.copytree(source, fixture / source.name)
            else:
                (fixture / source.name).symlink_to(source)
        shell = fixture / "qml/shell.qml"
        shell.write_text(shell.read_text().replace(
            'readonly property string focusedAppId: ToplevelManager.activeToplevel?.appId || ""',
            'property string focusedAppId: ""').replace('case "status": break;',
            'case "testFocus": focusedAppId = request.appId; break;\n        case "status": break;'))
        os.environ["NOTJIK_CONFIG"] = str(path / "config.json")
        os.environ["NOTJIK_STATE_DIR"] = str(path / "state")
        os.environ["QT_ACCESSIBILITY"] = "0"
        config = {"theme": "rose-punk", "placement": {"monitors": [1], "edge": "t", "position": "r", "offsetY": 32},
                  "timeouts": {"low": 0.25, "normal": 0.4, "critical": 0}, "pauseOnHover": False, "grouping": {"enabled": False}}
        (path / "config.json").write_text(json.dumps(config))
        with (path / "qml.log").open("w+") as output:
            proc = subprocess.Popen(["quickshell", "--path", str(fixture / "qml"), "--no-color"], stdout=output, stderr=output)
            try:
                def call(op, **kw):
                    return notjik.ipc(fixture, {"op": op, **kw})
                def ids():
                    return [n["id"] for n in call("list")["notices"]]
                wait_for(lambda: call("status")["ok"])
                assert call("status")["theme"] == "rose-punk"
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                guard = subprocess.run([sys.executable, "-c", "from pathlib import Path; import notjik; raise SystemExit(notjik.launch(Path.cwd(), []))"],
                                       cwd=ROOT, capture_output=True, text=True)
                assert guard.returncode == 1 and "already has an owner" in guard.stderr, guard.stderr
                # Built-in tables work without any user component registration.
                table = {"results": [{"label": "Sectors", "value": "12",
                          "ingredients": [{"label": "New signals", "value": "3"}]}],
                         "state": "Complete", "tone": "success", "duration": "42s"}
                table_id = call("send", notice={"title": "Survey", "app": "survey",
                    "id": "report", "type": "results-table", "data": table, "timeout": 0})["id"]
                time.sleep(.5)  # Allow the real QML Loader and nested delegates to render.
                assert next(n for n in call("list")["notices"] if n["id"] == table_id)["data"] == table
                assert call("send", notice={"title": "New survey", "app": "survey",
                    "id": "report", "type": "results-table", "data": {}, "timeout": 0})["id"] == table_id
                time.sleep(.3)
                call("dismiss", id="all")
                # Sender-chosen IDs are persistent upserts, scoped by application.
                named = call("send", notice={"title":"Upload", "app":"backup", "id":"nightly", "type":"progress", "data":{"value":10}})["id"]
                original = call("list")["notices"][0]
                time.sleep(0.6)
                assert named in ids()  # default normal timeout in this test is only 400 ms
                for value in (25, 50, 100):
                    assert call("send", notice={"title":"Upload updated", "app":"backup", "id":"nightly", "type":"progress", "data":{"value":value}})["id"] == named
                current = call("list")["notices"][0]
                assert len(ids()) == 1 and current["data"]["value"] == 100
                assert current["received"] == original["received"] and current["updatedAt"] >= original["updatedAt"]
                other = call("send", notice={"title":"Other sender", "app":"other", "id":"nightly"})["id"]
                assert other != named and len(ids()) == 2
                call("dismiss", id=named)
                assert call("send", notice={"title":"Recreate", "app":"backup", "id":"nightly"})["id"] == named
                call("send", notice={"title":"Finish", "app":"backup", "id":"nightly", "timeout":200})
                wait_for(lambda: named not in ids())
                call("dismiss", id="all")
                first = call("send", notice={"title": "Until dismissed", "timeout": 0})["id"]
                time.sleep(0.6)
                assert first in ids()
                replacement = call("send", notice={"title": "Updated", "replace": first, "timeout": 0})["id"]
                assert replacement == first and len(ids()) == 1
                call("dismiss", id="all")
                call("pause")
                paused = call("send", notice={"title": "Paused timeout", "timeout": 250})["id"]
                time.sleep(0.5)
                assert paused in ids() and call("status")["visible"] == 0
                call("resume")
                wait_for(lambda: paused not in ids())
                for urgency in ("low", "normal"):
                    ident = call("send", notice={"title": urgency, "urgency": urgency})["id"]
                    wait_for(lambda: ident not in ids())
                critical = call("send", notice={"title": "critical", "urgency": "critical"})["id"]
                time.sleep(0.5)
                assert critical in ids()
                call("dismiss", id="all")
                marker = path / "action-result"
                ident = call("send", notice={"title": "Command action", "timeout": 0, "actions": [
                    {"label": "Run", "command": [sys.executable, "-c", "from pathlib import Path; import sys; Path(sys.argv[1]).write_text('clicked')", str(marker)]}]})["id"]
                call("action", id=ident, index=0)
                wait_for(marker.exists)
                assert marker.read_text() == "clicked" and ident not in ids()

                def notify(title, replaces=0, timeout=0, actions=(), hints=(), hint_count=None):
                    result = subprocess.run(["busctl", "--user", "call", "org.freedesktop.Notifications", "/org/freedesktop/Notifications",
                        "org.freedesktop.Notifications", "Notify", "susssasa{sv}i", "notjik test", str(replaces),
                        "network-wireless", title, "Synthetic body", str(len(actions)), *actions, str(len(hints) // 3 if hint_count is None else hint_count), *hints, str(timeout)],
                        check=True, capture_output=True, text=True)
                    return int(result.stdout.split()[1])

                # A healthy bridge owns chat alerts, but never call notifications.
                call("telegramBridge", active=True)
                assert call("status")["telegramBridgeActive"]
                fallback = "dbus-" + str(notify("Desktop fallback", actions=("default", "Open"),
                    hints=("desktop-entry", "s", "telegram", "category", "s", "im.received")))
                time.sleep(.1)
                assert fallback not in ids()
                assert call("status")["telegramDelivery"]["nativeReceived"] == 1
                assert call("status")["telegramDelivery"]["nativeFocusSuppressed"] == 0
                assert call("status")["telegramDelivery"]["nativeBridgeSuppressed"] == 1
                bridge_card = call("send", notice={"title":"Independent bridge card", "app":"Telegram via Notjik", "timeout":0})["id"]
                assert fallback not in ids() and bridge_card in ids()
                call_notice = "dbus-" + str(notify("Incoming call", hints=("desktop-entry", "s", "telegram", "category", "s", "call.incoming")))
                wait_for(lambda: call_notice in ids())
                call("dismiss", id="all")
                call("telegramBridge", active=False)
                fallback = "dbus-" + str(notify("Desktop fallback", hints=("desktop-entry", "s", "telegram", "category", "s", "im.received")))
                wait_for(lambda: fallback in ids())
                call("dismiss", id="all")

                # Focus policy covers both transports and clears existing Telegram cards.
                local_focus = call("send", notice={"title":"Focus check", "app":"Telegram via Notjik", "timeout":0})["id"]
                native_focus = "dbus-" + str(notify("Focus check", hints=("desktop-entry", "s", "telegram")))
                wait_for(lambda: native_focus in ids())
                call("testFocus", appId="org.telegram.desktop._account")
                wait_for(lambda: local_focus not in ids() and native_focus not in ids())
                suppressed = call("send", notice={"title":"Suppressed", "app":"Telegram via Notjik", "timeout":0})["id"]
                suppressed_native = "dbus-" + str(notify("Suppressed", hints=("desktop-entry", "s", "telegram")))
                unrelated = call("send", notice={"title":"Other app", "app":"Mail", "timeout":0})["id"]
                assert unrelated in ids() and suppressed not in ids() and suppressed_native not in ids()
                call("testFocus", appId="firefox")
                assert suppressed not in ids() and suppressed_native not in ids()
                restored = call("send", notice={"title":"Focus left", "app":"Telegram", "timeout":0})["id"]
                assert restored in ids()
                call("dismiss", id="all")

                # Live policy changes suppress both transports without losing original actions.
                blocked_marker = path / "blocked-action"
                local = call("send", notice={"title":"Caller policy", "app":"Telegram", "timeout":0,
                    "actions":[{"label":"Run", "command":["touch", str(blocked_marker)]}]})["id"]
                native_blocked = "dbus-" + str(notify("Native caller policy", actions=("default", "Open"),
                    hints=("desktop-entry", "s", "telegram")))
                wait_for(lambda: native_blocked in ids())
                config["actions"] = {"blacklist": ["TELEGRAM"]}
                (path / "config.json").write_text(json.dumps(config))
                wait_for(lambda: all(not n["actions"] for n in call("list")["notices"]))
                for ident in (local, native_blocked):
                    try:
                        call("action", id=ident, index=0)
                        raise AssertionError("Blacklisted action should be rejected")
                    except ValueError as error:
                        assert "disabled for this caller" in str(error), str(error)
                assert not blocked_marker.exists()
                assert len(ids()) == 2
                # Avatar activation is narrowly limited to Telegram's live native default action.
                with (path / "telegram-signals.log").open("w+") as signals:
                    monitor = subprocess.Popen(["gdbus", "monitor", "--session", "--dest", "org.freedesktop.Notifications"], stdout=signals, stderr=signals)
                    try:
                        time.sleep(.2)
                        call("openTelegramChat",id=native_blocked)
                        time.sleep(.2)
                    finally:
                        monitor.terminate();monitor.wait(timeout=5)
                    signals.seek(0);emitted=signals.read()
                    assert "ActionInvoked" in emitted and "default" in emitted,emitted
                wait_for(lambda:native_blocked not in ids())
                try:
                    call("openTelegramChat",id=local)
                    raise AssertionError("Local commands must not be exposed through the avatar")
                except ValueError as error:
                    assert "unavailable" in str(error),str(error)
                config["actions"] = {"blacklist": []}
                (path / "config.json").write_text(json.dumps(config))
                wait_for(lambda: all(len(n["actions"]) == 1 for n in call("list")["notices"]))
                call("action", id=local, index=0)
                wait_for(blocked_marker.exists)
                call("dismiss", id="all")

                native = notify("Standard notification", actions=("default", "Open"))
                wait_for(lambda: "dbus-" + str(native) in ids())
                notify("Standard updated", replaces=native, actions=("default", "Open"))
                wait_for(lambda: any(n["title"] == "Standard updated" for n in call("list")["notices"]))
                assert len(ids()) == 1
                notify("Standard updated", replaces=native, actions=("default", "Open updated label"))
                wait_for(lambda: call("list")["notices"][0]["actions"][0]["label"] == "Open updated label")
                with (path / "signals.log").open("w+") as signals:
                    monitor = subprocess.Popen(["gdbus", "monitor", "--session", "--dest", "org.freedesktop.Notifications"], stdout=signals, stderr=signals)
                    try:
                        time.sleep(0.2)
                        call("action", id="dbus-" + str(native), index=0)
                        time.sleep(0.2)
                    finally:
                        monitor.terminate()
                        monitor.wait(timeout=5)
                    signals.seek(0)
                    emitted = signals.read()
                    assert "ActionInvoked" in emitted and "NotificationClosed" in emitted, emitted
                timed = notify("Native 300ms", timeout=300)
                wait_for(lambda: "dbus-" + str(timed) not in ids(), seconds=2)
                progress = notify("Native progress", hints=("value", "i", "45"))
                wait_for(lambda: any(n["type"] == "progress" and n["data"]["value"] == 45 for n in call("list")["notices"]))
                artwork = notify("Native image data", hints=("image-data", "(iiibiiay)", "1", "1", "4", "true", "8", "4", "4", "255", "83", "166", "255"), hint_count=1)
                wait_for(lambda: any(n["id"] == "dbus-" + str(artwork) and n["image"].startswith("image:") for n in call("list")["notices"]))
                call("dismiss", id="all")

                # QML components are loaded from the user's config directory, outside the app.
                (path / "Issue.qml").write_text((ROOT / "examples/Issue.qml").read_text())
                config["components"] = {"issue": "Issue.qml"}
                (path / "config.json").write_text(json.dumps(config))
                wait_for(lambda: call("send", notice={"title": "Custom component", "type": "issue", "data": {"number": 184}, "timeout": 0}))
                time.sleep(0.3)
                call("dismiss", id="all")

                # Bounded queues preserve persistent cards and only display maxVisible cards.
                config.update(maxVisible=1, maxQueued=3)
                (path / "config.json").write_text(json.dumps(config))
                time.sleep(1.2)
                for index in range(3):
                    call("send", notice={"title": "Queued " + str(index), "timeout": 0})
                assert call("status")["visible"] == 1
                try:
                    call("send", notice={"title": "Overflow", "timeout": 0})
                    raise AssertionError("Persistent queue should reject new arrivals")
                except ValueError as error:
                    assert "Queue is full" in str(error)
                assert len(ids()) == 3
                notify("Native overflow")
                assert len(ids()) == 3
                call("dismiss", id="all")
                config.update(maxVisible=5, maxQueued=100)
                (path / "config.json").write_text(json.dumps(config))
                time.sleep(1.2)

                # Grouped identities survive navigation, history is distinct from raw events.
                config["grouping"] = {"enabled": True}
                (path / "config.json").write_text(json.dumps(config))
                time.sleep(1.2)
                first_grouped = call("send", notice={"title": "First grouped", "app": "Threads", "timeout": 0})["id"]
                second_grouped = call("send", notice={"title": "Second grouped", "app": "Threads", "timeout": 0})["id"]
                assert len(ids()) == 2 and call("status")["visible"] == 1
                call("navigate", direction="previous")
                assert call("status")["selectedId"] == first_grouped
                call("navigate", direction="next")
                assert call("status")["selectedId"] == second_grouped
                call("unfocus")
                wait_for(lambda: call("status")["historyCount"] > 0)
                call("history", mode="show")
                assert call("status")["drawerOpen"] and call("status")["visible"] == 0
                assert call("status")["historySelectedKey"] == ""
                call("navigate", direction="previous")
                wait_for(lambda: bool(call("status")["historySelectedKey"]))
                selected_history = call("status")["historySelectedKey"]
                call("navigate", direction="previous")
                assert call("status")["historySelectedKey"] != selected_history
                call("history", mode="hide")
                call("dismiss", id="all")
                call("history", mode="show")
                assert call("status")["drawerOpen"]
                assert call("status")["historySelectedKey"] == ""
                call("history", mode="hide")

                # Exercise all component loaders and live theme changes.
                for kind, data in (("progress", {"value": 67, "label": "285 KiB/s"}),
                                   ("status", {"state": "Connected", "tone": "success", "detail": "Wi-Fi"}),
                                   ("timer", {"seconds": 90, "label": "Next sync"})):
                    call("send", notice={"title": kind.title(), "type": kind, "data": data, "timeout": 0})
                time.sleep(0.5)
                config["theme"] = "monochrome"
                (path / "config.json").write_text(json.dumps(config))
                wait_for(lambda: call("status")["theme"] == "monochrome")
                # Invalid edits keep the last good configuration.
                (path / "config.json").write_text('{"placement":{"edge":"invalid"}}')
                wait_for(lambda: "Configuration:" in call("status")["historyError"])
                assert call("status")["theme"] == "monochrome"
                (path / "config.json").write_text(json.dumps(config))
                wait_for(lambda: not call("status")["historyError"])
                call("reload")
                time.sleep(0.3)
                wait_for(lambda: not call("status")["historyError"])
                call("dismiss", id="all")
                time.sleep(0.3)
                db = notjik.database()
                events = {row[0] for row in db.execute("SELECT event FROM events")}
                db.close()
                assert {"received", "updated", "action", "dismissed", "expired"} <= events, events
                output.flush()
                output.seek(0)
                log = output.read()
                errors = [line for line in log.splitlines() if any(word in line for word in ("ReferenceError", "TypeError", "Unable to assign", "Binding loop", "ERROR:", "component failed"))]
                assert not errors, "\n".join(errors)
                print("PASS: private-bus Wayland load; local and D-Bus sends; replacement; urgency and timeout; pause/resume; command and standard actions; close signals; image-data; custom QML; persistent queue limit; components; theme reload; invalid-config recovery; history")
            except Exception:
                output.flush()
                output.seek(0)
                print(output.read(), file=sys.stderr)
                raise
            finally:
                proc.terminate()
                proc.wait(timeout=5)


if __name__ == "__main__":
    run()
