"""Render-size changes must preserve both loaded artwork slots."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which('quickshell'), 'Quickshell is required')
class AlbumArtResizeTests(unittest.TestCase):
    def test_resize_during_display_and_crossfade_does_not_reload(self):
        with tempfile.TemporaryDirectory(prefix='album-art-resize-') as directory:
            temp = Path(directory)
            shutil.copy(ROOT / 'qml/AlbumArt.qml', temp)
            for name, color in [('first', 'red'), ('second', 'blue')]:
                (temp / (name + '.svg')).write_text(
                    f'<svg xmlns="http://www.w3.org/2000/svg" width="600" height="600">'
                    f'<rect width="600" height="600" fill="{color}"/></svg>')
            (temp / 'shell.qml').write_text('''
import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    QtObject { id:controller; signal artworkReceived(string token,string source) }
    AlbumArt {
        id:art; controller:controller; width:420; height:350
        requestedSource:Qt.resolvedUrl("first.svg").toString()
    }
    FileView { id:resultFile; path:Qt.resolvedUrl("result.txt").toString().replace("file://", ""); blockWrites:true }
    property int steps:0
    property bool started:false
    property bool crossfadeChecked:false
    function fail(message) { resultFile.setText("RESIZE_FAIL: "+message); Qt.quit(); }
    function resize() {
        const image=art.current;
        const decodedWidth=image.sourceSize.width, decodedHeight=image.sourceSize.height;
        art.width=220+(steps%7)*93; art.height=180+(steps%5)*67;
        if(image.sourceSize.width!==decodedWidth || image.sourceSize.height!==decodedHeight)
            fail("resize changed decode size");
        if(image.status!==Image.Ready || !art.ready)fail("displayed cover disappeared");
    }
    Connections {
        target:art
        function onReadyChanged() { if(started && !art.ready)fail("cover became unready"); }
    }
    Timer {
        interval:16; repeat:true; running:true
        onTriggered: {
            if(!started) { if(!art.ready || art.transitioning)return; started=true; }
            ++steps;
            resize();
            if(steps===10)art.requestedSource=Qt.resolvedUrl("second.svg").toString();
            if(art.transitioning) {
                crossfadeChecked=true;
                if(art.incoming.status!==Image.Ready)fail("incoming cover disappeared");
            }
            if(steps===60) {
                if(!crossfadeChecked || !art.displayedSource.endsWith("second.svg"))
                    fail("track transition did not complete");
                else { resultFile.setText("RESIZE_PASS"); Qt.quit(); }
            }
        }
    }
    Timer { interval:5000; running:true; onTriggered:fail("load timeout") }
}
''')
            result = subprocess.run(
                ['quickshell', '--path', str(temp), '--no-color', '--log-rules', 'qml.*=true'],
                env={**os.environ, 'QT_QPA_PLATFORM': 'offscreen',
                     'QT_QUICK_BACKEND': 'software'},
                capture_output=True, text=True, timeout=10)
            output = result.stdout + result.stderr
            self.assertEqual(result.returncode, 0, output)
            self.assertTrue((temp / 'result.txt').exists(), output)
            self.assertEqual((temp / 'result.txt').read_text(), 'RESIZE_PASS', output)
