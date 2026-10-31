"""Real QML Shift-click and Wayland clipboard acceptance with synthetic cards.

Run: dbus-run-session --config-file apps/notjik/tests/bus.conf -- python3 -B apps/notjik/tests/clipboard.py
The notification server uses a private bus. Original clipboard bytes are restored.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
from notjik import ipc
from integration import wait_for
from native_fixture import copy_runtime


def run():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("A private dbus-run-session is required")
    mime = subprocess.run(["wl-paste", "--list-types"], capture_output=True, check=False).stdout.splitlines()
    mime = mime[0].decode() if mime else None
    original = subprocess.check_output(["wl-paste", "--no-newline", "--type", mime]) if mime else b""
    with tempfile.TemporaryDirectory(prefix="notjik-copy-test-") as temp:
        root = Path(temp)
        copy_runtime(root)
        shell = root / "qml/shell.qml"
        shell.write_text(shell.read_text().replace("model: root.screens", "id: popupPanels\n        model: root.screens")
            .replace('case "status": break;', '''case "copyTest": {
                const card = popupPanels.instances[0].testCard();
                if (!card) return {ok:true,ready:false};
                card.testClick(request);
                return {ok:true,ready:true,feedback:card.testFeedback(),selected:historySelectedKey};
            }
            case "status": break;'''))
        panel = root / "qml/NotificationPanel.qml"
        panel.write_text(panel.read_text().replace("    id: panel", '''    id: panel
    function testCard() { return cardItems.count ? cardItems.itemAt(0).testCard : null; }''', 1)
            .replace("property var cachedNotice: null", "property var cachedNotice: null\n                        property alias testCard: card"))
        card = root / "qml/NotificationCard.qml"
        card.write_text(card.read_text().replace("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replace("    id: card", '''    id: card
    TestEvent { id: events }
    function testFeedback() { return copyFeedback.running; }
    function testClick(request) {
        if (request.mode === "history") historyMode = true;
        if (request.mode === "floating") floating = true;
        card.controller.selectedId = notice.id;
        card.controller.keyboardMode = true;
        let point = Qt.point(width / 2, 30);
        if (request.target === "close") point = closeButton.mapToItem(card, closeButton.width/2, closeButton.height/2);
        if (request.target === "action") {
            function findButton(item) {
                if (item instanceof T.AbstractButton && item.text === "Run") return item;
                for (const child of item.children) { const found = findButton(child); if (found) return found; }
                return null;
            }
            const button = findButton(actionFooter);
            if (!button || !button.visible || !button.enabled) throw Error("Action not ready");
            point = button.mapToItem(card, button.width/2, button.height/2);
        }
        const modifiers = (request.shift ? Qt.ShiftModifier : Qt.NoModifier) | (request.control ? Qt.ControlModifier : Qt.NoModifier);
        events.mouseMove(card, point.x, point.y, 0, Qt.NoButton, modifiers);
        if (!request.hover) events.mouseClick(card, point.x, point.y, Qt.LeftButton, modifiers, 0);
    }''', 1))
        (root / "config.json").write_text(json.dumps({"theme":"rose-punk", "placement":{"monitors":[1]},
            "grouping":{"enabled":False}, "stack":{"animationMs":0}}))
        env = dict(os.environ, NOTJIK_CONFIG=str(root / "config.json"), NOTJIK_STATE_DIR=str(root / "state"), QT_ACCESSIBILITY="0")
        with (root / "qml.log").open("w+") as log:
            process = subprocess.Popen(["quickshell", "--path", str(root / "qml"), "--no-color"], env=env, stdout=log, stderr=log)
            try:
                def call(op, **kw):
                    return ipc(root, {"op":op, **kw})
                def live(ident):
                    return any(n["id"] == ident for n in call("list")["notices"])
                def click(**kw):
                    return call("copyTest", **kw)
                def clipboard():
                    return subprocess.check_output(["wl-paste", "--no-newline"]).decode()
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                body = "Saved /tmp/synthetic_recording.mp4\n\n" + "Full untruncated body. " * 30
                for mode in ("popup", "floating", "history"):
                    ident = call("send", notice={"title":"Clipboard acceptance", "body":body, "timeout":0})["id"]
                    wait_for(lambda: click(hover=True).get("ready"))
                    time.sleep(.25)
                    response = click(shift=True, control=True, mode=mode)
                    assert response["feedback"] and live(ident), response
                    assert clipboard() == body.strip()
                    assert not response["selected"]
                    if mode == "floating":
                        click(shift=True, target="close")
                        assert live(ident)
                    call("dismiss", id="all")
                    time.sleep(.3)
                marker = root / "action"
                ident = call("send", notice={"title":"Action acceptance", "body":"Copy, keep open", "timeout":0,
                    "resident":True, "actions":[{"label":"Run", "command":["touch",str(marker)]}]})["id"]
                wait_for(lambda: click(hover=True).get("ready"))
                time.sleep(.3)
                click(shift=True, target="action")
                time.sleep(.2)
                assert clipboard() == "Copy, keep open" and live(ident) and not marker.exists()
                click(target="action")
                wait_for(marker.exists)
                assert live(ident)
                click()
                wait_for(lambda: not live(ident))
                print("QML Shift-click: popup, floating, history, Ctrl priority, close/action protection, normal action/dismissal, and Wayland clipboard passed")
            except Exception:
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                process.terminate()
                process.wait(timeout=5)
                if mime:
                    subprocess.run(["wl-copy", "--type", mime], input=original, check=True)
                else:
                    subprocess.run(["wl-copy", "--clear"], check=True)


if __name__ == "__main__":
    run()
