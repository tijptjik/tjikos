"""Private-bus privacy transitions using a test-only, temporary QML hook.

Does not lock the desktop or add a production lock-state override.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from integration import ROOT, wait_for
from native_fixture import copy_runtime, copy_file
from notjik import ipc


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("A private dbus-run-session is required")
    with tempfile.TemporaryDirectory(prefix="notjik-privacy-") as temp:
        root = Path(temp)
        copy_runtime(root)
        shell = root / "qml/shell.qml"
        code = shell.read_text().replace('case "status": break;', '''case "testLock": lockState = request.state; break;
        case "testPrivacy": config = Object.assign({}, config, {privacy: {lockScreen: request.policy}}); break;
        case "status": break;''')
        code = code.replace('root.lockState = response.lockState;', 'if (root.lockState === "unknown") root.lockState = response.lockState;')
        shell.write_text(code)
        os.environ["NOTJIK_CONFIG"] = str(root / "config.json")
        os.environ["NOTJIK_STATE_DIR"] = str(root / "state")
        (root / "config.json").write_text(json.dumps({"placement": {"monitors": [1]}}))
        with (root / "qml.log").open("w+") as output:
            proc = subprocess.Popen(["quickshell", "--path", str(root / "qml"), "--no-color"], stdout=output, stderr=output)
            try:
                def call(op, **kwargs):
                    return ipc(root, {"op": op, **kwargs})
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                ident = call("send", notice={"title": "Private test", "timeout": 0,
                    "actions": [{"label": "Harmless action", "command": ["true"]}]})["id"]
                call("navigate", direction="next")
                assert call("status")["keyboardMode"]
                call("testLock", state="locked")
                state = call("status")
                assert not state["keyboardMode"] and state["visible"] == 0
                call("testLock", state="unlocked")
                call("history", mode="show")
                call("testLock", state="locked")
                assert not call("status")["drawerOpen"]
                for policy in ("hide", "redact", "show"):
                    call("testPrivacy", policy=policy)
                    assert call("status")["visible"] == (1 if policy == "show" else 0)
                    for op, kwargs in (("history", {"mode": "show"}), ("navigate", {"direction": "next"}),
                                       ("action", {"id": ident, "index": 0})):
                        try:
                            call(op, **kwargs)
                        except ValueError as error:
                            assert "locked" in str(error)
                        else:
                            raise AssertionError(op + " should fail while locked")
                call("testPrivacy", policy="hide")
                call("testLock", state="unknown")
                assert call("status")["visible"] == 0
                call("testLock", state="locked")
                timed = call("send", notice={"title": "Wait for unlock", "timeout": 300})["id"]
                time.sleep(0.6)
                assert any(n["id"] == timed for n in call("list")["notices"])
                call("testLock", state="unlocked")
                wait_for(lambda: all(n["id"] != timed for n in call("list")["notices"]))
                call("dismiss", id="all")
                output.flush()
                output.seek(0)
                log = output.read()
                assert not any(word in log for word in ("ReferenceError", "TypeError", "Binding loop", "ERROR:")), log
                print("PASS: hide/redact/show; unknown fails closed; lock closes drawer and keyboard; actions blocked; timeouts resume after unlock")
            finally:
                proc.terminate()
                proc.wait(timeout=5)


if __name__ == "__main__":
    main()
