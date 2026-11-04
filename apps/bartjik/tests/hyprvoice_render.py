"""Synthetic Hyprvoice tray status, pulse, and layout checks on a private bus."""

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
sys.path.insert(0, str(ROOT / "tests/reference"))
from bartjik import ipc


PROBE = """
    function hyprvoiceFixtureProbe() {
        const panel=panels.instances[0], dots=[], parts=[];
        function visit(q) {
            if(q.objectName==='recording-indicator') {
                const xy=q.mapToItem(panel.contentItem,0,0);
                dots.push({tray:q.isTrayIndicator,visible:q.visible,opacity:q.opacity,
                    x:xy.x,y:xy.y,width:q.width,height:q.height,color:String(q.color),
                    slotWidth:q.parent.width,slotHeight:q.parent.height,
                    localX:q.x,localY:q.y,slotX:q.parent.x,slotY:q.parent.y});
            }
            if(q.kind==='icon' && q.moduleConfig?.provider==='icontray' && q.visible) {
                const xy=q.mapToItem(panel.contentItem,0,0);
                parts.push({id:q.row.id,x:xy.x,y:xy.y,width:q.width,height:q.height});
            }
            for(const child of q.children || [])visit(child);
        }
        visit(panel.contentItem);
        return {ok:true,state:root.hyprvoiceState,screenRecording:root.screenRecording,dots:dots,parts:parts,bar:panel.inspect()};
    }
"""

FIXTURE_CASES = """case 'hyprvoiceFixture':
            if(req.state!==undefined)native.hyprvoiceState=req.state;
            if(req.screenRecording!==undefined)native.screenRecording=req.screenRecording;
            if(req.emptyTray!==undefined)root.hyprvoiceFixtureEmptyTray=req.emptyTray;
            return hyprvoiceFixtureProbe();
        case 'hyprvoiceProbe':return hyprvoiceFixtureProbe();
        case 'status':break;"""


def tray_dot(probe):
    dots = [dot for dot in probe["dots"] if dot["tray"]]
    assert len(dots) == 1, probe
    assert not any(dot["visible"] for dot in probe["dots"] if not dot["tray"]), probe
    return dots[0]


def layout(probe):
    return {
        "geometry": probe["bar"]["geometry"],
        "islands": [
            {key: island[key] for key in ("id", "x", "y", "width", "height")}
            for island in probe["bar"]["islands"]
        ],
        "parts": probe["parts"],
    }


