"""Capture the retained original QML using the same synthetic parity fixture."""

import json, os, shutil, subprocess, tempfile, time, sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
root = Path(
    os.environ.get("TASKTJIK_PARITY_DIR")
    or tempfile.mkdtemp(prefix="tasktjik-reference-")
)
root.mkdir(parents=True, exist_ok=True)
qml = root / "qml"
shutil.copytree(REPO / "apps/tasktjik/qml", qml, dirs_exist_ok=True)
fixture = json.loads((Path(__file__).parent / "fixtures/parity.json").read_text())
(root / "fixture.json").write_text(json.dumps(fixture))
(qml / "shell.qml").write_text(
    """import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
 FloatingWindow { id:window; title:"Tasktjik reference"; implicitWidth:1000;implicitHeight:680;color:'#26233a'
 Rectangle {id:canvas;anchors.fill:parent;color:"#26233a"
 TodoApp {id:view;anchors.fill:parent;anchors.margins:20;focus:true;view:'all';theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'});stateData:FIXTURE}
 }}
 IpcHandler {target:"reference";function capture(path:string):void {canvas.grabToImage(r=>r.saveToFile(path));} function editor():void {view.newDefault();} }
}""".replace("FIXTURE", json.dumps(fixture))
)
(root / "runtime").mkdir(exist_ok=True, mode=0o700)
env = {
    **os.environ,
    "QT_QPA_PLATFORM": "offscreen",
    "QT_QUICK_BACKEND": "software",
    "QT_QUICK_CONTROLS_STYLE": "Basic",
    "XDG_RUNTIME_DIR": str(root / "runtime"),
    "XDG_CONFIG_HOME": str(root / "config"),
}
with (root / "qml.log").open("w") as log:
    p = subprocess.Popen(
        ["quickshell", "--path", str(qml)], env=env, stdout=log, stderr=log
    )
    try:
        time.sleep(1)
        for name in ["reference", "reference-editor"]:
            if name.endswith("editor"):
                subprocess.run(
                    [
                        "quickshell",
                        "ipc",
                        "--path",
                        str(qml),
                        "call",
                        "reference",
                        "editor",
                    ],
                    env=env,
                    check=True,
                )
            time.sleep(0.4)
            subprocess.run(
                [
                    "quickshell",
                    "ipc",
                    "--path",
                    str(qml),
                    "call",
                    "reference",
                    "capture",
                    str(root / (name + ".png")),
                ],
                env=env,
                check=True,
            )
            time.sleep(0.3)
    finally:
        p.terminate()
        p.wait()
print(root)
