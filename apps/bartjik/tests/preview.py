"""Synthetic Wayland preview timing, sliding, cancellation and screenshot acceptance."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from render import wait_for

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("Private bus required")
    destination = Path(sys.argv[1])
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="bartjik-preview-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        # Pointer injection belongs only to the temporary fixture, never production IPC.
        shell = temp / "qml/shell.qml"
        source = shell.read_text().replace(
            "if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}",
            "if(msg.kind==='pointer'){}",
        )
        source = source.replace(
            "case 'status':break;",
            "case 'previewPointer':root.pointer=req.pointer;return {ok:true};\n        case 'status':break;",
        )
        shell.write_text(source)
        cfg = json.loads((ROOT / "examples/comparison.json").read_text())
        cfg["bars"] = cfg["bars"][:1]
        cfg["bars"][0].update(monitors="all", autoHide=False)
        path = temp / "config.json"
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp / "qml"), BARTJIK_CONFIG=str(path))
        with (destination / "preview.log").open("w") as log:
            p = subprocess.Popen(
                [str(ROOT / "bin/bartjik"), "--demo"], stdout=log, stderr=log
            )
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))

                def state():
                    return ipc({"op": "workspacePreviewState"})["preview"]

                def enter(item, x=400):
                    return ipc(
                        {
                            "op": "workspacePreviewTest",
                            "module": "spaces",
                            "item": item,
                            "x": x,
                        }
                    )

                def leave(item):
                    return ipc(
                        {
                            "op": "workspacePreviewTest",
                            "module": "spaces",
                            "item": item,
                            "x": 400,
                            "leave": True,
                        }
                    )

                enter("1")
                time.sleep(0.1)
                assert not state()["opened"]
                leave("1")
                time.sleep(0.35)
                assert not state()["opened"]
                enter("2")
                time.sleep(0.1)
                assert not state()["opened"]
                wait_for(lambda: state()["opened"])
                assert state()["key"].endswith("/2") and state()["windows"] == 2
                screen = ipc({"op": "monitors"})["monitors"][0]
                expected_width = min(20 + (state()["height"] - 20) * screen["width"] / screen["height"], screen["width"] - 24)
                assert abs(state()["width"] - expected_width) < 1
                wait_for(lambda: abs(state()["card"]["width"] - expected_width) < 1)
                first = state()["card"]["x"]
                target = 710 - state()["width"] / 2
                enter("3", 700)
                wait_for(lambda: state()["key"].endswith("/3"))
                intermediate = state()["card"]["x"]
                assert first <= intermediate < target, (first, intermediate)
                wait_for(lambda: abs(state()["card"]["x"] - target) < 1)
                # A delayed leave from the previous item must not hide the current preview.
                leave("2")
                time.sleep(0.2)
                assert state()["opened"]
                screen = ipc({"op": "monitors"})["monitors"][0]
                ipc(
                    {
                        "op": "previewPointer",
                        "pointer": dict(
                            valid=True,
                            monitor=screen["name"],
                            x=800,
                            y=screen["height"] - 75,
                        ),
                    }
                )
                wait_for(
                    lambda: abs(state()["card"]["x"] - (800 - state()["width"] / 2)) < 1
                )
                image = destination / "workspace-preview.png"
                ipc({"op": "captureWorkspace", "path": str(image)})
                wait_for(
                    lambda: image.exists()
                    and image.read_bytes().endswith(b"IEND\xaeB`\x82")
                )
                leave("3")
                wait_for(lambda: not state()["opened"])
                assert state()["windows"] == 0
                screens = ipc({"op": "monitors"})["monitors"]
                if len(screens) > 1:
                    enter("1")
                    wait_for(lambda: state()["opened"])
                    other = screens[1]["name"]
                    ipc(
                        {
                            "op": "workspacePreviewTest",
                            "module": "spaces",
                            "item": "2",
                            "x": 300,
                            "monitor": other,
                        }
                    )
                    time.sleep(0.1)
                    assert not state()["opened"] and state()["pending"]
                    wait_for(lambda: state()["opened"])
                    assert state()["key"].startswith(other + "/")
                    leave("1")
                    time.sleep(0.2)
                    assert state()["opened"]
                # CLI hiding a bar also closes its preview and releases captures.
                ipc({"op": "hide", "bar": "workspace-bar"})
                wait_for(lambda: not state()["opened"])
                # Live disable unloads the native preview component, timers and captures.
                cfg["modules"]["spaces"]["preview"]["enabled"] = False
                path.write_text(json.dumps(cfg))
                ipc({"op": "reload"})
                wait_for(
                    lambda: not ipc({"op": "config"})["config"]["modules"]["spaces"][
                        "preview"
                    ]["enabled"]
                )
                assert state() == {"opened": False}
            finally:
                p.terminate()
                p.wait(timeout=5)
        errors = [
            l
            for l in (destination / "preview.log").read_text().splitlines()
            if any(k in l for k in ("WARN scene", "ERROR", "Traceback", "Binding loop"))
        ]
        assert not errors, errors[:20]
    print(
        "Workspace preview passed: 300ms dwell, cancellation, animated follow, stale-leave isolation, capture cleanup and live disable"
    )


if __name__ == "__main__":
    main()
