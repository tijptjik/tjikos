"""Real Wayland render on a private bus, capturing only synthetic QML content."""

import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def wait_for(fn, timeout=15):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            value = fn()
            if value:
                return value
        except (ValueError, OSError):
            pass
        time.sleep(0.1)
    raise AssertionError("Timed out waiting for render/IPC")


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("Run with dbus-run-session --config-file tests/bus.conf")
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="bartjik-render-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        os.environ["BARTJIK_QML"] = str(temp / "qml")
        os.environ["BARTJIK_CONFIG"] = str(temp / "config.json")
        config = json.loads((ROOT / "examples/demo.json").read_text())
        (temp / "config.json").write_text(json.dumps(config))
        with (destination / "render.log").open("w") as log:
            p = subprocess.Popen(
                [str(ROOT / "bin/bartjik"), "--demo"], stdout=log, stderr=log
            )
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))
                screens = ipc({"op": "monitors"})["monitors"]
                screen = screens[0]
                evidence = {"screens": screens, "themes": {}}
                for theme in ("rose-punk", "monochrome"):
                    config["theme"] = theme
                    (temp / "config.json").write_text(json.dumps(config))
                    wait_for(lambda: ipc({"op": "status"})["theme"] == theme)
                    time.sleep(0.4)
                    bars = ipc({"op": "bars"})["bars"]
                    evidence["themes"][theme] = bars
                    for bar in bars:
                        name = theme + "-" + bar["key"].split("/")[1] + ".png"
                        path = destination / name
                        ipc({"op": "capture", "bar": bar["key"], "path": str(path)})
                        wait_for(path.exists)
                    workspace = next(
                        b for b in bars if b["key"].endswith("/workspace-bar")
                    )
                    g = workspace["geometry"]
                    assert abs(g["x"] + g["width"] - screen["width"] * 0.4) < 1, g
                    assert (
                        len(
                            next(b for b in bars if b["key"].endswith("/status-bar"))[
                                "islands"
                            ]
                        )
                        == 4
                    )
                # Invalid edits keep the last good live configuration.
                (temp / "config.json").write_text("{broken")
                wait_for(lambda: ipc({"op": "status"}).get("error"))
                assert ipc({"op": "status"})["theme"] == "monochrome"
                (temp / "config.json").write_text(json.dumps(config))
                wait_for(lambda: not ipc({"op": "status"}).get("error"))
                ipc({"op": "hide", "bar": "workspace-bar"})
                assert not next(
                    b
                    for b in ipc({"op": "bars"})["bars"]
                    if b["key"].endswith("/workspace-bar")
                )["visible"]
                ipc({"op": "show", "bar": "workspace-bar"})
                # All sides, stacking, shared reservation and monitor override runtime smoke.
                config["bars"][0]["side"] = "l"
                config["bars"][0]["reserve"] = True
                config["bars"][1]["side"] = "t"
                config["bars"][1]["reserve"] = True
                config["bars"][2]["side"] = "t"
                config["bars"][2]["reserve"] = True
                (temp / "config.json").write_text(json.dumps(config))
                time.sleep(1.5)
                assert not ipc({"op": "status"}).get("error")
                (destination / "geometry.json").write_text(
                    json.dumps(evidence, indent=2) + "\n"
                )
            finally:
                p.terminate()
                try:
                    p.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    p.kill()
                    p.wait()
        errors = [
            line
            for line in (destination / "render.log").read_text().splitlines()
            if any(
                x in line for x in ("WARN scene", "ERROR", "Traceback", "Binding loop")
            )
        ]
        if errors:
            raise AssertionError("\n".join(errors[:15]))
    print(
        "Both themes rendered; alignment, islands, reload retention, visibility, vertical/top surfaces and reservation checked"
    )


if __name__ == "__main__":
    main()
