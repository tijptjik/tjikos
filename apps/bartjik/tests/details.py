"""Synthetic Wayland acceptance for status cards and their persistent shell."""

import json
import os
import re
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


def committed_shell_regions(trace):
    """Input regions are double buffered; only their state at commit applies."""
    regions, surfaces, commits = {}, {}, []
    for line in trace.splitlines():
        if match := re.search(r'get_layer_surface\([^,]+, wl_surface[#@](\d+),.*"bartjik-panel-shell"', line):
            surfaces[match[1]] = None
        if match := re.search(r'create_region\(new id wl_region[#@](\d+)', line):
            regions[match[1]] = []
        if match := re.search(r'wl_region[#@](\d+)\.add\(([^)]+)', line):
            regions[match[1]].append(tuple(map(int, match[2].split(","))))
        if match := re.search(r'wl_surface[#@](\d+)\.set_input_region\(([^)]+)', line):
            if match[1] in surfaces:
                region = re.search(r'wl_region[#@](\d+)', match[2])
                surfaces[match[1]] = list(regions[region[1]]) if region else None
        if match := re.search(r'wl_surface[#@](\d+)\.commit\(', line):
            if match[1] in surfaces:
                commits.append((match[1], surfaces[match[1]]))
    return commits


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("Private bus required")
    destination = Path(sys.argv[1])
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="bartjik-refined-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        # IPC drives hover; actual desktop pointer motion must not alter the fixture.
        for name in (
            "Part.qml",
            "CryptoPanel.qml",
            "PreviewHover.qml",
            "DetailPanel.qml",
        ):
            qml = temp / "qml" / name
            qml.write_text(
                qml.read_text().replace(
                    "HoverHandler {", "HoverHandler { enabled: false;"
                )
            )
        # Sample inside Qt so IPC polling cannot miss the 140ms animation.
        qml = temp / "qml/DetailPanel.qml"
        qml.write_text(qml.read_text().replace("    id:popup", """    id:popup
    property var testSamples:[]
    Timer {id:sampleTimer;interval:8;repeat:true;onTriggered:popup.testSamples.push(popup.inspect())}
    function sampleTransition(start){
        if(start){testSamples=[];sampleTimer.start();}else sampleTimer.stop();
        return testSamples;
    }"""))
        qml = temp / "qml/shell.qml"
        qml.write_text(qml.read_text().replace("case 'status':break;", """case 'transitionSamples':return {ok:true,samples:detailPanel.sampleTransition(req.start)};
        case 'status':break;"""))
        cfg = json.loads((ROOT / "examples/comparison.json").read_text())
        for b in cfg["bars"]:
            b.update(monitors=[1], autoHide=False)
        path = temp / "config.json"
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp / "qml"), BARTJIK_CONFIG=str(path))
        with (destination / "render.log").open("w") as log:
            p = subprocess.Popen(
                [str(ROOT / "bin/bartjik"), "--demo"], stdout=log, stderr=log,
                env={**os.environ, "WAYLAND_DEBUG": "client"},
            )
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))
                screen = ipc({"op": "monitors"})["monitors"][0]
                for module, title, count in [
                    ("time", "World clock", 4),
                    ("vpn", "Mullvad", 2),
                    ("tailnet", "Tailscale", 4),
                    ("input", "Input methods", 3),
                    ("quota", "Codex", 10),
                    ("credit", "OpenRouter", 3),
                ]:
                    ipc({"op": "detailTest", "module": module})
                    time.sleep(0.08)
                    assert not ipc({"op": "detailState"})["panel"]["opened"]
                    wait_for(lambda: ipc({"op": "detailState"})["panel"]["opened"])
                    state = ipc({"op": "detailState"})["panel"]
                    assert state["title"] == title and state["count"] == count, state
                    image = destination / (module + "-panel.png")
                    ipc({"op": "captureDetail", "path": str(image)})
                    wait_for(
                        lambda: image.exists()
                        and image.read_bytes().endswith(b"IEND\xaeB`\x82")
                    )
                    ipc({"op": "detailTest", "module": module, "leave": True})
                    wait_for(lambda: not ipc({"op": "detailState"})["panel"]["opened"])
                # The same fixed surfaces must survive every card switch.
                # Native geometry is independent of the animated card inside.
                log_start = len((destination / "render.log").read_text())
                previous = None
                previous_state = None
                transitions = []
                for module, title in [("quota", "Codex"), ("credit", "OpenRouter"),
                                      ("quota", "Codex"), ("credit", "OpenRouter")]:
                    if previous is not None:
                        ipc({"op": "detailTest", "module": previous, "leave": True})
                        time.sleep(0.025)
                    ipc({"op": "transitionSamples", "start": True})
                    ipc({"op": "detailTest", "module": module})
                    wait_for(lambda: ipc({"op": "detailState"})["panel"]["opened"])
                    wait_for(lambda: ipc({"op": "detailState"})["panel"]["title"] == title
                             and ipc({"op": "detailState"})["panel"]["crossfade"] == 1)
                    state = ipc({"op": "detailState"})["panel"]
                    assert state["title"] == title and not state["pending"], state
                    samples = ipc({"op": "transitionSamples"})["samples"]
                    if previous_state:
                        before, after = previous_state["card"], state["card"]
                        assert any(0 < p["crossfade"] < 1 for p in samples), samples
                        assert any(min(before["height"], after["height"]) < p["card"]["height"]
                                   < max(before["height"], after["height"]) for p in samples), samples
                        assert any(min(before["width"], after["width"]) < p["card"]["width"]
                                   < max(before["width"], after["width"]) for p in samples), samples
                        for sample in samples:
                            assert sample["opened"] and sample["surface"] == state["surface"], sample
                            card = sample["card"]
                            assert abs(card["y"] + card["height"] - state["anchorY"]) < 1, sample
                        transitions.extend(samples)
                    previous = module
                    previous_state = state
                ipc({"op": "detailTest", "module": "credit", "leave": True})
                wait_for(lambda: not ipc({"op": "detailState"})["panel"]["opened"])
                time.sleep(0.1)  # Wait for the closed card's empty input-region commit.
                full_trace = (destination / "render.log").read_text()
                trace = full_trace[log_start:]
                assert not re.search(r"get_layer_surface\(", trace), "Switch recreated a native shell"
                # Compare committed mask bounds with the sampled card envelope.
                baseline = len(committed_shell_regions(full_trace[:log_start]))
                regions = committed_shell_regions(full_trace)[baseline:]
                bounds, last = [], {}
                for surface, rectangles in regions:
                    assert rectangles is not None, "Shell captured the entire screen"
                    last[surface] = rectangles
                    if rectangles:
                        x = min(r[0] for r in rectangles)
                        y = min(r[1] for r in rectangles)
                        right = max(r[0] + r[2] for r in rectangles)
                        bottom = max(r[1] + r[3] for r in rectangles)
                        assert abs(bottom - state["anchorY"]) <= 1, (x, y, right, bottom)
                        assert 420 <= right - x <= 440 and bottom - y <= max(p["card"]["height"] for p in transitions) + 1
                        bounds.append((right - x, bottom - y))
                assert any(420 < width < 440 for width, height in bounds), bounds
                assert last and all(not region for region in last.values()), last
                (destination / "transitions.json").write_text(json.dumps(transitions, indent=2))
                creations = re.findall(r"xdg_surface[#@]\d+\.get_popup\(", trace)
                assert not creations, ("Card switches must reuse the stable surface", len(creations))
                assert not re.search(r"xdg_popup[#@]\d+\.reposition\(", trace), trace
                for module in ("quota", "credit", "quota", "credit", "quota"):
                    ipc({"op": "detailTest", "module": module})
                    if module == "quota" and not ipc({"op": "detailState"})["panel"]["opened"]:
                        wait_for(lambda: ipc({"op": "detailState"})["panel"]["opened"])
                    time.sleep(0.025)
                wait_for(lambda: ipc({"op": "detailState"})["panel"]["title"] == "Codex"
                         and ipc({"op": "detailState"})["panel"]["crossfade"] == 1)
                ipc({"op": "detailTest", "module": "credit"})
                ipc({"op": "hide", "bar": "status-bar"})
                time.sleep(0.2)
                assert not ipc({"op": "detailState"})["panel"]["opened"]
                ipc({"op": "show", "bar": "status-bar"})
                ipc({"op": "detailTest", "module": "time"})
                wait_for(lambda: ipc({"op": "detailState"})["panel"]["opened"])
                cfg["modules"]["time"]["panel"]["hover"] = False
                path.write_text(json.dumps(cfg))
                ipc({"op": "reload"})
                wait_for(lambda: not ipc({"op": "detailState"})["panel"]["opened"])
                ipc({"op": "detailTest", "module": "vpn"})
                wait_for(lambda: ipc({"op": "detailState"})["panel"]["opened"])
                ipc({"op": "hide", "bar": "status-bar"})
                wait_for(lambda: not ipc({"op": "detailState"})["panel"]["opened"])
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
        "World clock, Mullvad, Tailscale, input methods, Codex and OpenRouter panels: hover delays, data, screenshots, stable surfaces and card transitions, leave and hide passed"
    )


if __name__ == "__main__":
    main()
