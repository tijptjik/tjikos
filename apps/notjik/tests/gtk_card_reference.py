"""Render original QML cards with the GTK gallery payloads; synthetic/private only."""
import json
from pathlib import Path
import re
import subprocess
import tempfile


def capture_reference(root, artifacts, env):
    with tempfile.TemporaryDirectory(prefix="notjik-qml-reference-") as temp:
        work = Path(temp)
        qml = work / "qml"
        qml.mkdir()
        for source in (root / "qml").iterdir():
            if source.name != "NotificationCard.qml":
                (qml / source.name).symlink_to(source)
        card = (root / "qml/NotificationCard.qml").read_text()
        card = re.sub(r"readonly property bool controlsRevealed:.*?(?=    readonly property bool hasInteractions)", "readonly property bool controlsRevealed: controller.captureRevealed\n", card, flags=re.S)
        (qml / "NotificationCard.qml").write_text(card)
        theme = json.loads((root / "themes/rose-punk.json").read_text())
        notices = json.loads((artifacts / "rose-punk/fixtures.json").read_text())
        target = artifacts / "qml-reference"
        target.mkdir()
        for index in [0, 4, 8, 10, 11, 13]:
            notice = {"body": "", "icon": "", "image": "", "urgency": "normal", "actions": [], "archived": False, **notices[index]}
            for revealed in [False, True]:
                output = target / f'{"revealed" if revealed else "card"}-{index:02}.png'
                shell = f'''import QtQuick
import Quickshell
import "qml" as Cards
ShellRoot {{
    FloatingWindow {{
        color: "transparent"
        implicitWidth: 418
        implicitHeight: Math.ceil(card.implicitHeight)
        Cards.NotificationCard {{ id: card; width: 418; height: implicitHeight; monitorName: "synthetic"; controller: owner; notice: {json.dumps(notice)} }}
        Item {{ id: owner
            property var theme: {json.dumps(theme)}
            property var config: ({{components:{{}},handlers:[],actions:{{blacklist:[]}}}})
            property var avatarCache: ({{}})
            property double now: {notice['received']}
            property bool locked: false
            property bool paused: false
            property bool keyboardMode: false
            property bool captureRevealed: {str(revealed).lower()}
            property string selectedId: ""
            property string keyboardMonitor: ""
            property string historySelectedKey: ""
            property var telegramPlayer: player
            property var telegramRecorder: recorder
            function setHover(m,id,hover) {{}}
            function targetMonitor() {{return "synthetic";}}
        }}
        Cards.TelegramPlayback {{id: player;locked:false}}
        QtObject {{id: recorder;property string noticeId:"";property string phase:"idle";property bool recording:false;property bool holdsNotice:false;property double duration:0;property bool uncertain:false;property string error:""}}
        Timer {{interval:700;running:true;onTriggered:card.grabToImage(result=>{{result.saveToFile({json.dumps(str(output))});Qt.quit();}})}}
    }}
}}
'''
                path = work / "shell.qml"
                path.write_text(shell)
                result = subprocess.run(["quickshell", "--path", str(path), "--no-duplicate"], env=env, capture_output=True, text=True, timeout=10)
                (target / (output.stem + ".log")).write_text(result.stdout + result.stderr)
                assert result.returncode == 0 and output.exists(), result.stdout + result.stderr