def active_slot(idle, active, horizontal=True):
    dot = tray_dot(active)
    assert dot["visible"] and dot["width"] == dot["height"] == 10, active
    before = next(i for i in idle["bar"]["islands"] if i["id"] == "tray")
    after = next(i for i in active["bar"]["islands"] if i["id"] == "tray")
    axis, cross = ("x", "y") if horizontal else ("y", "x")
    size, other_size = ("width", "height") if horizontal else ("height", "width")
    slot_size = dot["slotWidth"] if horizontal else dot["slotHeight"]
    assert abs(after[size] - before[size] - slot_size - 12) < .01, (before, after, dot)
    assert after[other_size] == before[other_size], (before, after)
    centers = [dot[axis] + dot[size] / 2] + [p[axis] + p[size] / 2 for p in active["parts"]]
    assert centers == sorted(centers), centers
    steps = [right - left for left, right in zip(centers, centers[1:])]
    assert len(steps) >= 2 and all(abs(step - steps[1]) < .01 for step in steps), (steps, dot, active["parts"])
    cross_center = dot[cross] + dot[other_size] / 2
    assert all(abs(p[cross] + p[other_size] / 2 - cross_center) < .01 for p in active["parts"]), active
    return layout(active)


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("Run with dbus-run-session --config-file tests/bus.conf")
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    evidence = {}
    with tempfile.TemporaryDirectory(prefix="bartjik-hyprvoice-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        shell = temp / "qml/shell.qml"
        source = shell.read_text()
        assert "case 'status':break;" in source
        source = source.replace("    function request(req) {", PROBE + "    function request(req) {")
        source = source.replace("case 'status':break;", FIXTURE_CASES)
        source = source.replace("    property bool workspaceReminder: false", "    property bool hyprvoiceFixtureEmptyTray: false\n    property bool workspaceReminder: false")
        source = source.replace("const m=bar.moduleConfigs[id];if(!m)return [];", "const m=bar.moduleConfigs[id];if(!m)return [];\n        if(demo && hyprvoiceFixtureEmptyTray && m.provider==='icontray')return [];")
        shell.write_text(source)
        config = {
            "theme": "rose-punk",
            "style": {"animateIcons": True},
            "modules": {
                "tray": {"provider": "icontray", "style": {"iconSize": 21, "itemGap": 12}},
                "time": {"provider": "datetime"},
            },
            "bars": [{
                "id": "voice-fixture", "monitors": [1], "side": "t", "position": "c",
                "islands": [{"id": "tray", "modules": ["tray"]}, {"id": "clock", "modules": ["time"]}],
            }],
        }
        config_path = temp / "config.json"
        config_path.write_text(json.dumps(config))
        os.environ.update(BARTJIK_QML=str(temp / "qml"), BARTJIK_CONFIG=str(config_path))

        def probe():
            return ipc({"op": "hyprvoiceProbe"})

        def state(value):
            ipc({"op": "hyprvoiceFixture", "state": value})
            return wait_for(lambda: (result := probe())["state"] == value and result)

        def capture(name):
            shot = destination / (name + ".png")
            shot.unlink(missing_ok=True)
            ipc({"op": "capture", "bar": probe()["bar"]["key"], "path": str(shot)})
            wait_for(lambda: shot.exists() and shot.read_bytes().endswith(b"IEND\xaeB`\x82"))

        log_path = destination / "render.log"
        with log_path.open("w") as log:
            process = subprocess.Popen([str(ROOT / "bin/bartjik"), "--demo"], stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))
                wait_for(lambda: len(probe()["parts"]) >= 2)
                time.sleep(.3)
                idle = state("idle")
                assert not tray_dot(idle)["visible"], idle
                baseline = layout(idle)
                capture("idle")

                recording = state("recording")
                assert tray_dot(recording)["visible"] and tray_dot(recording)["opacity"] == 1, recording
                active_layout = active_slot(idle, recording)
                time.sleep(.25)
                assert tray_dot(probe())["opacity"] == 1
                capture("recording")

                state("processing")
                samples = []
                for _ in range(16):
                    sample = probe()
                    dot = tray_dot(sample)
                    assert dot["visible"], sample
                    assert layout(sample) == active_layout, (sample, active_layout)
                    samples.append(dot["opacity"])
                    time.sleep(.09)
                assert max(samples) - min(samples) > .3, samples
                assert 0 < min(samples) <= max(samples) <= 1, samples
                wait_for(lambda: tray_dot(probe())["opacity"] < .5)
                capture("processing-dim")
                wait_for(lambda: tray_dot(probe())["opacity"] > .9)
                capture("processing-bright")
                evidence["processing_opacity"] = samples

                recording = state("recording")
                wait_for(lambda: tray_dot(probe())["opacity"] == 1)
                time.sleep(.3)
                assert tray_dot(probe())["opacity"] == 1
                assert layout(recording) == active_layout
                for value in ("idle", "unavailable"):
                    inactive = state(value)
                    assert not tray_dot(inactive)["visible"]
                    assert layout(inactive) == baseline, (inactive, baseline)
                # Screen recording shares the same leading slot, taking precedence over processing pulse.
                state("idle")
                ipc({"op": "hyprvoiceFixture", "screenRecording": True})
                screen = wait_for(lambda: (result := probe())["screenRecording"] and result)
                assert tray_dot(screen)["visible"] and tray_dot(screen)["opacity"] == 1
                assert layout(screen) == active_layout
                capture("screen-recording")
                for voice in ("recording", "processing", "unavailable"):
                    state(voice)
                    time.sleep(.2)
                    assert tray_dot(probe())["opacity"] == 1
                    assert layout(probe()) == active_layout
                ipc({"op": "hyprvoiceFixture", "screenRecording": False})
                state("idle")
                assert layout(probe()) == baseline
                evidence["screen_recording"] = screen
                evidence["horizontal_layout"] = {"idle": baseline, "active": active_layout}

                config["style"]["animateIcons"] = False
                config_path.write_text(json.dumps(config))
                wait_for(lambda: ipc({"op": "config"})["bars"][0]["style"]["animateIcons"] is False)
                state("processing")
                time.sleep(.2)
                reduced = [tray_dot(probe())["opacity"]]
                time.sleep(.6)
                reduced.append(tray_dot(probe())["opacity"])
                assert reduced == [1, 1], reduced
                capture("reduced-motion")

                config["bars"][0]["side"] = "l"
                config_path.write_text(json.dumps(config))
                wait_for(lambda: ipc({"op": "config"})["bars"][0]["side"] == "l")
                time.sleep(.2)
                vertical_idle = state("idle")
                ipc({"op": "hyprvoiceFixture", "screenRecording": True})
                vertical_recording = probe()
                active_slot(vertical_idle, vertical_recording, horizontal=False)
                capture("vertical-recording")
                ipc({"op": "hyprvoiceFixture", "screenRecording": False})
                vertical_inactive = state("idle")
                assert layout(vertical_inactive) == layout(vertical_idle)
                ipc({"op": "hyprvoiceFixture", "screenRecording": True})
                evidence["vertical_layout"] = {"idle": layout(vertical_idle), "active": layout(vertical_recording)}

                ipc({"op": "hyprvoiceFixture", "emptyTray": True})
                empty = wait_for(lambda: not (result := probe())["parts"] and result)
                island = next(i for i in empty["bar"]["islands"] if i["id"] == "tray")
                assert island["visible"] and island["width"] > 0 and island["height"] > 0, empty
                assert tray_dot(empty)["visible"] and empty["bar"]["inputRegions"] == 2, empty
                capture("empty-tray-recording")
                ipc({"op": "hyprvoiceFixture", "screenRecording": False})
                state("idle")
                wait_for(lambda: not next(i for i in probe()["bar"]["islands"] if i["id"] == "tray")["visible"])
                evidence["empty_tray"] = empty
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines()
                  if any(word in line for word in ("WARN scene", "ERROR", "Traceback", "Binding loop", "ReferenceError", "TypeError"))]
        assert not errors, errors
    (destination / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print("Hyprvoice dot: leading icon-size slot, matching spacing, island expansion/contraction, steady processing layout, reduced motion and empty tray passed:", destination)


if __name__ == "__main__":
    main()
