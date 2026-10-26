"""Real Qt mouse events across shared crypto/workspace island padding and gaps."""

import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from render import wait_for

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/")
    with tempfile.TemporaryDirectory(prefix="bartjik-hover-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        p = temp / "qml/Bar.qml"
        s = p.read_text().replace("import QtQuick", "import QtQuick\nimport QtTest", 1)
        s = s.replace(
            "    id: panel",
            """    id: panel
    TestEvent {id:testInput}
    function hoverTest(id,x,y,leave) {
        const i=groups.children.find(c=>c.islandId===id);
        const p=i.mapToItem(contentItem,0,0);
        if(leave){testInput.mouseMove(contentItem,0,0,0,Qt.NoButton,Qt.NoModifier);testInput.mouseClick(contentItem,0,0,Qt.LeftButton,Qt.NoModifier,0);}
        else if(x!==undefined)testInput.mouseMove(i,x,y,0,Qt.NoButton,Qt.NoModifier);
        return {ok:true,geometry:geometry,x:p.x,y:p.y,width:i.width,height:i.height,targets:i.previewTargets().map(t=>({x:t.x,y:t.y,width:t.width,height:t.height,id:t.row.id,enabled:t.enabled,provider:t.provider}))};
    }""",
        )
        p.write_text(s)
        p = temp / "qml/shell.qml"
        s = p.read_text().replace(
            "if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}",
            "if(msg.kind==='pointer'){}",
        )
        s = s.replace(
            "case 'status':break;",
            "case 'hoverTest':return panels.instances.find(p=>p.bar.id==='workspace-bar').hoverTest(req.island,req.x,req.y,req.leave);\n        case 'status':break;",
        )
        p.write_text(s)
        cfg = json.loads((ROOT / "examples/comparison.json").read_text())
        # Keep every appended hover target inside the test bar's clip region.
        cfg["layout"]["centre_width"] = "100%"
        cfg["bars"] = cfg["bars"][:1]
        cfg["bars"][0]["islands"].append({"id":"ai","modules":["quota","credit"]})
        cfg["bars"][0]["islands"].append({"id":"audio","modules":["sound"]})
        cfg["bars"][0]["islands"].append({"id":"network","modules":["net"]})
        cfg["modules"]["sound"]["actions"]["hoverEnter"]="audio.panel"
        if "--connectivity-only" in sys.argv:
            cfg["bars"][0]["islands"]=[i for i in cfg["bars"][0]["islands"] if i["id"] in ("audio","network")]
        cfg["bars"][0].update(monitors=[1], autoHide=False, position="c", alignment="c")
        path = temp / "config.json"
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp / "qml"), BARTJIK_CONFIG=str(path))
        with (temp / "log").open("w") as log:
            process = subprocess.Popen(
                [str(ROOT / "bin/bartjik"), "--demo"], stdout=log, stderr=log
            )
            try:
                wait_for(lambda: ipc({"op": "status"}).get("ready"))
                time.sleep(0.5)
                for island, op in ([] if "--ai-only" in sys.argv or "--connectivity-only" in sys.argv else [
                    ("markets", "cryptoState"),
                    ("workspaces", "workspacePreviewState"),
                ]):

                    def state():
                        s = ipc({"op": op})
                        return s.get("preview", s)

                    def move(x=None, y=5, leave=False):
                        q = {
                            "op": "hoverTest",
                            "island": island,
                            "y": y,
                            "leave": leave,
                        }
                        if x is not None:
                            q["x"] = x
                        return ipc(q)

                    layout = move()
                    targets = layout["targets"]
                    first = targets[0]
                    second = targets[1]
                    move(2)
                    time.sleep(0.06)
                    assert not state()["opened"]
                    move(leave=True)
                    time.sleep(0.35)
                    assert not state()["opened"]
                    start = time.monotonic()
                    # Continuous motion on one item must not keep restarting dwell.
                    while time.monotonic() - start < 0.4:
                        move(2 + (int((time.monotonic() - start) * 100) % 5))
                        time.sleep(0.025)
                    wait_for(lambda: state()["opened"])
                    expected = first["id"]
                    assert (
                        state().get("pair") == expected
                        if island == "markets"
                        else "/" + expected + "/" in state()["key"]
                    )
                    # Cover actual labels/icons, not only the previously tested outer padding.
                    for t in targets:
                        for x in (t["x"]+3,t["x"]+t["width"]/2,t["x"]+t["width"]-3):
                            move(x,t["y"]+t["height"]/2)
                            time.sleep(.2)
                            assert state()["opened"], (island,x,state())
                    gap = first["x"] + first["width"] + 2
                    move(gap)
                    time.sleep(0.18)
                    assert state()["opened"]

                    def centered():
                        move(gap)
                        s = state()
                        return (
                            abs(s["anchorX"] + s["width"] / 2 - (layout["x"] + gap)) < 1
                        )

                    try:
                        wait_for(centered)
                    except AssertionError:
                        print("centre failure", island, layout, gap, state(), flush=True)
                        raise
                    move(second["x"] + 3)
                    wait_for(
                        lambda: state().get("pair") == second["id"]
                        if island == "markets"
                        else "/" + second["id"] + "/" in state()["key"]
                    )
                    move(layout["width"] - 2, layout["height"] - 2)
                    time.sleep(0.2)
                    assert state()["opened"]
                    move(leave=True)
                    wait_for(lambda: not state()["opened"])
                if "--connectivity-only" not in sys.argv:
                    # Each AI module opens cold over icon and value, and retains hover.
                    layout=ipc({"op":"hoverTest","island":"ai"})
                    for target, title in zip(layout["targets"], ("Codex", "OpenRouter")):
                        for x in (target["x"]+4,target["x"]+target["width"]-4):
                            ipc({"op":"hoverTest","island":"ai","leave":True})
                            time.sleep(.35)
                            ipc({"op":"hoverTest","island":"ai","x":x,"y":target["y"]+target["height"]/2})
                            wait_for(lambda:ipc({"op":"detailState"})["panel"]["opened"])
                            assert ipc({"op":"detailState"})["panel"]["title"]==title
                            time.sleep(.3)
                            assert ipc({"op":"detailState"})["panel"]["opened"]
                if "--ai-only" not in sys.argv:
                    for island, module in (("audio", "sound"), ("network", "net")):
                        layout=ipc({"op":"hoverTest","island":island})
                        target=layout["targets"][0]
                        for x in (2, target["x"]+4,target["x"]+target["width"]-4,layout["width"]-2):
                            ipc({"op":"hoverTest","island":island,"leave":True})
                            time.sleep(.3)
                            ipc({"op":"hoverTest","island":island,"x":x,"y":target["y"]+target["height"]/2})
                            try:
                                wait_for(lambda:ipc({"op":"detailState"})["panel"]["opened"])
                            except AssertionError:
                                print('native hover failure',island,x,layout,ipc({'op':'detailState'}),flush=True)
                                raise
                            time.sleep(.3)
                            result=ipc({"op":"detailState"})["panel"]
                            assert result["provider"]==island,result
                            assert abs(result["anchorX"]+result["width"]/2-layout["x"]-x)<1,result
                            assert not result["audio"]["meters"] and not result["network"]["traffic"],result
                            # Continue moving on the same item after it has opened.
                            x2=min(layout["width"]-1,x+4)
                            ipc({"op":"hoverTest","island":island,"x":x2,"y":target["y"]+target["height"]/2})
                            wait_for(lambda:abs(ipc({"op":"detailState"})["panel"]["anchorX"]+result["width"]/2-layout["x"]-x2)<1)
            finally:
                shutil.copyfile(temp/"log", "/tmp/bartjik-hover-r13.log")
                process.terminate()
                process.wait(timeout=5)
        errors = [
            l
            for l in (temp / "log").read_text().splitlines()
            if any(k in l for k in ("WARN scene", "ERROR", "Traceback", "Binding loop"))
        ]
        assert not errors, errors[:15]
    print(
        "Codex/OpenRouter: real mouse hover over icon and value passed" if "--ai-only" in sys.argv else "Shared hover: icon/value/padding targets, correct context, native panels, continuous pointer centring and leave cleanup passed"
    )


if __name__ == "__main__":
    main()
