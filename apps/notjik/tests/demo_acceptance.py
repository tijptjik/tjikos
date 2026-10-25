"""Exercise the public demo CLI on a private bus with synthetic data only."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from integration import ROOT, wait_for
from notjik import ipc


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("Use dbus-run-session --config-file tests/bus.conf")
    with tempfile.TemporaryDirectory(prefix="notjik-demo-test-") as temp:
        path = Path(temp)
        os.environ["NOTJIK_CONFIG"] = str(path / "config.json")
        os.environ["NOTJIK_STATE_DIR"] = str(path / "state")
        (path / "config.json").write_text(json.dumps({"theme":"rose-punk", "pauseOnHover":False}))
        with (path / "qml.log").open("w+") as log:
            process = subprocess.Popen(["quickshell", "--path", str(ROOT / "qml"), "--no-color"], stdout=log, stderr=log)
            try:
                def call(op, **kw):
                    return ipc(ROOT, {"op":op, **kw})
                def demo(*args):
                    result = subprocess.run([sys.executable, str(ROOT / "bin/notjikctl"), "demo", "--solo", *args], capture_output=True, text=True)
                    assert result.returncode == 0, result.stderr
                    return result.stdout
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                sentinel = call("send", notice={"title":"Other sender must survive", "app":"Other test", "timeout":0})["id"]
                wait_for(lambda: call("status")["historyCount"] == 1)
                assert "actions-wrapped" in demo("list")
                gallery = demo("all", "--interval", "30ms")
                assert "36/36 telegram-grouped:" in gallery, gallery
                notices = call("list")["notices"]
                assert len(notices) == 4 and any(n["id"] == sentinel for n in notices), notices
                wait_for(lambda: call("status")["visible"] == 2)  # sentinel plus grouped Telegram messages
                time.sleep(.4)
                assert call("status")["historyCount"] == 1  # default demos do not pollute history
                demo("actions")
                action = next(n for n in call("list")["notices"] if n["app"] == "notjik demo")
                call("action", id=action["id"], index=0)
                assert len(call("list")["notices"]) == 1
                demo("grouped", "--log")
                wait_for(lambda: call("status")["historyCount"] == 4)
                assert call("status")["visible"] == 2  # sentinel plus one three-member group
                demo("clear")
                assert [n["id"] for n in call("list")["notices"]] == [sentinel]
                demo("icon", "--timeout", "200ms")
                wait_for(lambda: len(call("list")["notices"]) == 1)
                call("history", mode="show")
                demo("standard")
                assert not call("status")["drawerOpen"]
                demo("clear")
                call("dismiss", id=sentinel)
                time.sleep(.4)
                log.flush(); log.seek(0)
                output = log.read()
                assert not any(error in output for error in ("ReferenceError", "TypeError", "Unable to assign", "Binding loop", "ERROR:", "component failed", "Cannot open")), output
                print("PASS: all 36 demo types via CLI; safe cleanup; stable updates; harmless actions; transient history; grouping; timed expiry; drawer transition")
            finally:
                process.terminate(); process.wait(timeout=5)


if __name__ == "__main__":
    main()
