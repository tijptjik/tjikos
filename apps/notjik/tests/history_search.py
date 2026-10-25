"""Exercise real drawer key events using synthetic history on an isolated bus.

Run with dbus-run-session --config-file tests/bus.conf -- python3 -B tests/history_search.py
Only a temporary QML copy receives test IPC and QtTest key injection.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from notjik import ipc
from integration import wait_for


def run():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("A private dbus-run-session is required")
    with tempfile.TemporaryDirectory(prefix="notjik-search-") as temp:
        root = Path(temp)
        for name in ("qml", "themes"):
            shutil.copytree(ROOT / name, root / name)
        for name in ("notjik.py", "lock_watch.py"):
            shutil.copy(ROOT / name, root / name)
        shell = root / "qml/shell.qml"
        shell.write_text(shell.read_text().replace("HistoryDrawer { controller: root }", "HistoryDrawer { id: historyDrawer; controller: root }").replace('case "status": break;', '''case "searchState": return {ok:true, query:historyQuery, editing:historyDrawer.searchEditing(),
            open:drawerOpen, rows:historyRows, matches:filteredHistoryItems.map(n=>n.historyKey),
            selected:historySelectedKey, live:notices.length, groups:expandedGroups};
        case "searchKey": historyDrawer.searchKey(request.key, request.control || false); return {ok:true};
        case "searchFocus": historyDrawer.focusHistory(); return {ok:true};
        case "searchGroup": toggleGroup(request.key); return {ok:true};
        case "searchCapture": historyDrawer.capture(request.path); return {ok:true};
        case "status": break;'''))
        drawer = root / "qml/HistoryDrawer.qml"
        drawer.write_text(drawer.read_text().replace("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replace("    id: drawer", '''    id: drawer
    function searchEditing() { return searchInput.activeFocus; }
    function searchKey(key, control) {
        const modifiers = control ? Qt.ControlModifier : Qt.NoModifier;
        if (key.length === 1) testKeys.keyClickChar(key, modifiers, 0);
        else testKeys.keyClick(Qt["Key_" + key], modifiers, 0);
    }
    function capture(path) { drawerContent.grabToImage(result => result.saveToFile(path)); }''', 1)
            .replace("id: drawerContent", "id: drawerContent\n        TestEvent { id: testKeys }", 1)
            .replace("WlrKeyboardFocus.OnDemand", "WlrKeyboardFocus.None")
            .replace("else if (drawer.visible && drawer.hadWindowFocus)", "else if (false)")
            .replace("mask: Region { item: backdrop; radius: backdrop.radius }", "mask: Region { width: 0; height: 0 }"))
        env = dict(os.environ, NOTJIK_CONFIG=str(root / "config.json"), NOTJIK_STATE_DIR=str(root / "state"), QT_ACCESSIBILITY="0")
        (root / "config.json").write_text(json.dumps({"theme":"rose-punk", "placement":{"monitors":[1]}, "pauseOnHover":False,
            "history":{"maxAgeDays":7, "maxEvents":0, "drawerLimit":0}}))
        with (root / "qml.log").open("w+") as log:
            process = subprocess.Popen(["quickshell", "--path", str(root / "qml"), "--no-color"], env=env, stdout=log, stderr=log)
            try:
                def call(op, **kw):
                    return ipc(root, {"op":op, **kw})
                def state():
                    return call("searchState")
                def key(value, control=False):
                    call("searchKey", key=value, control=control)
                def type_text(value):
                    for char in value:
                        key(char)
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                for app, title, body in [("Telegram Desktop", "Weekend plans", "Meet at Central / pier"),
                                         ("Telegram Desktop", "Work", "Build is ready"),
                                         ("Mail", "Central booking", "Confirmed")]:
                    call("send", notice={"app":app, "title":title, "body":body, "timeout":0})
                wait_for(lambda: call("status")["historyCount"] == 3)
                call("history", mode="show")
                call("searchFocus")
                assert len(state()["rows"]) == 2, state()
                key("/")
                assert state()["editing"] and state()["query"] == "", state()
                type_text("CENTRAL")
                result = state()
                assert result["query"] == "CENTRAL" and len(result["matches"]) == 2, result
                assert len(result["rows"]) == 4, result
                key("Tab")
                assert state()["selected"] == result["matches"][0] and state()["editing"], state()
                key("Tab")
                assert state()["selected"] == result["matches"][1] and state()["editing"], state()
                key("Tab")
                assert state()["selected"] == result["matches"][0], state()
                key("Backtab")
                assert state()["selected"] == result["matches"][1] and state()["editing"], state()
                assert all(row["count"] == 1 for row in result["rows"] if row["kind"] == "group"), result
                group = result["rows"][0]["key"]
                call("searchGroup", key=group)
                assert len(state()["rows"]) == 3
                call("searchGroup", key=group)
                key("Down")
                selected = state()["selected"]
                assert selected in result["matches"] and state()["editing"]
                key("Down")
                assert state()["selected"] in result["matches"] and state()["selected"] != selected
                key("/")
                assert state()["query"] == "CENTRAL/" and state()["matches"] == [] and state()["selected"] == "", state()
                key("Down")
                key("Tab")
                key("Backtab")
                assert state()["selected"] == "" and state()["editing"]
                key("Backspace")
                assert len(state()["matches"]) == 2 and state()["live"] == 3, state()
                key("Home")
                key("Delete")
                assert state()["query"] == "ENTRAL" and state()["live"] == 3, state()
                key("a", True)
                key("Backspace")
                key("Backspace")
                key("Delete")
                assert state()["query"] == "" and state()["live"] == 3, state()
                type_text("central")
                key("Return")
                assert not state()["editing"] and state()["query"] == "central" and state()["selected"] in state()["matches"], state()
                selected = state()["selected"]
                key("Tab")
                assert state()["selected"] != selected and not state()["editing"], state()
                key("Backtab")
                assert state()["selected"] == selected and not state()["editing"], state()
                key("/")
                assert state()["editing"]
                type_text("telegram")
                assert len(state()["matches"]) == 2 and state()["query"] == "telegram", state()
                type_text(" central")
                assert len(state()["matches"]) == 1, state()
                screenshot = Path(os.environ.get("NOTJIK_SEARCH_CAPTURE", "/tmp/notjik-history-search.png"))
                call("searchCapture", path=str(screenshot))
                wait_for(screenshot.exists)
                key("Escape")
                assert state()["open"] and state()["query"] == "" and not state()["editing"], state()
                assert len(state()["rows"]) == 2 and state()["groups"] == {}, state()
                key("Escape")
                assert not state()["open"], state()
                call("history", mode="show")
                call("searchFocus")
                key("/")
                type_text("missing")
                assert not state()["matches"]
                call("history", mode="hide")
                call("history", mode="show")
                assert state()["query"] == "" and len(state()["matches"]) == 3 and not state()["editing"], state()
                print("PASS: slash, typing, literal slash, editing, filtered navigation, Tab/Shift+Tab, counts, groups, Escape, reopening")
            finally:
                process.terminate()
                process.wait(timeout=5)
                log.seek(0)
                output = log.read()
                problems = [line for line in output.splitlines() if " ERROR" in line or "ReferenceError" in line or "TypeError" in line or "Binding loop" in line]
                if problems:
                    print(output, file=sys.stderr)
                    raise AssertionError("QML runtime errors")
                if sys.exc_info()[0]:
                    print(output, file=sys.stderr)


if __name__ == "__main__":
    run()
