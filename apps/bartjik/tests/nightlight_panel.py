"""Render the real detail panel and click its controls with synthetic data only."""

import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests"))
from render import wait_for


def main():
    assert os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/")
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="bartjik-night-panel-") as temporary:
        temp = Path(temporary)
        shutil.copytree(ROOT / "qml", temp / "qml")
        for name in ("Part.qml", "PreviewHover.qml", "DetailPanel.qml"):
            path = temp / "qml" / name
            path.write_text(path.read_text().replace("HoverHandler {", "HoverHandler {enabled:false;"))
        path = temp / "qml/NightlightDetails.qml"
        path.write_text(path.read_text().replace("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
                        .replace("    id:details", "    id:details\n    TestEvent {id:nightKeys}\n    function testKey(key){nightKeys.keyClickChar(key,Qt.NoModifier,0);}", 1))
        path = temp / "qml/DetailPanel.qml"
        source = path.read_text().replace("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
        source = source.replace("    id:popup", """    id:popup
    property var fixturePanel:null
    property var fixtureFeedback:({})
    property var fixtureEvents:[]
    TestEvent {id:nightInput}
    function nightTest(req) {
        if(req.panel)fixturePanel=req.panel;
        if(req.feedback)fixtureFeedback=req.feedback;
        if(req.reset)fixtureEvents=[];
        const all=[];function walk(item){all.push(item);for(const child of item.children || [])walk(child);};walk(nightlightDetails);
        const target=all.find(i=>i.objectName===req.name);
        if(req.name && !target)throw Error('Missing control '+req.name);
        if(req.scroll){
            const view=nightlightDetails.parent.parent;
            const y=target.mapToItem(nightlightDetails,0,0).y;
            view.contentY=Math.max(0,Math.min(y-20,view.contentHeight-view.height));
        }
        if(req.click)nightInput.mouseClick(target,target.width*(req.fraction || .5),target.height/2,Qt.LeftButton,Qt.NoModifier,0);
        if(req.focus){target.forceActiveFocus();target.selectAll();}
        if(req.text!==undefined)target.text=req.text;
        if(req.key)nightlightDetails.testKey(req.key);
        return {panel:panelData,events:fixtureEvents,state:inspect(),feedback:fixtureFeedback,
            control:target?{text:target.text,checked:target.checked,enabled:target.enabled,focus:target.activeFocus,visible:target.visible}:null};
    }
""", 1)
        source = source.replace("readonly property var panelData:row?.panel ||", "readonly property var panelData:fixturePanel || row?.panel ||")
        source = "\n".join("    readonly property var feedback:fixtureFeedback" if line.strip().startswith("readonly property var feedback:") else line for line in source.split("\n"))
        source = source.replace("function action(name,selection){actionFor(selected?.context.module,name,selection);}",
                                "function action(name,selection){fixtureEvents=fixtureEvents.concat([{action:name,selection:selection}]);}")
        path.write_text(source)
        path = temp / "qml/shell.qml"
        source = path.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        source = source.replace("case 'status':break;", """case 'nightTest':return {ok:true,result:detailPanel.nightTest(req)};
        case 'nightOpen': {
            const p=panels.instances.find(p=>p.bar.screen===req.monitor);
            const row=items(p.bar,'filter')[0],g=p.geometry;
            detailPanel.close();
            detailPanel.open({barKey:p.barKey,module:'filter',item:row.id,monitor:p.bar.screen,
                anchor:{x:g.x+g.width/2,y:g.y,width:0,height:g.height}},p,row);
            return {ok:true};
        }
        case 'nightClose':detailPanel.close();return {ok:true};
        case 'status':break;""")
        path.write_text(source)
        config = temp / "config.json"
        config.write_text(json.dumps(dict(theme="rose-punk", modules={"filter": {"provider": "nightlight"}},
                                         bars=[dict(id="night-fixture", monitors="all", autoHide=False,
                                                    islands=[dict(id="controls", modules=["filter"])]) ])))
        env = {**os.environ, "BARTJIK_QML": str(temp / "qml"), "BARTJIK_CONFIG": str(config)}

        def ipc(**request):
            result = subprocess.run(["quickshell", "ipc", "-p", str(temp / "qml"), "call", "bartjik", "request", json.dumps(request)],
                                    env=env, capture_output=True, text=True, timeout=5)
            if result.returncode:
                raise ValueError(result.stderr)
            value = json.loads(result.stdout)
            assert value.get("ok"), value
            return value

        def probe(**request):
            return ipc(op="nightTest", **request)["result"]

        def click(name, **request):
            probe(name="nightlight-" + name, scroll=True)
            time.sleep(.03)
            return probe(name="nightlight-" + name, click=True, **request)

        def capture(name):
            image = destination / (name + ".png")
            time.sleep(.3)
            ipc(op="captureDetail", path=str(image))
            wait_for(lambda: image.exists() and image.read_bytes().endswith(b"IEND\xaeB`\x82"))

        log_path = destination / "render.log"
        with log_path.open("w") as log:
            process = subprocess.Popen([str(ROOT / "bin/bartjik"), "--demo"], env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc(op="status").get("ready"))
                # The actual default hover route opens the shared detail panel.
                ipc(op="detailTest", module="filter")
                wait_for(lambda: ipc(op="detailState")["panel"]["opened"])
                assert ipc(op="detailState")["panel"]["provider"] == "nightlight"
                baseline = copy.deepcopy(probe()["panel"])
                for screen in ipc(op="monitors")["monitors"]:
                    ipc(op="nightOpen", monitor=screen["name"])
                    wait_for(lambda: probe()["state"]["opened"])
                    state = probe()["state"]
                    assert state["width"] <= screen["width"] - 24, state
                    assert state["height"] <= screen["height"] * .65, state
                    capture("solar-" + screen["name"])

                probe(reset=True)
                result = click("enabled")
                assert result["events"] == [dict(action="enabled", selection="off")], result
                # A failed action must not leave an optimistic switch state behind.
                result = probe(feedback=dict(error=True, message="Fixture failure"), name="nightlight-enabled")
                assert result["control"]["checked"], result
                probe(feedback={})
                result = click("solar")
                assert result["events"][-1] == dict(action="mode", selection="fixed"), result

                fixed = copy.deepcopy(baseline)
                fixed["settings"]["mode"] = "fixed"
                fixed["enabled"] = False
                probe(panel=fixed)
                click("start")
                probe(name="nightlight-start", focus=True, text="22:30")
                assert probe()["state"]["nightlight"]["needsKeyboard"]
                probe(name="nightlight-end", text="06:45")
                # Provider refreshes must not erase the user's edited times.
                refreshed = copy.deepcopy(fixed)
                refreshed["locationAgeHours"] = 2
                probe(panel=refreshed)
                result = click("hours")
                assert json.loads(result["events"][-1]["selection"]) == dict(start="22:30", end="06:45"), result
                assert result["events"][-1]["action"] == "hours"
                capture("fixed-hours")

                probe(reset=True)
                click("temperature", fraction=.3)
                wait_for(lambda: any(e["action"] == "temperature" for e in probe()["events"]))
                selected = int(probe()["events"][-1]["selection"])
                assert 2000 < selected < 4000, selected
                probe(reset=True)
                click("gamma", fraction=.6)
                wait_for(lambda: any(e["action"] == "gamma" for e in probe()["events"]))
                assert 70 <= int(probe()["events"][-1]["selection"]) <= 110
                result = probe(feedback=dict(busy=True), name="nightlight-solar")
                assert not result["control"]["enabled"], result
                probe(feedback={})

                click("city-search")
                probe(name="nightlight-city-search", focus=True)
                for char in "Hong":
                    probe(key=char)
                result = click("city-search-button")
                assert result["events"][-1] == dict(action="city-search", selection="Hong"), result
                cities = copy.deepcopy(baseline)
                cities["cities"] = [dict(id=i, name="Synthetic city " + str(i), detail="Fixture region") for i in range(5)]
                probe(panel=cities)
                result = click("city-4")
                assert result["events"][-1] == dict(action="city-select", selection="4"), result
                capture("city-results-scrolled")
                result = click("locate")
                assert result["events"][-1] == dict(action="locate", selection=""), result
                probe(panel=fixed, feedback=dict(error=True, message="Device location unavailable. Allow location access or search for your city."))
                probe(name="nightlight-feedback", scroll=True)
                capture("location-unavailable")
                ipc(op="nightClose")
                wait_for(lambda: not probe()["state"]["opened"])
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines()
                  if any(word in line for word in ("WARN scene", "ERROR", "ReferenceError", "TypeError", "Binding loop"))]
        assert not errors, errors[:15]
    print("Night light: rendered panel on each monitor, switches, sliders, fixed hours, typing, city results, scrolling and error recovery passed")


if __name__ == "__main__":
    main()
