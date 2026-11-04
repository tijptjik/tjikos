"""Real panel interaction with synthetic data and intercepted session actions."""
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
    with tempfile.TemporaryDirectory(prefix="bartjik-idle-panel-") as temporary:
        temp = Path(temporary)
        shutil.copytree(ROOT / "qml", temp / "qml")
        for name in ("Part.qml", "PreviewHover.qml", "DetailPanel.qml"):
            path = temp / "qml" / name
            path.write_text(path.read_text().replace("HoverHandler {", "HoverHandler {enabled:false;"))
        path = temp / "qml/PowerConfirmation.qml"
        source = path.read_text().replace("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
        source = source.replace("    id:confirmation", """    id:confirmation
    TestEvent {id:powerInput}
    function testPower(req){
        const all=[];function walk(i){all.push(i);for(const c of i.children || [])walk(c);};walk(body);
        if(req.name){const button=all.find(i=>i.objectName===req.name);if(!button)throw Error('Missing power control');powerInput.mouseClick(button,button.width/2,button.height/2,Qt.LeftButton,Qt.NoModifier,0);}
        if(req.escape)powerInput.keyClick(Qt.Key_Escape,Qt.NoModifier,0);
        if(req.capture)body.grabToImage(image=>image.saveToFile(req.capture));
        const cancel=all.find(i=>i.objectName==='power-cancel'),accept=all.find(i=>i.objectName==='power-confirm');
        const left=cancel.mapToItem(body,0,0).x,right=accept.mapToItem(body,accept.width,0).x;
        return {visible:visible,width:body.width,left:left,right:right};
    }
""", 1).replace("'bartjik-power-confirmation'", "'bartjik-power-confirmation-test'")
        path.write_text(source)
        path = temp / "qml/IdleDetails.qml"
        path.write_text(path.read_text().replace("    id:details", "    id:details\n    function testPower(req){return powerWindow.testPower(req);}", 1)
                        .replace("    PowerConfirmation {", "    PowerConfirmation {\n        id:powerWindow", 1))
        path = temp / "qml/DetailPanel.qml"
        source = path.read_text().replace("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
        source = source.replace("    id:popup", """    id:popup
    property var fixturePanel:null
    property var fixtureFeedback:({})
    property var fixtureEvents:[]
    TestEvent {id:idleInput}
    function idleTest(req){
        if(req.panel)fixturePanel=req.panel;
        if(req.feedback)fixtureFeedback=req.feedback;
        if(req.reset)fixtureEvents=[];
        const all=[];function walk(i){all.push(i);for(const c of i.children || [])walk(c);};walk(idleDetails);
        const target=all.find(i=>i.objectName===req.name);
        if(req.name && !target)throw Error('Missing idle control '+req.name);
        if(req.scroll){const view=idleDetails.parent.parent,y=target.mapToItem(idleDetails,0,0).y;view.contentY=Math.max(0,Math.min(y-20,view.contentHeight-view.height));}
        if(req.click)idleInput.mouseClick(target,target.width/2,target.height/2,Qt.LeftButton,Qt.NoModifier,0);
        if(req.choice){const list=target.popup.contentItem,index=req.choice==='first'?0:target.count-1;list.positionViewAtIndex(index,ListView.Beginning);list.forceLayout();const option=list.itemAtIndex(index);if(!option)throw Error('Missing dropdown option');idleInput.mouseClick(option,option.width/2,option.height/2,Qt.LeftButton,Qt.NoModifier,0);}
        const power=idleDetails.testPower(req.power || {});
        return {panel:panelData,events:fixtureEvents,state:inspect(),power:power,control:target?{text:target.text,checked:target.checked,enabled:target.enabled,displayText:target.displayText}:null};
    }
""", 1)
        source = source.replace("readonly property var panelData:row?.panel ||", "readonly property var panelData:fixturePanel || row?.panel ||")
        source = "\n".join("    readonly property var feedback:fixtureFeedback" if line.strip().startswith("readonly property var feedback:") else line for line in source.split("\n"))
        source = source.replace("function action(name,selection){actionFor(selected?.context.module,name,selection);}",
                                "function action(name,selection){fixtureEvents=fixtureEvents.concat([{action:name,selection:selection}]);}")
        path.write_text(source)
        path = temp / "qml/shell.qml"
        source = path.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        source = source.replace("case 'status':break;", """case 'idleTest':return {ok:true,result:detailPanel.idleTest(req)};
        case 'idleOpen': {
            const p=panels.instances.find(p=>p.bar.screen===req.monitor),row=items(p.bar,'idle')[0],g=p.geometry;
            detailPanel.close();detailPanel.open({barKey:p.barKey,module:'idle',item:row.id,monitor:p.bar.screen,anchor:{x:g.x+g.width/2,y:g.y,width:0,height:g.height}},p,row);
            return {ok:true};
        }
        case 'idleClose':detailPanel.close();return {ok:true};
        case 'status':break;""")
        path.write_text(source)
        config = temp / "config.json"
        config.write_text(json.dumps(dict(theme="rose-punk", modules={"idle": {"provider": "caffeine"}},
                                         bars=[dict(id="idle-fixture", monitors="all", autoHide=False,
                                                    islands=[dict(id="controls", modules=["idle"])])])))
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
            return ipc(op="idleTest", **request)["result"]

        def click(name):
            probe(name="idle-" + name, scroll=True)
            time.sleep(.03)
            return probe(name="idle-" + name, click=True)

        def capture(name):
            path = destination / (name + ".png")
            time.sleep(.3)
            ipc(op="captureDetail", path=str(path))
            wait_for(lambda: path.exists() and path.read_bytes().endswith(b"IEND\xaeB`\x82"))

        def centered(monitor):
            monitors = {m["name"]: m for m in json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], text=True))}
            layers = json.loads(subprocess.check_output(["hyprctl", "-j", "layers"], text=True))
            found = [layer for level in layers[monitor]["levels"].values() for layer in level
                     if layer.get("namespace") == "bartjik-power-confirmation-test"]
            if len(found) != 1:
                return False
            layer = found[0]
            m = monitors[monitor]
            w, h = m["width"] / m["scale"], m["height"] / m["scale"]
            if m["transform"] % 2:
                w, h = h, w
            return abs(layer["x"] + layer["w"] / 2 - (m["x"] + w / 2)) <= 1 and abs(layer["y"] + layer["h"] / 2 - (m["y"] + h / 2)) <= 1

        log_path = destination / "render.log"
        with log_path.open("w") as log:
            process = subprocess.Popen([str(ROOT / "bin/bartjik"), "--demo"], env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc(op="status").get("ready"))
                ipc(op="detailTest", module="idle")
                wait_for(lambda: ipc(op="detailState")["panel"]["opened"])
                assert probe()["state"]["provider"] == "caffeine"
                baseline = copy.deepcopy(probe()["panel"])
                for screen in ipc(op="monitors")["monitors"]:
                    ipc(op="idleOpen", monitor=screen["name"])
                    wait_for(lambda: probe()["state"]["opened"])
                    capture("idle-" + screen["name"])
                    probe(reset=True)
                    click("poweroff")
                    wait_for(lambda: probe()["power"]["visible"] and centered(screen["name"]))
                    assert not probe()["events"]
                    power = probe()["power"]
                    assert abs((power["left"] + power["right"]) / 2 - power["width"] / 2) <= 1, power
                    image = destination / ("power-confirmation-" + screen["name"] + ".png")
                    probe(power=dict(capture=str(image)))
                    wait_for(lambda: image.exists() and image.read_bytes().endswith(b"IEND\xaeB`\x82"))
                    result = probe(power=dict(name="power-cancel"))
                    assert not result["power"]["visible"] and not result["events"], result
                click("poweroff")
                result = probe(power=dict(escape=True))
                assert not result["power"]["visible"] and not result["events"], result
                click("poweroff")
                result = probe(power=dict(name="power-confirm"))
                assert result["events"] == [dict(action="poweroff", selection="confirm")], result
                assert not result["power"]["visible"]
                assert click("lock")["events"][-1] == dict(action="lock", selection="")
                assert click("suspend")["events"][-1] == dict(action="suspend", selection="")
                assert click("enabled")["events"][-1] == dict(action="enabled", selection="off")
                assert probe(name="idle-enabled")["control"]["checked"]
                click("duration")
                result = probe(name="idle-duration", choice="first")
                assert result["events"][-1] == dict(action="duration", selection="1800"), result
                timed = copy.deepcopy(baseline)
                timed.update(duration=1800, deadline=time.time() + 1800)
                probe(panel=timed)
                assert "30 min" in probe(name="idle-state")["control"]["text"]
                probe(reset=True)
                click("delay-lock")
                probe(name="idle-delay-lock", choice="first")
                assert probe()["state"]["idle"]["dirty"]
                assert not probe()["events"]
                refreshed = copy.deepcopy(timed)
                refreshed["deadline"] += 1
                probe(panel=refreshed)
                result = click("apply")
                request = json.loads(result["events"][-1]["selection"])
                assert request["delays"]["lock"] == 0 and request["revision"] == "synthetic", request
                click("reset")
                assert not probe()["state"]["idle"]["dirty"]
                game = copy.deepcopy(baseline)
                game.update(gamePolicy=True, delays=dict(dim=0, lock=0, screenOff=15, suspend=0))
                probe(panel=game)
                capture("game-idle-policy")
                result = probe(feedback=dict(busy=True), name="idle-poweroff")
                assert not result["control"]["enabled"]
                probe(feedback=dict(error=True, message="Fixture failure"))
                capture("idle-error")
                ipc(op="idleClose")
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines()
                  if any(word in line for word in ("WARN scene", "ERROR", "ReferenceError", "TypeError", "Binding loop"))]
        assert not errors, errors[:20]
    print("Idle panel: actual hover, timers, staged delays, session actions, centered confirmation, Cancel/Escape and confirmed poweroff passed on every output")


if __name__ == "__main__":
    main()
