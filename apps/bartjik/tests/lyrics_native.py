"""Exercise the real MPRIS position getter and signals, with no external lyrics."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from render import wait_for

ROOT = Path(__file__).resolve().parents[1]


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-lyrics-native-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        # Reuse the existing private-bus fixture; extend only this temporary copy.
        fixture = (ROOT / 'tests/native_fixture.py').read_text()
        fixture = fixture.replace('dbus.ObjectPath("/track/1")', 'dbus.ObjectPath("/com/spotify/track/1234567890abcdefghijkl")')
        fixture = fixture.replace('self.position = 0', "self.position = 0\n        self.paused = False")
        fixture = fixture.replace('PlaybackStatus="Playing"', 'PlaybackStatus="Paused" if self.paused else "Playing"')
        fixture = fixture.replace('            if cmd == "seekable":', '''            if cmd.startswith("clock:"):
                _, state, seconds = cmd.split(":")
                player.position = int(float(seconds) * 1000000)
                player.paused = state == "paused"
                player.PropertiesChanged('org.mpris.MediaPlayer2.Player', {'PlaybackStatus':'Paused' if player.paused else 'Playing'}, [])
                player.Seeked(dbus.Int64(player.position))
            if cmd == "seekable":''')
        (temp / 'player.py').write_text(fixture)
        (temp / 'qml/shell.qml').write_text('''import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    id:root
    property string requestedSpotifyId:''
    signal lyricsReceived(string token,var result)
    Native {id:native}
    readonly property var row:native.items({provider:'playerctl',players:['bartjikFixture']})[0] || null
    function mediaPlayer(id){return native.players.find(p=>p.dbusName===id) || null;}
    function send(msg){if(msg.op==='lyrics')requestedSpotifyId=msg.track.spotifyId || '';if(msg.op==='lyrics')lyricsReceived(msg.token,{status:'ready',text:'Synthetic words',lines:Array.from({length:30},(_,i)=>({time:i*2+2,text:'Synthetic line '+i}))});}
    PanelWindow {
        implicitWidth:420;implicitHeight:300;color:'#26233a'
        Lyrics {
            id:lyrics;anchors.fill:parent
            controller:root;theme:({font:'monospace',text:'#eeeeee',muted:'#aaaaaa',accent:'#ff53a6'})
            playerId:root.row?.id || '';track:root.row?.media || {};active:!!root.row
        }
    }
    IpcHandler {
        target:'lyricsNative'
        function state():string {return JSON.stringify({spotifyId:requestedSpotifyId,rawTrack:root.row?.media?.trackId || '',parsed:root.row?.media?.spotifyId || '',active:lyrics.active,synced:lyrics.synced,index:lyrics.currentLine,position:lyrics.position,playing:lyrics.player?.isPlaying || false});}
    }
}''')
        control = temp / 'control'

        def state():
            result = subprocess.run(['quickshell','ipc','-p',str(temp / 'qml'),'call','lyricsNative','state'],capture_output=True,text=True,timeout=3)
            if result.returncode:
                raise ValueError('not ready')
            return json.loads(result.stdout)

        def clock(mode, seconds):
            control.write_text('clock:' + mode + ':' + str(seconds))

        log_path = destination / 'lyrics-native.log'
        with log_path.open('w') as log:
            shell = subprocess.Popen(['quickshell','-p',str(temp / 'qml')],stdout=log,stderr=log)
            player = None
            try:
                wait_for(lambda: not state()['active'])
                player = subprocess.Popen(['/usr/bin/python3',str(temp / 'player.py'),str(temp / 'events.json'),str(control)],stdout=log,stderr=log)
                wait_for(lambda: state()['synced'])
                assert state()['spotifyId'] == '1234567890abcdefghijkl', state()
                clock('playing', 3.5)
                wait_for(lambda: state()['index'] == 0)
                wait_for(lambda: state()['index'] == 1)
                clock('paused', 8.5)
                wait_for(lambda: not state()['playing'] and state()['index'] == 3)
                paused = state()['position']
                time.sleep(.5)
                assert abs(state()['position'] - paused) < .05, state()
                clock('paused', 22.5)
                wait_for(lambda: state()['index'] == 10)
                clock('paused', 2.5)
                wait_for(lambda: state()['index'] == 0)
                clock('playing', 3.5)
                wait_for(lambda: state()['playing'] and state()['index'] == 1)
                player.terminate();player.wait(timeout=5);player=None
                wait_for(lambda: not state()['active'] and not state()['synced'])
            finally:
                if player:
                    player.terminate();player.wait(timeout=5)
                shell.terminate();shell.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines() if any(s in line for s in ('WARN scene','ERROR','TypeError','ReferenceError','Binding loop'))]
        assert not errors, errors
    print('PASS: native MPRIS Spotify ID propagation, elapsed position, pause/resume, seeks in both directions, selected-player lookup and disconnect; no lyric network requests.')


if __name__ == '__main__':
    main()
