"""Exercise production native-row bindings with synthetic services in real Qt."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which('quickshell'), 'Quickshell is required')
class NativeCacheTests(unittest.TestCase):
    def test_native_rows_survive_provider_updates_and_track_service_changes(self):
        source = (ROOT / 'qml/Native.qml').read_text()
        methods = source[source.index('    function base('):source.index('    function snapshot(')]
        shell = (ROOT / 'qml/shell.qml').read_text()
        configured = shell.split('configuredModules:', 1)[1].split('hyprvoiceEnabled:', 1)[0].strip()
        methods = methods.replace('Status.NeedsAttention', '2').replace(
            'property var configuredModules: []', 'property var configuredModules: ' + configured)
        with tempfile.TemporaryDirectory(prefix='bartjik-native-cache-') as directory:
            root = Path(directory)
            shutil.copyfile(ROOT / 'qml/TrayIcons.js', root / 'TrayIcons.js')
            shutil.copyfile(ROOT / 'qml/Media.js', root / 'Media.js')
            (root / 'shell.qml').write_text('''
import QtQuick
import Quickshell
import Quickshell.Io
import "TrayIcons.js" as TrayIcons
import "Media.js" as Media
ShellRoot {
    id:root
    property var bars:[{moduleConfigs:{native:root.settings}}]
    property int ticks:0
    property var firstRows:null
    property var settings:({provider:'icontray',exclude:['Input Method'],iconOverrides:{}})
    QtObject {id:trayItem;property string id:'synthetic';property string title:'Synthetic'
        property string icon:'test';property string tooltipTitle:'Original'
        property string tooltipDescription:'';property int status:0}
    QtObject {id:player;property string dbusName:'synthetic-player'
        property string identity:'Spotify';property string desktopEntry:'spotify'
        property bool isPlaying:true;property string trackTitle:'First track'
        property string trackArtist:'Synthetic artist';property string trackAlbumArtist:''
        property string trackAlbum:'';property string trackArtUrl:'';property var metadata:({})
        property real length:100;property bool canTogglePlaying:true;property bool canGoPrevious:true
        property bool canSeek:true;property bool canGoNext:true;property bool shuffleSupported:true
        property bool canControl:true;property bool shuffle:false}
    QtObject {
        id:native
        property var tray:[trayItem]
        property var trayRecords:[{id:'sni-1',object:trayItem}]
        property var attention:({})
        property var players:[]
        METHODS
    }
    FileView {id:result;path:Qt.resolvedUrl('result.txt').toString().replace('file://','');blockWrites:true}
    function require(ok,message){if(!ok){console.error(message);Qt.exit(1);}}
    Timer {interval:1;running:true;repeat:true;onTriggered:{
        const rows=native.items(root.settings);
        if(!root.firstRows)root.firstRows=rows;
        // Model the repeated consumers invalidated by unrelated provider updates.
        for(let i=0;i<200;++i)root.require(native.items(root.settings)===root.firstRows,'native rows reallocated');
        if(++root.ticks<500)return;
        root.require(rows.length===1 && rows[0].tooltip==='Original','initial row');
        trayItem.tooltipTitle='Changed';
        root.require(native.items(root.settings)[0].tooltip==='Changed','stale title');
        native.attention={synthetic:true};
        root.require(native.items(root.settings)[0].state.urgent,'stale attention');
        root.settings={provider:'icontray',exclude:['SYN*'],iconOverrides:{}};
        root.require(native.items(root.settings).length===0,'stale exclusion');
        root.settings={provider:'icontray',exclude:[],iconOverrides:{synthetic:'builtin:telegram'}};
        root.require(native.items(root.settings)[0].icon.kind==='image','stale override');
        native.tray=[];
        root.require(native.items(root.settings).length===0,'removed tray item retained');
        root.settings={provider:'playerctl',players:['spotify']};
        native.players=[player];
        root.require(native.items(root.settings)[0].text.value==='First track','initial player');
        player.trackTitle='Second track';
        root.require(native.items(root.settings)[0].text.value==='Second track','stale track');
        player.isPlaying=false;
        root.require(native.items(root.settings)[0].idle,'stale playback');
        native.players=[];
        root.require(native.items(root.settings)[0].id==='spotify-idle','removed player retained');
        result.setText('PASS');Qt.quit();
    }}
}
'''.replace('METHODS', methods))
            completed = subprocess.run(['quickshell', '--path', str(root), '--no-color'],
                env={**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software',
                     'XDG_RUNTIME_DIR': str(root)}, capture_output=True, text=True, timeout=20)
            self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
            self.assertEqual((root / 'result.txt').read_text(), 'PASS')


if __name__ == '__main__':
    unittest.main()
