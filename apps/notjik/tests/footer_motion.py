"""Render a detached synthetic card offscreen and check its footer animation.

Run: python3 -B tests/footer_motion.py
No desktop, notification service, or private data is used.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from native_fixture import copy_file


ROOT = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="notjik-footer-") as temp:
        root = Path(temp)
        shutil.copytree(ROOT / "qml", root / "qml", copy_function=copy_file)
        panel = root / "qml/FloatingPanel.qml"
        panel.write_text(panel.read_text().replace("property bool placed: false", "property bool placed: true")
                         .replace("    id: panel", """    id: panel
    function sample() {
        return {windowHeight:height, requestedHeight:implicitHeight, minimum:minimumSize.height,
            maximum:maximumSize.height, cardHeight:card.height, top:card.y,
            body:card.bodyHeight, admission:card.admissionHeight};
    }
""", 1))
        card = root / "qml/NotificationCard.qml"
        code = card.read_text()
        start = code.index("    readonly property bool controlsRevealed:")
        end = code.index("    readonly property bool hasInteractions:", start)
        card.write_text(code[:start] + "    readonly property bool controlsRevealed: controller.testOpen\n" + code[end:])
        theme = json.loads((ROOT / "themes/rose-punk.json").read_text())
        shell = """import QtQuick
import Quickshell
ShellRoot {
    id: root
    property bool testOpen: false
    property bool ready: true
    property bool locked: false
    property bool keyboardMode: false
    property string draggingId: ""
    property string dragPreviewId: ""
    property string session: "footer-test"
    property double now: Date.now()
    property var floatingPositions: ({demo:{width:420}})
    property var floatingStates: ({})
    property var avatarCache: ({})
    property var config: ({components:{},actions:{},stack:{animationMs:220}})
    property var theme: THEME
    property var notices: [{id:"demo",title:"Transfer progress · synthetic example",
        body:"Example service",app:"notjik demo",type:"progress",urgency:"normal",
        icon:"",image:"",data:{value:67,demo:true},actions:[{label:"View details",command:["true"]}]}]
    function setHover(monitor, id, hovered) {}
    function dismissNotice(id) {}
    function unfocus() {}
    FloatingPanel { id: panel; controller: root; noticeId: "demo" }
    property int elapsed: 0
    Timer {
        interval: 16; running: true; repeat: true
        onTriggered: {
            root.elapsed += 16;
            // Full open/close, then reverse twice before the animation finishes.
            root.testOpen = (root.elapsed >= 320 && root.elapsed < 640)
                || (root.elapsed >= 960 && root.elapsed < 1008) || root.elapsed >= 1056;
            console.warn("FOOTER_SAMPLE " + JSON.stringify(Object.assign({time:root.elapsed}, panel.sample())));
            if (root.elapsed >= 1408) Qt.quit();
        }
    }
}
""".replace("THEME", json.dumps(theme))
        (root / "qml/shell.qml").write_text(shell)
        env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software",
                   QT_QUICK_CONTROLS_STYLE="Basic", QT_ACCESSIBILITY="0",
                   XDG_RUNTIME_DIR=str(root), XDG_CACHE_HOME=str(root / "cache"))
        result = subprocess.run(["quickshell", "--path", str(root / "qml"), "--no-color"],
                                env=env, text=True, capture_output=True, timeout=15)
        output = result.stdout + result.stderr
        assert result.returncode == 0, output
        assert not any(error in output for error in ("ReferenceError", "TypeError", "Binding loop", "Unable to assign", "ERROR:")), output
        samples = [json.loads(line.split("FOOTER_SAMPLE ", 1)[1]) for line in output.splitlines()
                   if "FOOTER_SAMPLE " in line]
        samples = [s for s in samples if s["time"] >= 160]
        assert samples, output
        for key in ("windowHeight", "requestedHeight", "minimum", "maximum", "top", "body", "admission"):
            assert max(s[key] for s in samples) - min(s[key] for s in samples) < .01, (key, samples)
        closed = samples[0]["cardHeight"]
        opened = samples[-1]["cardHeight"]
        assert opened > closed + 20, samples
        assert abs(opened - samples[-1]["admission"]) < .01, samples[-1]
        for start, end, direction in ((320, 624, 1), (640, 944, -1), (1056, 1408, 1)):
            heights = [s["cardHeight"] for s in samples if start <= s["time"] <= end]
            assert any(closed + .1 < h < opened - .1 for h in heights), heights
            assert all((b - a) * direction >= -.01 for a, b in zip(heights, heights[1:])), heights
        assert all(closed - .01 <= s["cardHeight"] <= opened + .01 for s in samples), samples
        print("PASS: footer opens, closes and reverses without overshoot or native window resizing")


if __name__ == "__main__":
    main()
