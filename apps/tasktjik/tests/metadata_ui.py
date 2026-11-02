"""Render read-only metadata with synthetic tasks and check UI save isolation."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix="tasktjik-metadata-ui-"))
(root / "runtime").mkdir(mode=0o700)
home = root / "qml"
shutil.copytree(APP / "qml", home)
metadata = {"external_id": "synthetic-42", "nested": {"enabled": False},
            "values": [None, 3, "<b>literal</b>"], "note": "first\nsecond"}
task = {"id": "fixture", "title": "Synthetic metadata task", "kind": "todo",
        "group": "All", "tags": [], "due": "", "repeat": {}, "reference": "",
        "revision": 1, "status": "ready", "done": False, "subtasks": [], "metadata": metadata}
state = {"today": "2099-01-01", "groups": ["All"], "tasks": [task]}
(home / "shell.qml").write_text('''import QtQuick
import QtTest
import Quickshell
import Quickshell.Io
ShellRoot {
    TestCase {id:input;when:false}
    FloatingWindow {
        implicitWidth:700;implicitHeight:800;color:'#26233a'
        TodoApp {id:view;anchors.fill:parent;property var lastRequest:({})
            onRequest:value=>lastRequest=value
            view:'all';stateData:STATE
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
        }
    }
    IpcHandler {target:'metadata-test'
        function open():string {
            view.selectedId='fixture';view.openEditor();input.wait(100);
            const footer=input.findChild(view,'task-metadata');
            return JSON.stringify({text:footer.text,visible:footer.visible,font:footer.font.family,
                plain:footer.textFormat===Text.PlainText,tabFocus:footer.activeFocusOnTab});
        }
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
        function save():string {
            view.saveEditor();
            if(!view.pendingSave)throw Error('Missing save');
            return JSON.stringify(view.pendingSave.request);
        }
        function empty():string {
            view.pendingSave=null;view.editor='';
            const task=JSON.parse(JSON.stringify(view.stateData.tasks[0]));task.metadata={};
            view.openEditor(task);input.wait(20);
            const footer=input.findChild(view,'task-metadata');
            if(footer.visible || footer.text!=='')throw Error('Empty metadata displayed');
            view.editor='';delete task.metadata;view.openEditor(task);input.wait(20);
            if(footer.visible || footer.text!=='')throw Error('Legacy task metadata displayed');
            return 'PASS';
        }
    }
}
'''.replace('STATE', json.dumps(state)))
env = {**os.environ, "QT_QPA_PLATFORM": "offscreen", "QT_QUICK_BACKEND": "software",
       "XDG_RUNTIME_DIR": str(root / "runtime"), "XDG_CONFIG_HOME": str(root / "config"),
       "XDG_CACHE_HOME": str(root / "cache")}


def ipc(method, *args):
    return subprocess.check_output(["quickshell", "ipc", "--path", str(home), "call",
                                    "metadata-test", method, *map(str, args)], env=env,
                                   text=True, stderr=subprocess.DEVNULL, timeout=10)


with (root / "qml.log").open("w") as log:
    process = subprocess.Popen(["quickshell", "--path", str(home)], env=env, stdout=log, stderr=log)
    try:
        for _ in range(50):
            try:
                result = json.loads(ipc("open"))
                break
            except (subprocess.CalledProcessError, ValueError):
                if process.poll() is not None:
                    raise AssertionError((root / "qml.log").read_text())
                time.sleep(.1)
        else:
            raise AssertionError("Metadata fixture did not start: " + str(root))
        expected = "\n".join(key + " : " + json.dumps(metadata[key], separators=(",", ":")) for key in sorted(metadata))
        assert result == {"text": expected, "visible": True, "font": "monospace", "plain": True, "tabFocus": False}, result
        ipc("capture", root / "metadata.png")
        for _ in range(50):
            if (root / "metadata.png").exists():
                break
            time.sleep(.1)
        assert (root / "metadata.png").exists()
        saved = json.loads(ipc("save"))
        assert saved["action"] == "edit" and "metadata" not in saved["fields"], saved
        assert ipc("empty").strip() == "PASS"
        errors = (root / "qml.log").read_text()
        assert not any(word in errors for word in ["TypeError", "ReferenceError", "Binding loop", "Cannot assign"]), errors
        print("PASS literal monospace footer, empty/legacy hiding and read-only save:", root)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
