"""Exercise mouse-wheel momentum and interruption in the rendered native task list."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix="tasktjik-scroll-ui-"))
(root / "runtime").mkdir(mode=0o700)
home = root / "qml"
shutil.copytree(APP / "qml", home)
tasks = [{"id": str(i), "title": "Synthetic scrolling task " + str(i), "kind": "todo",
          "group": "All", "tags": [], "due": "", "repeat": {}, "reference": "", "revision": 1,
          "status": "ready", "done": False, "subtasks": []} for i in range(100)]
(home / "shell.qml").write_text('''import QtQuick
import QtTest
import Quickshell
import Quickshell.Io
ShellRoot {
    TestCase {id:input;when:false}
    FloatingWindow {
        implicitWidth:700;implicitHeight:700;color:'#26233a'
        TodoApp {id:view;anchors.fill:parent;view:'all';stateData:STATE
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
        }
    }
    IpcHandler {target:'scroll-test'
        function ready():string {return JSON.stringify({height:input.findChild(view,'task-scroll').contentItem.contentHeight});}
        function measure(count:int,spacing:int):string {
            const scroll=input.findChild(view,'task-scroll'),f=scroll.contentItem;
            scroll.stopWheelScroll();f.cancelFlick();f.contentY=1000;input.wait(50);
            for(let i=0;i<count;i++) {
                input.mouseWheel(scroll,scroll.width-30,scroll.height/2,0,-120,Qt.NoButton,Qt.NoModifier,0);
                if(i<count-1)input.wait(spacing);
            }
            const immediate=f.contentY;input.wait(100);const gliding=f.contentY;
            input.wait(250);const settled=f.contentY,movingAt350=f.moving;
            input.wait(1200);
            return JSON.stringify({immediate:immediate,gliding:gliding,settled:settled,movingAt350:movingAt350,final:f.contentY,moving:f.moving});
        }
        function interrupt():string {
            const scroll=input.findChild(view,'task-scroll'),f=scroll.contentItem;
            scroll.stopWheelScroll();f.contentY=1500;
            input.mouseWheel(scroll,scroll.width-30,scroll.height/2,0,-240,Qt.NoButton,Qt.NoModifier,0);
            input.wait(80);const down=f.contentY;
            input.mouseWheel(scroll,scroll.width-30,scroll.height/2,0,120,Qt.NoButton,Qt.NoModifier,0);
            input.wait(80);const reversed=f.contentY;
            input.mouseClick(scroll,scroll.width-30,scroll.height/2);
            const stopped=f.contentY;input.wait(300);
            return JSON.stringify({down:down,reversed:reversed,stopped:stopped,final:f.contentY});
        }
        function motion():string {
            const scroll=input.findChild(view,'task-scroll'),f=scroll.contentItem;
            scroll.stopWheelScroll();f.contentY=1000;input.wait(50);
            const samples=[f.contentY],jumps=[];
            for(let notch=0;notch<3;notch++) {
                const before=f.contentY;
                input.mouseWheel(scroll,scroll.width-30,scroll.height/2,0,-120,Qt.NoButton,Qt.NoModifier,0);
                jumps.push(f.contentY-before);
                for(let frame=0;frame<8;frame++){input.wait(8);samples.push(f.contentY);}
            }
            for(let frame=0;frame<50;frame++){input.wait(8);samples.push(f.contentY);}
            return JSON.stringify({jumps:jumps,samples:samples,moving:f.moving});
        }
        function edge(bottom:bool,distance:int):string {
            const scroll=input.findChild(view,'task-scroll'),f=scroll.contentItem;
            scroll.stopWheelScroll();
            const low=f.originY,high=low+f.contentHeight-f.height;
            const boundary=bottom?high:low,delta=bottom?-120:120;
            f.contentY=boundary+(bottom?-distance:distance);
            const samples=[f.contentY];
            input.mouseWheel(scroll,scroll.width-30,scroll.height/2,0,delta,Qt.NoButton,Qt.NoModifier,0);
            for(let i=0;i<60;i++){input.wait(8);samples.push(f.contentY);}
            let restarted=false;
            for(let i=0;i<5;i++) {
                input.mouseWheel(scroll,scroll.width-30,scroll.height/2,0,delta,Qt.NoButton,Qt.NoModifier,0);
                restarted=restarted || f.moving;
                input.wait(8);samples.push(f.contentY);
            }
            const final=f.contentY;
            input.mouseWheel(scroll,scroll.width-30,scroll.height/2,0,-delta,Qt.NoButton,Qt.NoModifier,0);
            input.wait(350);
            return JSON.stringify({samples:samples,low:low,high:high,boundary:boundary,final:final,
                restarted:restarted,reverseTravel:Math.abs(f.contentY-boundary),moving:f.moving});
        }
        function controls():string {
            const scroll=input.findChild(view,'task-scroll');
            scroll.stopWheelScroll();scroll.contentItem.contentY=0;input.wait(50);
            const row=view.visualRows().find(r=>r.y>=0 && r.y<scroll.height-50);
            view.selectedId='';input.mouseClick(scroll,row.x+120,row.y+24);
            if(view.selectedId!==row.id)throw Error('Task click was intercepted');
            const draft=JSON.parse(JSON.stringify(view.stateData.tasks[0]));
            draft.subtasks=Array.from({length:40},(_,i)=>({id:'step-'+i,title:'Synthetic step '+i,kind:'task',done:false,due:'',items:[]}));
            view.openEditor(draft);input.wait(100);
            const editor=input.findChild(view,'editor-scroll');
            editor.contentItem.contentY=0;
            input.mouseWheel(editor,editor.width-30,editor.height/2,0,-120,Qt.NoButton,Qt.NoModifier,0);
            input.wait(600);
            const offset=editor.contentItem.contentY;
            if(offset<80)throw Error('Editor did not scroll');
            view.cancelEditor();input.wait(30);
            return 'PASS';
        }
        function gestures():string {
            const scroll=input.findChild(view,'task-scroll');
            const events=[{pixelDelta:{x:0,y:12},angleDelta:{x:0,y:120},phase:Qt.NoScrollPhase,modifiers:Qt.NoModifier},
                {pixelDelta:{x:0,y:0},angleDelta:{x:0,y:120},phase:Qt.ScrollMomentum,modifiers:Qt.NoModifier},
                {pixelDelta:{x:0,y:0},angleDelta:{x:120,y:0},phase:Qt.NoScrollPhase,modifiers:Qt.NoModifier},
                {pixelDelta:{x:0,y:0},angleDelta:{x:0,y:120},phase:Qt.NoScrollPhase,modifiers:Qt.ControlModifier}];
            return JSON.stringify(events.map(event=>{scroll.scrollWheel(event);return event.accepted;}));
        }
    }
}
'''.replace("STATE", json.dumps({"tasks": tasks, "groups": ["All"], "today": "2099-01-01"})))
env = {**os.environ, "QT_QPA_PLATFORM": "offscreen", "QT_QUICK_BACKEND": "software",
       "XDG_RUNTIME_DIR": str(root / "runtime"), "XDG_CONFIG_HOME": str(root / "config"),
       "XDG_CACHE_HOME": str(root / "cache")}


def ipc(method, *args):
    return subprocess.check_output(["quickshell", "ipc", "--path", str(home), "call",
                                    "scroll-test", method, *map(str, args)], env=env,
                                   text=True, stderr=subprocess.DEVNULL, timeout=10)


with (root / "qml.log").open("w") as log:
    process = subprocess.Popen(["quickshell", "--path", str(home)], env=env, stdout=log, stderr=log)
    try:
        for _ in range(50):
            try:
                if json.loads(ipc("ready"))["height"] > 3000:
                    break
            except (subprocess.CalledProcessError, ValueError):
                if process.poll() is not None:
                    raise AssertionError((root / "qml.log").read_text())
            time.sleep(.1)
        else:
            raise AssertionError("Scrolling fixture did not start: " + str(root))
        for bottom in [False, True]:
            for distance in [10, 120]:
                edge = json.loads(ipc("edge", str(bottom).lower(), distance))
                assert all(edge["low"] - .1 <= y <= edge["high"] + .1 for y in edge["samples"]), edge
                direction = 1 if bottom else -1
                assert all(direction * (b - a) >= -.1 for a, b in zip(edge["samples"], edge["samples"][1:])), edge
                assert abs(edge["final"] - edge["boundary"]) < 1 and not edge["restarted"], edge
                assert 130 < edge["reverseTravel"] < 190 and not edge["moving"], edge
        single = json.loads(ipc("measure", 1, 0))
        slow = json.loads(ipc("measure", 3, 700))
        fast = json.loads(ipc("measure", 3, 40))
        print(json.dumps({"single": single, "slow": slow, "fast": fast}), flush=True)
        assert 130 < single["final"] - 1000 < 190, single
        assert abs(single["immediate"] - 1000) < 1, single
        assert not single["movingAt350"] and abs(single["settled"] - single["final"]) < 1, single
        assert single["gliding"] > single["immediate"] and single["final"] > single["gliding"], single
        assert 1.05 < (fast["final"] - 1000) / (slow["final"] - 1000) < 1.5, (slow, fast)
        assert not fast["moving"] and not slow["moving"], (slow, fast)
        motion = json.loads(ipc("motion"))
        assert all(abs(jump) < 1 for jump in motion["jumps"]), motion
        steps = [b - a for a, b in zip(motion["samples"], motion["samples"][1:])]
        assert all(step >= 0 for step in steps) and sum(step > 0 for step in steps) >= 10, motion
        assert not motion["moving"], motion
        interrupted = json.loads(ipc("interrupt"))
        assert interrupted["reversed"] < interrupted["down"], interrupted
        assert abs(interrupted["final"] - interrupted["stopped"]) < 1, interrupted
        assert json.loads(ipc("gestures")) == [False] * 4
        assert ipc("controls").strip() == "PASS"
        errors = (root / "qml.log").read_text()
        assert not any(word in errors for word in ["TypeError", "ReferenceError", "Binding loop", "Cannot assign"]), errors
        print("PASS wheel travel, rapid acceleration, momentum, reversal, click stop, edge and gesture passthrough:", root)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
