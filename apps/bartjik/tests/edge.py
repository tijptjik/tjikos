"""Real Wayland fade/mask acceptance; pointer samples injected only into a private fixture."""

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
        raise RuntimeError("Run on a private bus")
    with tempfile.TemporaryDirectory(prefix="bartjik-edge-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        shell = temp / "qml/shell.qml"
        source = shell.read_text().replace(
            "if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}",
            "if(msg.kind==='pointer'){}",
        )
        source = source.replace(
            "case 'reload':",
            "case 'testPointer':root.pointer=req.pointer;root.pointerUpdated=Date.now()+60000;return {ok:true};\n        case 'reload':",
        )
        shell.write_text(source)
        config = json.loads((ROOT / "examples/comparison.json").read_text())
        config["bars"] = config["bars"][:1]
        config["bars"][0].update(
            monitors=[1], activationDelay=500, hideDelay=1000,
            fadeDuration=400, offsetY=-300, keepVisibleDistance="420px"
        )
        path = temp / "config.json"
        path.write_text(json.dumps(config))
        os.environ.update(BARTJIK_CONFIG=str(path), BARTJIK_QML=str(temp / "qml"))
        with (temp / "log").open("w") as log:
            process = subprocess.Popen(
                [str(ROOT / "bin/bartjik"), "--demo"], stdout=log, stderr=log
            )
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))
                screen = ipc({"op": "monitors"})["monitors"][0]
                height = screen["height"]

                def move(distance, monitor=None):
                    return ipc(
                        {
                            "op": "testPointer",
                            "pointer": dict(
                                valid=True,
                                monitor=monitor or screen["name"],
                                x=10,
                                y=height - distance,
                            ),
                        }
                    )

                def bar():
                    return ipc({"op": "bars"})["bars"][0]

                move(600)
                wait_for(lambda: bar()["opacity"] == 0)
                assert bar()["inputRegions"] == 0
                move(1, "other-monitor")
                time.sleep(0.6)
                assert bar()["concealed"]
                move(1)
                time.sleep(0.2)
                assert bar()["concealed"]
                move(300)  # cancel short activation
                time.sleep(0.4)
                assert bar()["concealed"]
                move(1)
                wait_for(lambda: 0 < bar()["opacity"] < 1)
                wait_for(lambda: bar()["opacity"] == 1)
                assert bar()["inputRegions"] > 0
                move(340)  # travel from edge to offset bar
                time.sleep(1.2)
                assert bar()["opacity"] == 1
                g = bar()["geometry"]
                assert abs((height - g["y"] - g["height"]) * screen["scale"] - 300) < 1
                move(600)
                time.sleep(0.4)
                assert not bar()["concealed"]
                wait_for(lambda: bar()["concealed"] and 0 < bar()["opacity"] < 1)
                move(340)  # reverse an in-progress fade
                wait_for(lambda: bar()["opacity"] == 1)
                move(600)
                wait_for(lambda: bar()["opacity"] == 0)
                assert bar()["inputRegions"] == 0
                config["bars"][0].update(activationDelay=0, hideDelay=0, fadeDuration=0)
                path.write_text(json.dumps(config))
                ipc({"op": "reload"})
                wait_for(lambda: ipc({"op": "config"})["bars"][0]["hideDelay"] == 0)
                move(1)
                wait_for(lambda: bar()["opacity"] == 1, timeout=1)
                move(600)
                wait_for(lambda: bar()["opacity"] == 0, timeout=1)
                config["bars"][0]["autoHide"] = False
                path.write_text(json.dumps(config))
                ipc({"op": "reload"})
                wait_for(lambda: bar()["opacity"] == 1)
                # Exercise the real shared controller on every available screen.
                screens = ipc({"op": "monitors"})["monitors"]
                if len(screens) < 2:
                    raise AssertionError("Cross-monitor acceptance requires two screens")
                config["bars"][0].update(
                    monitors="all", autoHide=True, syncMonitors=True,
                    activationDelay=0, hideDelay=0, fadeDuration=0,
                )
                primary = dict(config["bars"][0], id="primary-only", monitors=[1],
                               position="r", islands=[dict(id="clock", modules=["time"])])
                config["bars"].append(primary)
                path.write_text(json.dumps(config))
                ipc({"op": "reload"})
                wait_for(lambda: len(ipc({"op": "bars"})["bars"]) == len(screens) + 1)

                def all_bars():
                    return ipc({"op": "bars"})["bars"]

                def on_screen(target, distance):
                    ipc({"op": "testPointer", "pointer": dict(
                        valid=True, monitor=target["name"], x=10,
                        y=target["height"] - distance,
                    )})

                on_screen(screen, screen["height"])
                wait_for(lambda: all(b["opacity"] == 0 for b in all_bars()))
                for target in screens:
                    on_screen(target, 1)
                    wait_for(lambda: all(b["opacity"] == 1 for b in all_bars()))
                    on_screen(target, 100 / target["scale"])
                    time.sleep(0.15)
                    assert all(b["opacity"] == 1 for b in all_bars())
                on_screen(screen, screen["height"])
                wait_for(lambda: all(b["opacity"] == 0 and b["inputRegions"] == 0 for b in all_bars()))
                for definition in config["bars"]:
                    definition["syncMonitors"] = False
                path.write_text(json.dumps(config))
                ipc({"op": "reload"})
                wait_for(lambda: not ipc({"op": "config"})["bars"][0]["syncMonitors"])
                for target in screens:
                    on_screen(target, 1)
                    wait_for(lambda: all(
                        b["opacity"] == (1 if b["key"].startswith(target["name"] + "/") else 0)
                        for b in all_bars()
                    ))
                for definition in config["bars"]:
                    definition["syncMonitors"] = True
                path.write_text(json.dumps(config))
                ipc({"op": "reload"})
                wait_for(lambda: all(b["opacity"] == 1 for b in all_bars()))
                ipc({"op": "hide", "bar": "all"})
                wait_for(lambda: all(not b["visible"] for b in all_bars()))
                ipc({"op": "show", "bar": "all"})
                wait_for(lambda: all(b["visible"] and b["opacity"] == 1 for b in all_bars()))
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [
            line
            for line in (temp / "log").read_text().splitlines()
            if any(
                k in line for k in ("WARN scene", "ERROR", "Traceback", "Binding loop")
            )
        ]
        assert not errors, errors[:10]
    print(
        "Wayland edge acceptance passed: dwell cancellation, 300px offset, safe traversal, hide delay, fade reversal, input masks, cross-monitor activation, opt-out, CLI visibility and live reload"
    )


if __name__ == "__main__":
    main()
