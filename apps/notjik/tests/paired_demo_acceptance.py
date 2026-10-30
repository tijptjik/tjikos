"""Isolated two-server demo acceptance. Requires the optional Dunst reference."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from integration import ROOT, wait_for
import comparison
from demo import TYPES
from notjik import ipc


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("Run with dbus-run-session --config-file tests/bus.conf")
    monitor=json.loads(subprocess.check_output(["hyprctl","-j","monitors"],text=True))[0]["name"]
    display = os.environ["WAYLAND_DISPLAY"]
    if not display.startswith("/"):
        os.environ["WAYLAND_DISPLAY"] = str(Path(os.environ["XDG_RUNTIME_DIR"]) / display)
    with tempfile.TemporaryDirectory(prefix="njp-") as temp:
        root = Path(temp)
        os.environ["XDG_RUNTIME_DIR"] = str(root)
        os.environ["NOTJIK_CONFIG"] = str(root / "base.json")
        (root / "base.json").write_text(json.dumps({"placement":{"primaryMonitor":monitor}}))
        def demo(*args):
            result = subprocess.run([str(ROOT / "bin/notjikctl"), "demo", *args], capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, result.stderr + result.stdout
            return result.stdout
        try:
            assert "standard:" in demo("next")
            assert "title-only:" in demo("next")
            assert "Dunst: left" in demo("standard")
            session = comparison.state()
            def call(op, **kw):
                return ipc(Path(session["app"]), {"op":op, **kw})
            def dunst_count():
                return int(subprocess.check_output(["busctl", "--address="+session["buses"]["dunst"], "get-property",
                    "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.dunstproject.cmd0", "displayedLength"], text=True).split()[1])
            wait_for(lambda: dunst_count() == 1)
            first = json.loads((comparison.runtime_dir()/"demo-ids.json").read_text())["ids"]
            demo("standard", "--append")
            assert json.loads((comparison.runtime_dir()/"demo-ids.json").read_text())["ids"] == first
            assert len(call("list")["notices"]) == 1 and dunst_count() == 1
            demo("actions-wrapped")  # Must return without waiting for action selection.
            assert len(call("list")["notices"][0]["actions"]) == 4
            assert "resident:" in demo("next")
            assert dunst_count() == 1
            assert len(call("list")["notices"]) == 1
            assert "static time" in demo("timer")
            demo("update")
            assert len(call("list")["notices"]) == 1
            assert call("list")["notices"][0]["data"]["value"] == 100
            assert dunst_count() == 1
            assert "grouped history/count is notjik-only" in demo("grouped")
            assert call("status")["visible"] == 1 and len(call("list")["notices"]) == 3
            wait_for(lambda: dunst_count() == 3)
            gallery = demo("all", "--interval", "30ms")
            for index, name in enumerate(TYPES, 1):
                assert f"{index}/{len(TYPES)} {name}:" in gallery
            demo("stack")
            assert len(call("list")["notices"]) == 4
            wait_for(lambda: dunst_count() == 4)
            assert not call("status")["groupingEnabled"]
            # Independent manual fixtures on both private servers must survive demo cleanup.
            sentinel = call("send", notice={"app":"Manual reference", "title":"Keep me", "timeout":0})["id"]
            comparison.demo_notify(session, {"app":"Manual reference", "title":"Keep me", "timeout":0})
            demo("clear")
            assert [n["id"] for n in call("list")["notices"]] == [sentinel]
            wait_for(lambda: dunst_count() == 1)
            output = (comparison.runtime_dir()/"session.log").read_text()
            assert not any(word in output for word in ("ReferenceError", "TypeError", "Binding loop", "ERROR:", "Unable to assign")), output
            print(f"PASS: both servers; all {len(TYPES)} demos; native action labels without waiting; mapped replacement IDs; grouped baseline; safe paired cleanup")
        finally:
            comparison.stop()


if __name__ == "__main__":
    main()
