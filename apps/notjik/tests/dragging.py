"""Real QML pointer gestures against synthetic notices on an isolated D-Bus.

Run: dbus-run-session --config-file tests/bus.conf -- python3 -B tests/dragging.py
Test IPC and QtTest are injected only into a temporary copy.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
from notjik import ipc
from integration import wait_for
from native_fixture import copy_runtime, copy_file


def run():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("A private dbus-run-session is required")
    with tempfile.TemporaryDirectory(prefix="notjik-drag-test-") as temp:
        root = Path(temp)
        copy_runtime(root)
        shell = root / "qml/shell.qml"
        shell.write_text(shell.read_text().replace("model: root.screens", "id: popupPanels\n        model: root.screens")
            .replace('case "status": break;', '''case "testScreens": return {ok:true, screens:Quickshell.screens.map(s=>({name:s.name,x:s.x,y:s.y,width:s.width,height:s.height}))};
            case "dragState": return {ok:true, ids:floatingIds, positions:floatingPositions, dragging:draggingId, preview:dragPreviewId,
                popups:popupPanels.instances[0].testCards(), floats:[].concat(...floatingPanels.instances.map(p => p.testCards())),
                stack:visibleNotices.map(n=>n.id)};
            case "pointer": {
                const panel = request.floating ? floatingPanels.instances.find(p=>p.noticeId===request.id) : popupPanels.instances[0];
                panel.pointer(request.phase, request.x, request.y, request.control); return {ok:true};
            }
            case "dragCapture": floatingPanels.instances.find(p=>p.noticeId===request.id).capture(request.id, request.path); return {ok:true};
            case "testLock": lockState = request.locked ? "locked" : "unlocked"; return {ok:true};
            case "status": break;'''))
        card = root / "qml/NotificationCard.qml"
        card.write_text(card.read_text().replace("    id: card", '''    id: card
    function testGeometry() {
        const point = card.Window.window ? card.mapToItem(card.Window.window.contentItem, 0, 0) : Qt.point(0,0);
        const buttons = [];
        function visit(item) {
            if (!item.visible || !item.enabled) return;
            if (item instanceof Button) {
                const p = item.mapToItem(card, item.width/2, item.height/2);
                if (p.x>=0 && p.y>=0 && p.x<width && p.y<height) buttons.push({text:item.text, x:p.x, y:p.y, draggable:dragArea.contains(p), interactive:card.interactiveAt(card,p)});
            }
            for (const child of item.children) visit(child);
        }
        visit(card);
        return {id:notice.id, x:point.x, y:point.y, width:width, height:height, floating:floating,
            visible:visible, opacity:opacity, buttons:buttons, dragging:dragging};
    }''', 1).replace("import QtQuick\n", "import QtQuick\nimport QtQuick.Window\n", 1))
        for name in ("NotificationPanel.qml", "FloatingPanel.qml"):
            panel = root / "qml" / name
            floating = name == "FloatingPanel.qml"
            expression = "[card]" if floating else "Array.from({length:cardItems.count}, (_,i)=>cardItems.itemAt(i).testCard)"
            code = panel.read_text().replace("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            code = code.replace("    id: panel", '''    id: panel
    TestEvent { id: testEvents }
    function pointer(phase, x, y, control) {
        const modifiers = control ? Qt.ControlModifier : Qt.NoModifier;
        if (phase === "hover") testEvents.mouseMove(panel.contentItem, x, y, 0, Qt.NoButton, modifiers);
        if (phase === "press") testEvents.mousePress(panel.contentItem, x, y, Qt.LeftButton, modifiers, 0);
        if (phase === "move") testEvents.mouseMove(panel.contentItem, x, y, 0, Qt.LeftButton, modifiers);
        if (phase === "release") testEvents.mouseRelease(panel.contentItem, x, y, Qt.LeftButton, modifiers, 0);
    }
    function testCards() { return ''' + expression + '''.filter(c=>c).map(c=>c.testGeometry()); }
    function capture(id, path) {
        const capturedCard = ''' + expression + '''.find(c=>c.notice.id===id);
        capturedCard.grabToImage(result=>result.saveToFile(path));
    }''', 1)
            if floating:
                code = code.replace("required property string noticeId", "required property string noticeId\n    property int testSystemMoves: 0")
                code = code.replace("onDragStarted: panel.startSystemMove()", "onDragStarted: panel.testSystemMoves++")
                code = code.replace("c.testGeometry()", "Object.assign(c.testGeometry(), {address:panel.address,placed:panel.placed,frames:panel.placementFrames,moves:panel.testSystemMoves,natural:c.implicitHeight})")
            if not floating:
                code = code.replace("property var cachedNotice: null", "property var cachedNotice: null\n                        property alias testCard: card")
            panel.write_text(code)
        config = {"theme":"rose-punk", "placement":{"monitors":[1]}, "pauseOnHover":False,
                  "grouping":{"enabled":True}, "stack":{"animationMs":0}}
        (root / "config.json").write_text(json.dumps(config))
        env = dict(os.environ, NOTJIK_CONFIG=str(root / "config.json"), NOTJIK_STATE_DIR=str(root / "state"), QT_ACCESSIBILITY="0")
        with (root / "qml.log").open("w+") as log:
            process = subprocess.Popen(["quickshell", "--path", str(root / "qml"), "--no-color"], env=env, stdout=log, stderr=log)
            try:
                def call(op, **kw):
                    return ipc(root, {"op":op, **kw})
                def state():
                    return call("dragState")
                def send(title="Drag me", **kw):
                    return call("send", notice={"title":title, "body":"Synthetic drag acceptance", "timeout":0, **kw})["id"]
                def card(ident, floating=False, monitor=0):
                    values = state()["floats"] if floating else state()["popups"]
                    return next((c for c in values if c["id"]==ident), None)
                def pointer(phase, x, y, floating=False, monitor=0, ident=None, control=False):
                    call("pointer", phase=phase, x=x, y=y, floating=floating, monitor=monitor, id=ident, control=control)
                def gesture(ident, dx=90, dy=-90, floating=False, point=None, monitor=0):
                    c = card(ident, floating, monitor)
                    x, y = point or (c["width"]/2, 35)
                    x += c["x"]; y += c["y"]
                    pointer("press", x, y, floating, monitor, ident)
                    pointer("move", x+dx/2, y+dy/2, floating, monitor, ident)
                    pointer("move", x+dx, y+dy, floating, monitor, ident)
                    before = state()
                    pointer("release", x+dx, y+dy, floating, monitor, ident)
                    return before
                def reveal(ident, floating=False, monitor=0):
                    # A real pointer enter reveals actions; allow the slide to settle.
                    pointer("hover",-20,-20,floating,monitor,ident)
                    for _ in range(3):
                        c=card(ident,floating,monitor)
                        pointer("hover",c["x"]+c["width"]/2,c["y"]+35,floating,monitor,ident)
                        time.sleep(.08)
                    return card(ident,floating,monitor)
                def click(ident, floating=False, button=None, monitor=0, control=False):
                    c = reveal(ident, floating, monitor) if button else card(ident, floating, monitor)
                    point = next(b for b in c["buttons"] if b["text"]==button) if button else {"x":c["width"]/2,"y":35}
                    x, y = c["x"]+point["x"], c["y"]+point["y"]
                    pointer("press", x,y,floating,monitor,ident,control)
                    pointer("release",x,y,floating,monitor,ident,control)
                def clients():
                    return json.loads(subprocess.check_output(["hyprctl","-j","clients"],text=True))
                def client(ident):
                    address=card(ident,True)["address"]
                    return next((c for c in clients() if c["address"]==address),None)
                def live(ident):
                    return any(n["id"]==ident for n in call("list")["notices"])
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                ident = send("Click dismisses")
                wait_for(lambda: card(ident))
                click(ident)
                wait_for(lambda: not live(ident))
                # Ctrl+click clears other groups and notices hidden behind the visible cap.
                ident = send("Grouped older", app="synthetic-group")
                grouped = send("Grouped newer", app="synthetic-group")
                queued = [send(f"Queued {index}", app=f"synthetic-{index}") for index in range(7)]
                wait_for(lambda: card(ident))
                assert grouped not in state()["stack"] and len(state()["stack"]) < len(queued) + 2
                click(ident, control=True)
                wait_for(lambda: not call("list")["notices"] and not state()["popups"])
                ident = send(timeout=1200)
                wait_for(lambda: card(ident))
                during = gesture(ident)
                assert during["dragging"] == ident, during
                wait_for(lambda: ident in state()["ids"] and not state()["dragging"])
                wait_for(lambda:card(ident,True) and card(ident,True)["placed"])
                wait_for(lambda:card(ident,True)["width"]==state()["positions"][ident]["width"] and card(ident,True)["height"]<300)
                wait_for(lambda:not state()["preview"])
                assert card(ident,True)["frames"] >= 2, "Preview retired before native frames rendered"
                assert ident not in state()["stack"], state()
                time.sleep(1.3)
                assert live(ident), "Floating card expired"
                click(ident, True)
                assert live(ident), "Floating background click dismissed"
                first = card(ident,True)["moves"]
                during = gesture(ident, dx=75, dy=-55, floating=True, point=(12,12))
                assert card(ident,True)["moves"]==first+1, "Body drag did not request a compositor move"
                second = state()["positions"][ident]
                call("send", notice={"title":"Updated floating card", "replace":ident, "timeout":200})
                time.sleep(.3)
                assert live(ident) and state()["positions"][ident] == second
                capture = Path("/tmp/notjik-floating-acceptance.png")
                capture.unlink(missing_ok=True)
                call("dragCapture", id=ident, path=str(capture))
                wait_for(capture.exists)
                click(ident, True, "×")
                wait_for(lambda: not live(ident) and ident not in state()["ids"])
                marker = root / "action"
                ident = send("Actions stay interactive", resident=True, actions=[{"label":"Run", "command":["touch",str(marker)]}])
                wait_for(lambda: card(ident))
                c = reveal(ident)
                b = next(b for b in c["buttons"] if b["text"]=="Run")
                gesture(ident,point=(b["x"],b["y"]))
                assert ident not in state()["ids"] and not marker.exists(), state()
                gesture(ident)
                wait_for(lambda: card(ident, True) and card(ident,True)["placed"])
                time.sleep(.2)
                initial_geometry=client(ident)
                expected=state()["positions"][ident]
                assert all(abs(a-b)<2 for a,b in zip(initial_geometry["at"],[expected["x"],expected["y"]])),(initial_geometry,expected)
                c = reveal(ident,True); assert any(b["text"]=="Run" for b in c["buttons"]), c
                b=next(b for b in c["buttons"] if b["text"]=="Run")
                first = state()["positions"][ident]
                gesture(ident,floating=True,point=(b["x"],b["y"]))
                assert state()["positions"][ident]==first and not marker.exists()
                click(ident,True,"Run")
                wait_for(marker.exists)
                assert live(ident) and ident in state()["ids"]
                # Close is a control too: dragging from it must not move or close the panel.
                c=reveal(ident,True); b=next(b for b in c["buttons"] if b["text"]=="×")
                gesture(ident,floating=True,point=(b["x"],b["y"]))
                assert state()["positions"][ident]==first and live(ident)
                # Detaching a group representative must leave its other notices in the stack.
                other=send("Same app, separate popup")
                wait_for(lambda: card(other))
                assert other in state()["stack"] and ident not in state()["stack"]
                gesture(other,dx=-100,dy=-180)
                wait_for(lambda:len(state()["ids"])==2)
                assert live(ident) and live(other)
                wait_for(lambda:card(other,True) and card(other,True)["placed"])
                # Exercise the same Hyprland dispatcher as the workspace shortcuts,
                # targeting only the synthetic window and keeping desktop focus in place.
                first_client=client(ident)
                # Hover has resized the native window since its initial placement.
                assert first_client and first_client["floating"] and not first_client["pinned"], first_client
                target="name:notjik-acceptance-"+str(os.getpid())
                address=first_client["address"]
                def dispatch(code):
                    result=subprocess.check_output(["hyprctl","eval",code],text=True)
                    assert result.strip()=="ok",result
                dispatch('hl.dispatch(hl.dsp.window.move({workspace='+json.dumps(target)+',follow=false,window="address:'+address+'"}))')
                wait_for(lambda:client(ident)["workspace"]["name"]==target[5:])
                call("send",notice={"title":"Updated on another workspace","replace":ident,"timeout":100})
                third=send("New arrival must not replace the detached window")
                time.sleep(.3)
                updated=client(ident)
                assert updated["address"]==address and updated["workspace"]["name"]==target[5:] and not updated["pinned"],updated
                assert third in state()["stack"] and len(state()["ids"])==2
                # Compositor-native geometry is retained when another card is detached.
                second_address=client(other)["address"]
                dispatch('hl.dispatch(hl.dsp.window.move({x=150,y=160,relative=false,window="address:'+second_address+'"}))')
                wait_for(lambda:client(other)["at"]==[150,160])
                gesture(third,dx=-100,dy=-180)
                wait_for(lambda:card(third,True) and card(third,True)["placed"])
                assert client(other)["address"]==second_address and client(other)["at"]==[150,160]
                # Closing through the compositor follows the normal notification close path.
                dispatch('hl.dispatch(hl.dsp.window.close({window="address:'+address+'"}))')
                wait_for(lambda:not live(ident) and ident not in state()["ids"])
                ident=other
                call("testLock",locked=True)
                assert live(ident)
                call("testLock",locked=False)
                call("dismiss",id="all")
                wait_for(lambda:not state()["ids"] and not state()["popups"])
                final_marker=root / "final-action"
                final_id=send("Action dismisses floating notice",actions=[{"label":"Finish", "command":["touch",str(final_marker)]}])
                wait_for(lambda:card(final_id))
                gesture(final_id)
                wait_for(lambda:card(final_id,True) and card(final_id,True)["placed"])
                click(final_id,True,"Finish")
                wait_for(lambda:final_marker.exists() and not live(final_id))
                wait_for(lambda:not state()["ids"] and not state()["popups"])
                ident = send("Ctrl+click floating notice")
                wait_for(lambda: card(ident))
                gesture(ident)
                wait_for(lambda: card(ident, True) and card(ident, True)["placed"])
                other = send("Outstanding popup")
                wait_for(lambda: card(other))
                click(ident, True)
                assert live(ident) and live(other), "Plain floating click dismissed notices"
                click(ident, True, control=True)
                wait_for(lambda: not call("list")["notices"] and not state()["ids"] and not state()["popups"])
                time.sleep(.1)
                print("PASS: click, Ctrl+click clears grouped/queued/floating notices, detach, repeat drag, timeout pinning, updates, close, action exclusions and activation, grouping, multiple panels, native window identity, workspace transfer, compositor movement and close, cleanup")
            finally:
                process.terminate()
                process.wait(timeout=5)
                log.seek(0)
                output=log.read()
                problems=[line for line in output.splitlines() if any(word in line for word in (" ERROR", "ReferenceError", "TypeError", "Binding loop", "Unable to assign"))]
                if problems or sys.exc_info()[0]:
                    print(output,file=sys.stderr)
                if problems:
                    raise AssertionError("QML runtime errors: " + "\n".join(problems))


if __name__ == "__main__":
    run()
