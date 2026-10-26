"""Real QML/helper with synthetic MPRIS and a navigation-only app command."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from render import wait_for


def main():
    if not os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/'):
        raise RuntimeError('Private bus required')
    with tempfile.TemporaryDirectory(prefix='bartjik-navigation-') as td:
        temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
        shutil.copytree(ROOT/'bartjik_core',temp/'bartjik_core')
        events=temp/'events.json'
        # A synthetic CLI stands in for Spotify; the helper must navigate with
        # --uri and never call the fixture's playback-oriented OpenUri method.
        navigations = temp/'navigations.json'
        app = temp/'spotify_fixture.py'
        app.write_text('import json,sys\nfrom pathlib import Path\np=Path('+repr(str(navigations))+')\n'
                       'rows=json.loads(p.read_text()) if p.exists() else []\n'
                       'rows.append(sys.argv[1:]);p.write_text(json.dumps(rows))\n')
        helper = temp/'qml/media_navigate.py'
        code = helper.read_text()
        start = code.index('def navigation_command(')
        end = code.index('def navigate(', start)
        code = code[:start] + 'def navigation_command(uri, owner_pid):\n    return ["/usr/bin/python3", '+repr(str(app))+', "--uri="+uri]\n\n' + code[end:]
        helper.write_text(code)
        # Production QML, helper and D-Bus run; compositor focus stays synthetic.
        (temp/'qml/tray_focus.py').write_text('def main(app):\n    assert app == "spotify"\n')
        (temp/'qml/shell.qml').write_text('''import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    Native {id:native}
    IpcHandler {
        target:'navigation'
        function state():string {return JSON.stringify({players:native.players.length,error:native.navigationError,playing:native.players[0]?.isPlaying});}
        function click(action:string):void {native.act(action,native.players[0].dbusName,null,{});}
    }
}''')
        fixture=(ROOT/'tests/native_fixture.py').read_text()
        fixture=fixture.replace('org.mpris.MediaPlayer2.bartjikFixture','org.mpris.MediaPlayer2.spotify')
        fixture=fixture.replace('dbus.ObjectPath("/track/1")','dbus.ObjectPath("/com/spotify/track/aaaaaaaaaaaaaaaaaaaaaa")')
        fixture=fixture.replace('(Path(__file__).resolve().parents[1] / "qml/icons/demo-album.png").as_uri()',repr((ROOT/'qml/icons/demo-album.png').as_uri()))
        fixture=fixture.replace('"xesam:album": "Synthetic album",',
            '"spotify:artistUrl": "spotify:artist:cccccccccccccccccccccc",\n'
            '                    "spotify:contextUrl": "spotify:playlist:dddddddddddddddddddddd",\n'
            '                    "xesam:album": "Synthetic album",')
        fixture=fixture.replace('        self.shuffle = False\n\n    def props(self, interface):', '        self.shuffle = False\n\n    @dbus.service.method("org.mpris.MediaPlayer2.Player", in_signature="s")\n    def OpenUri(self, uri):\n        record("player:open:" + str(uri))\n\n    @dbus.service.method("org.mpris.MediaPlayer2")\n    def Raise(self):\n        record("player:raise")\n\n    def props(self, interface):')
        fixture=fixture.replace('PlaybackStatus="Playing",', 'PlaybackStatus=getattr(self,"playback","Playing"),')
        for method,state in (('Pause','Paused'),('Play','Playing')):
            fixture=fixture.replace('    def '+method+'(self):\n        record("player:toggle")',
                '    def '+method+'(self):\n        self.playback='+repr(state)+'\n'
                '        self.PropertiesChanged("org.mpris.MediaPlayer2.Player", {"PlaybackStatus":self.playback}, [])\n'
                '        record("player:toggle")')
        (temp/'fixture.py').write_text(fixture)
        def call(method,*args):
            result=subprocess.run(['quickshell','ipc','--path',str(temp/'qml'),'call','navigation',method,*args],capture_output=True,text=True,timeout=3)
            if result.returncode:raise ValueError('Fixture unavailable')
            return result.stdout
        with (temp/'render.log').open('w') as log:
            shell=subprocess.Popen(['quickshell','--path',str(temp/'qml')],stdout=log,stderr=log)
            service=subprocess.Popen(['/usr/bin/python3','-B',str(temp/'fixture.py'),str(temp/'events.json'),str(temp/'control')],stdout=log,stderr=log)
            try:
                wait_for(lambda:json.loads(call('state'))['players']==1)
                call('click','player.artist');call('click','player.playlist')
                wait_for(lambda:events.exists() and json.loads(events.read_text()).count('player:raise')==2)
                actions=json.loads(events.read_text())
                assert actions==['player:raise','player:raise'],actions
                assert json.loads(navigations.read_text())==[
                    ['--uri=spotify:artist:'+'c'*22], ['--uri=spotify:playlist:'+'d'*22]]
                assert not json.loads(call('state'))['error']
                for count,playing in ((1,False),(2,True)):
                    call('click','player.toggle')
                    wait_for(lambda:json.loads(events.read_text()).count('player:toggle')==count)
                    wait_for(lambda:json.loads(call('state'))['playing'] is playing)
            finally:
                service.terminate();shell.terminate();service.wait(timeout=5);shell.wait(timeout=5)
        errors=[line for line in (temp/'render.log').read_text().splitlines() if any(k in line for k in ('ERROR','TypeError','ReferenceError','Binding loop'))]
        assert not errors,errors
    print('Artist/playlist QML actions use --uri page navigation; MPRIS OpenUri never called; playback separate')


if __name__=='__main__':main()
