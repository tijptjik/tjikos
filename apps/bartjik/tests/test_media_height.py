"""Render media state changes without letting separator font metrics resize the bar."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("quickshell"), "Quickshell is required")
class MediaHeightTests(unittest.TestCase):
    def test_idle_playing_paused_and_missing_artist_keep_the_same_height(self):
        with tempfile.TemporaryDirectory(prefix="bartjik-media-height-") as directory:
            work = Path(directory)
            shutil.copytree(ROOT / "qml", work / "qml")
            theme = json.loads((ROOT / "themes/rose-punk.json").read_text())
            # Exercise the production island, including the separator's actual font metrics.
            fixture = '''
import QtQuick
import QtQuick.Window
import Quickshell
import Quickshell.Io
import "qml" as Production
ShellRoot {
    id: root
    property int step: 0
    property var samples: []
    property var style: THEME
    property var row: ({id:'media', idle:true, type:'icon', icon:{kind:'symbol',value:'play_arrow'}})
    QtObject {
        id: controller
        function displayItems(bar, id) { return [root.row]; }
        function connectivityPeer() { return ''; }
    }
    FileView { id: report; path: Qt.resolvedUrl('result.json').toString().replace('file://',''); blockWrites:true }
    Window {
        id: window
        visible:true; width:700; height:160
        Production.Island {
            id: island
            definition:({modules:['music']})
            bar:({key:'demo',id:'demo',screen:'demo',thickness:44,style:root.style,
                moduleConfigs:{music:{provider:'playerctl'}}})
            controller:controller; window:window; horizontal:true
        }
    }
    Timer {
        interval:200; running:true; repeat:true
        onTriggered: {
            root.samples.push({height:island.height,width:island.width});
            if (++root.step === 5) {
                report.setText(JSON.stringify(root.samples)); Qt.quit(); return;
            }
            root.row = root.step === 4
                ? {id:'media',idle:true,type:'icon',icon:{kind:'symbol',value:'play_arrow'}}
                : {id:'media',type:'icon-text',icon:{kind:'symbol',value:root.step===2?'play_arrow':'pause'},
                    artist:{value:root.step===3?'':'Synthetic artist'},track:{value:'Synthetic track'},
                    text:{value:'Synthetic track'}};
        }
    }
}
'''.replace("THEME", json.dumps(theme))
            (work / "shell.qml").write_text(fixture)
            env = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software",
                       XDG_RUNTIME_DIR=str(work), XDG_CACHE_HOME=str(work / "cache"),
                       XDG_CONFIG_HOME=str(work / "config"))
            result = subprocess.run(["quickshell", "--path", str(work), "--no-color"],
                                    env=env, text=True, capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue((work / "result.json").exists(), result.stdout + result.stderr)
            samples = json.loads((work / "result.json").read_text())
            self.assertEqual([sample["height"] for sample in samples], [44] * 5)
            self.assertGreater(samples[1]["width"], samples[0]["width"])
            self.assertEqual(samples[1]["width"], samples[2]["width"])
            self.assertLess(samples[3]["width"], samples[2]["width"])
            self.assertEqual(samples[0], samples[4])


if __name__ == "__main__":
    unittest.main()
