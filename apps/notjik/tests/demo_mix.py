"""Exercise `notjik demo mix` with real QML and a private notification bus.

dbus-run-session --config-file tests/bus.conf -- python3 -B tests/demo_mix.py
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from native_fixture import copy_runtime, ROOT

sys.path.insert(0, str(ROOT / "tests/reference"))
from notjik import ipc
from integration import wait_for


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("A private dbus-run-session is required")
    with tempfile.TemporaryDirectory(prefix="notjik-mix-test-") as temp:
        root = Path(temp)
        copy_runtime(root)
        shutil.copytree(ROOT / "examples", root / "examples")
        shutil.copy2(ROOT / "bin/notjik", root / "bin/notjik")
        shell = root / "qml/shell.qml"
        shell.write_text(shell.read_text().replace("model: root.screens", "id: testPanels\n        model: root.screens")
                         .replace('case "status": break;', '''case "mixMetrics": return {ok:true, panels:testPanels.instances.map(p=>p.mixMetrics())};
        case "mixCapture": testPanels.instances.find(p=>p.visible).capture(request.path); return {ok:true};
        case "status": break;'''))
        panel = root / "qml/NotificationPanel.qml"
        panel.write_text(panel.read_text().replace("    id: panel", '''    id: panel
    function capture(path) { notificationScene.grabToImage(result=>result.saveToFile(path)); }
    function mixMetrics() {
        return {visible:visible, mix:demoMix, layoutHeight:layoutHeight, heightBlocked:heightBlocked,
            cards:Array.from({length:cardItems.count}, (_,i)=>cardItems.itemAt(i)).map(s=>({
                id:s.cachedNotice?.id, extent:s.extent, height:s.cardHeight, retiring:s.retiring,
                y:s.mapToItem(viewport,0,0).y}))};
    }
''', 1))
        config = {"theme":"rose-punk", "maxVisible":4, "grouping":{"enabled":True}, "pauseOnHover":False}
        config_path = root / "config.json"
        config_path.write_text(json.dumps(config))
        before = config_path.read_bytes()
        env = dict(os.environ, NOTJIK_CONFIG=str(config_path), NOTJIK_STATE_DIR=str(root / "state"))
        with (root / "qml.log").open("w+") as log:
            process = subprocess.Popen(["quickshell", "--path", str(root / "qml"), "--no-color"], env=env, stdout=log, stderr=log)
            try:
                def call(op, **kw):
                    return ipc(root, {"op":op, **kw})
                def demo(*args):
                    result = subprocess.run([str(root / "bin/notjik"), "demo", *args], env=env, text=True, capture_output=True, timeout=15)
                    assert result.returncode == 0, result.stderr
                    return result.stdout
                def metrics():
                    return next(p for p in call("mixMetrics")["panels"] if p["visible"])
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                sentinel = call("send", notice={"app":"Synthetic other sender", "title":"Keep this notice", "timeout":1000})["id"]
                wait_for(lambda: call("status")["historyCount"] == 1)
                assert "mix" in demo("list")
                assert "mix:" in demo("mix")
                wait_for(lambda: len(metrics()["cards"]) == 6 and all(c["extent"] == 1 and not c["retiring"] for c in metrics()["cards"]))
                geometry = metrics()
                assert geometry["mix"] and not geometry["heightBlocked"], geometry
                time.sleep(1.1)  # Timed notices must not expire behind the preview.
                notices = call("list")["notices"]
                demos = [n for n in notices if n["app"] == "notjik demo"]
                ids = {n["id"] for n in demos}
                assert len(ids) == 6 and any(n["id"] == sentinel for n in notices)
                assert all(n["transient"] and n["data"]["demoMix"] for n in demos)
                assert call("status")["historyCount"] == 1
                assert config_path.read_bytes() == before
                time.sleep(.3)  # Let asynchronous local artwork finish loading.
                screenshot = Path("/tmp/notjik-demo-mix.png")
                screenshot.unlink(missing_ok=True)
                call("mixCapture", path=str(screenshot))
                wait_for(screenshot.exists)
                demo("mix")
                assert {n["id"] for n in call("list")["notices"] if n["app"] == "notjik demo"} == ids
                demo("clear", "--solo")
                wait_for(lambda: len(metrics()["cards"]) == 1 and not metrics()["mix"])
                assert [n["id"] for n in call("list")["notices"]] == [sentinel]
                call("dismiss", id=sentinel)
                time.sleep(.3)
                log.flush(); log.seek(0)
                output = log.read()
                assert not any(error in output for error in ("ReferenceError", "TypeError", "Unable to assign", "Binding loop", "ERROR:", "component failed")), output
                print("PASS: six rendered cards via notjik demo mix; stable IDs; transient fixtures; normal notices and config preserved")
                print(json.dumps(geometry))
                print(screenshot)
            finally:
                process.terminate()
                process.wait(timeout=5)


if __name__ == "__main__":
    main()
