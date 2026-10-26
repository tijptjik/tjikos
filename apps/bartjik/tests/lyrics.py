"""Real QML clicks, scrolling and asynchronous lyric replies on a private bus."""
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
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-lyrics-render-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        path = temp / 'qml/Lyrics.qml'
        text = path.read_text().replace('    id:lyrics', '''    id:lyrics
    function syncTest() {
        const line=verses.itemAt(currentLine);
        return {timer:playback.running,following:!resume.running,
            highlight:line?.highlighted || false,
            center:line?timed.y+line.y+line.height/2-scroll.contentY-scroll.height/2:0};
    }''')
        path.write_text(text)
        path = temp / 'qml/MediaDetails.qml'
        text = path.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        text = text.replace('    id:details', '''    id:details
    TestEvent {id:input}
    function lyricsTest(req) {
        if(req.click)input.mouseClick(lyricsButton,16,16,Qt.LeftButton,Qt.NoModifier,0);
        if(req.wheel)input.mouseWheel(lyrics,150,80,Qt.NoButton,Qt.NoModifier,0,-240,0);
        return {shown:lyricsShown,status:lyrics.status,text:lyrics.text,cover:cover.visible,
            current:lyrics.currentLine,synced:lyrics.synced,position:lyrics.position,
            timer:lyrics.syncTest().timer,following:lyrics.syncTest().following,
            highlighted:lyrics.syncTest().highlight,center:lyrics.syncTest().center,
            scroll:lyrics.children[0].contentY,contentHeight:lyrics.children[0].contentHeight,
            buttonX:lyricsButton.x,token:lyrics.token};
    }''')
        path.write_text(text)
        path = temp / 'qml/DetailPanel.qml'
        path.write_text(path.read_text().replace('    id:popup', '    id:popup\n    function lyricsTest(req){return mediaDetails.lyricsTest(req);}'))
        path = temp / 'qml/shell.qml'
        text = path.read_text().replace('    id: root', '''    id: root
    property bool deferLyrics:false
    property var lyricRequests:[]
    property string testTitle:''
    property bool hidePlayer:false
    QtObject {
        id:syntheticPlayer
        property real position:0
        property bool positionSupported:true
        property bool isPlaying:false
    }
    Timer {interval:100;repeat:true;running:syntheticPlayer.isPlaying;onTriggered:syntheticPlayer.position+=.1}''')
        text = text.replace('function send(message) {', "function send(message) {if(message.op==='lyrics'){lyricRequests=lyricRequests.concat([message]);if(deferLyrics)return;}")
        text = text.replace("return (state.providers[bar.sources[id]] || {}).items || [];", """const rows=(state.providers[bar.sources[id]] || {}).items || [];
        if(m.provider==='playerctl')return hidePlayer?[]:rows.map(r=>Object.assign({},r,{media:Object.assign({},r.media,testTitle?{title:testTitle}:{})}));
        return rows;""")
        text = text.replace("case 'status':break;", """case 'lyricsTest':
            if(req.defer!==undefined)deferLyrics=req.defer;
            if(req.title!==undefined)testTitle=req.title;
            if(req.hide!==undefined)hidePlayer=req.hide;
            if(req.reply)lyricsReceived(req.token,req.reply);
            return Object.assign({ok:true,requests:lyricRequests},detailPanel.lyricsTest(req));
        case 'status':break;""")
        text = text.replace("function mediaPlayer(id) {return native.players.find(p=>p.dbusName===id) || null;}", "function mediaPlayer(id) {return syntheticPlayer;}")
        text = text.replace("if(req.defer!==undefined)deferLyrics=req.defer;", "if(req.position!==undefined)syntheticPlayer.position=req.position;if(req.playing!==undefined)syntheticPlayer.isPlaying=req.playing;if(req.supported!==undefined)syntheticPlayer.positionSupported=req.supported;\n            if(req.defer!==undefined)deferLyrics=req.defer;")
        path.write_text(text)
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        for bar in cfg['bars']:
            bar['autoHide'] = False
        path = temp / 'config.json'
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_CONFIG=str(path), BARTJIK_QML=str(temp / 'qml'), XDG_CACHE_HOME=str(temp / 'cache'))

        def state(**req):
            return ipc(dict(op='lyricsTest', **req))

        def capture(name):
            path = destination / (name + '.png')
            ipc(dict(op='captureDetail', path=str(path)))
            wait_for(lambda: path.exists() and path.read_bytes().endswith(b'IEND\xaeB`\x82'))

        with (destination / 'lyrics.log').open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc(dict(op='status')).get('ready'))
                ipc(dict(op='detailTest', module='music'))
                wait_for(lambda: ipc(dict(op='detailState'))['panel']['opened'])
                assert not state()['shown'] and not state()['requests']
                assert state()['buttonX'] == 12
                state(click=True)
                wait_for(lambda: state()['status'] == 'ready')
                assert state()['shown'] and not state()['cover']
                assert 'Evening light' in state()['text']
                capture('lyrics-panel')
                state(click=True)
                assert not state()['shown'] and state()['cover']
                # Replies from the previous track or a closed view must be ignored.
                state(defer=True, click=True)
                wait_for(lambda: len(state()['requests']) == 2)
                old = state()['token']
                state(title='Second song')
                wait_for(lambda: len(state()['requests']) == 3)
                assert state()['status'] == 'loading' and not state()['text']
                state(token=old, reply=dict(status='ready', text='Obsolete lyrics'))
                assert state()['status'] == 'loading' and not state()['text']
                current = state()['token']
                long_text = '\n'.join('Synthetic lyric line ' + str(i) for i in range(40))
                state(token=current, reply=dict(status='ready', text=long_text, source='lyrics.ovh'))
                wait_for(lambda: state()['contentHeight'] > 300)
                state(wheel=True)
                wait_for(lambda: state()['scroll'] > 0)
                # Timed lyrics follow playback/seek position and stop advancing on pause.
                timed = [dict(time=i * 2 + 2, text='Synthetic timed line ' + str(i)) for i in range(20)]
                state(position=0, token=current, reply=dict(status='ready', text=long_text, lines=timed))
                wait_for(lambda: state()['synced'])
                assert state()['current'] == -1
                state(position=22.1)
                wait_for(lambda: state()['current'] == 10)
                # The preceding manual wheel scroll pauses following for four seconds.
                wait_for(lambda: state()['following'], timeout=6)
                wait_for(lambda: state()['scroll'] > 150 and abs(state()['center']) < 1)
                assert state()['highlighted']
                capture('synced-highlight')
                state(position=3)
                wait_for(lambda: state()['current'] == 0)
                state(position=3.8, playing=True)
                wait_for(lambda: state()['current'] == 1)
                assert state()['timer']
                state(playing=False)
                assert not state()['timer']
                paused = state()['position']
                time.sleep(.4)
                assert abs(state()['position'] - paused) < .01
                assert state()['current'] == 1
                state(position=18.1)
                wait_for(lambda: state()['current'] == 8)
                state(wheel=True)
                assert not state()['following']
                state(supported=False)
                assert not state()['synced'] and state()['current'] == -1
                state(supported=True)
                wait_for(lambda: state()['current'] == 8)
                requests = len(state()['requests'])
                state(position=1)
                assert len(state()['requests']) == requests
                state(title='Third song')
                wait_for(lambda: len(state()['requests']) == 4)
                assert state()['scroll'] == 0 and not state()['text'] and state()['current'] == -1
                for status in ('missing', 'instrumental', 'error'):
                    state(token=state()['token'], reply=dict(status=status, text=''))
                    assert state()['status'] == status
                    capture(status)
                state(click=True)
                state(token=state()['token'], reply=dict(status='ready', text='Hidden response'))
                assert not state()['shown'] and not state()['text']
                # Reopening retains the selected view and requests current metadata.
                state(click=True)
                ipc(dict(op='detailTest', module='music', leave=True))
                wait_for(lambda: not ipc(dict(op='detailState'))['panel']['opened'])
                requests = len(state()['requests'])
                time.sleep(.3)
                assert len(state()['requests']) == requests
                ipc(dict(op='detailTest', module='music'))
                wait_for(lambda: len(state()['requests']) == requests + 1)
                assert state()['shown']
                state(hide=True)
                wait_for(lambda: not ipc(dict(op='detailState'))['panel']['opened'])
            finally:
                process.terminate()
                process.wait(timeout=10)
        errors = [line for line in (destination / 'lyrics.log').read_text().splitlines()
                  if any(s in line for s in ('WARN scene', 'ERROR', 'ReferenceError', 'TypeError', 'Binding loop'))]
        assert not errors, errors
    print('PASS: lyrics toggle, source-free view, scrolling, timed highlighting, forward/backward seeks, pause/resume, missing position, track changes, stale replies and disappearance.')


if __name__ == '__main__':
    main()
