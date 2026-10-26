"""Synthetic Wayland acceptance for the compact comparison layout and crypto popup."""

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
    with tempfile.TemporaryDirectory(prefix="bartjik-refined-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        # IPC drives hover; actual desktop pointer motion must not alter the fixture.
        for name in ("Part.qml", "CryptoPanel.qml", "PreviewHover.qml"):
            qml = temp / "qml" / name
            qml.write_text(
                qml.read_text().replace(
                    "HoverHandler {", "HoverHandler { enabled: false;"
                )
            )
        cfg = json.loads((ROOT / "examples/comparison.json").read_text())
        for b in cfg["bars"]:
            b.update(monitors=[1], autoHide=False)
        path = temp / "config.json"
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp / "qml"), BARTJIK_CONFIG=str(path))
        with (destination / "render.log").open("w") as log:
            p = subprocess.Popen(
                [str(ROOT / "bin/bartjik"), "--demo"], stdout=log, stderr=log
            )
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))
                screen = ipc({"op": "monitors"})["monitors"][0]
                for theme in ("rose-punk", "monochrome"):
                    cfg["theme"] = theme
                    path.write_text(json.dumps(cfg))
                    ipc({"op": "reload"})
                    wait_for(lambda: ipc({"op": "status"})["theme"] == theme)
                    time.sleep(0.5)
                    bars = ipc({"op": "bars"})["bars"]
                    assert len(bars) == 2
                    status = next(b for b in bars if b["key"].endswith("/status-bar"))
                    assert [i["id"] for i in status["islands"]] == ["applications", "ai", "connectivity", "audio-clock-input", "controls"]
                    assert all(i["y"] == 0 for i in status["islands"])
                    assert status["geometry"]["height"] == 44, status
                    g = status["geometry"]
                    assert g["x"] >= 0 and g["x"] + g["width"] <= screen["width"] + 0.01
                    assert (
                        abs(
                            screen["height"]
                            - g["y"]
                            - g["height"]
                            - 60 / screen["scale"]
                        )
                        < 0.01
                    )
                    for b in bars:
                        image = destination / (
                            theme + "-" + b["key"].split("/")[1] + ".png"
                        )
                        ipc({"op": "capture", "bar": b["key"], "path": str(image)})
                        wait_for(
                            lambda: image.exists()
                            and image.read_bytes().endswith(b"IEND\xaeB`\x82")
                        )
                workspace = next(b for b in bars if b["key"].endswith("/workspace-bar"))
                assert [i["id"] for i in workspace["islands"]] == ["markets", "media", "workspaces"], workspace

                def hover(pair="BTC/USD", part="icon", leave=False, x=700):
                    return ipc(
                        {
                            "op": "cryptoHoverTest",
                            "module": "prices",
                            "item": pair,
                            "part": part,
                            "leave": leave,
                            "x": x,
                        }
                    )

                def state():
                    return ipc({"op": "cryptoState"})

                hover()
                time.sleep(0.1)
                assert not state()["opened"]
                hover(leave=True)
                time.sleep(0.35)
                assert not state()["opened"]
                hover()
                hover(part="text")
                hover(leave=True)  # stale icon leave must not cancel text dwell
                wait_for(lambda: state()["opened"])
                assert state()["pair"] == "BTC/USD" and not state()["pinned"]
                assert abs(state()["anchorX"] + state()["width"] / 2 - 710) < 1
                hover("SOL/USD", x=900)
                time.sleep(0.1)
                wait_for(lambda: state()["pair"] == "SOL/USD")
                assert abs(state()["anchorX"] + state()["width"] / 2 - 910) < 1
                hover("SOL/USD", leave=True)
                wait_for(lambda: not state()["opened"])
                for pair in ("BTC/USD", "SOL/USD", "AR/USD"):
                    ipc({"op": "cryptoPreview", "module": "prices", "item": pair})
                    wait_for(lambda: ipc({"op": "cryptoState"})["points"] == 288)
                    assert ipc({"op": "cryptoState"})["pair"] == pair
                    assert ipc({"op": "cryptoState"})["pinned"]
                image = destination / "crypto-panel.png"
                ipc({"op": "captureCrypto", "path": str(image)})
                wait_for(
                    lambda: image.exists()
                    and image.read_bytes().endswith(b"IEND\xaeB`\x82")
                )
                # Force outer alignment: renderer must preserve the edge-matched gutter.
                for b in cfg["bars"]:
                    b["alignment"] = b["position"]
                path.write_text(json.dumps(cfg))
                ipc({"op": "reload"})
                time.sleep(0.5)
                for b in ipc({"op": "bars"})["bars"]:
                    g = b["geometry"]
                    gap = (
                        g["x"]
                        if b["key"].endswith("workspace-bar")
                        else screen["width"] - g["x"] - g["width"]
                    )
                    assert abs(gap - 60 / screen["scale"]) < 0.01, (b["key"], gap)
            finally:
                p.terminate()
                p.wait(timeout=5)
        errors = [
            l
            for l in (destination / "render.log").read_text().splitlines()
            if any(k in l for k in ("WARN scene", "ERROR", "Traceback", "Binding loop"))
        ]
        assert not errors, errors[:20]
    print(
        "Compact one-row layout, 60px offset, 40% centre, both themes and three selected crypto charts rendered successfully"
    )


if __name__ == "__main__":
    main()
