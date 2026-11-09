"""Synthetic native GTK intents and backend recovery in the authorized stacked layout.

Requires a prepared comparison config and no active comparison. All providers/actions
use --demo, a private bus, and the fake compositor. No host input is injected.
"""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import threading
import time

APP = Path(__file__).resolve().parents[1]
ROOT = Path(os.environ['XDG_RUNTIME_DIR']) / 'bartjik-gtk-comparison'

def request(path, value):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(3)
        stream.connect(str(path))
        stream.sendall((json.dumps(value)+'\n').encode())
        return json.loads(stream.makefile().readline())

with tempfile.TemporaryDirectory(prefix='bartjik-native-ui-') as tmp:
    root=Path(tmp)
    activity=['idle']
    voice=socket.socket(socket.AF_UNIX)
    voice.bind(str(root/'voice.sock'))
    voice.listen()
    def voice_server():
        while True:
            try:connection,_=voice.accept()
            except OSError:return
            with connection:
                connection.recv(2)
                connection.sendall(('STATUS status='+activity[0]+'\n').encode())
    threading.Thread(target=voice_server,daemon=True).start()
    config=json.loads((ROOT/'config.json').read_text())
    config['modules']['music']['track']={'actions':{'rightClick':{'command':['false']},'doubleClick':'player.shuffle'}}
    config['modules']['music']['items']={'demo-player':{'track':{'actions':{'middleClick':None}}}}
    (root/'config.json').write_text(json.dumps(config))
    env={**os.environ,'BARTJIK_HOME':str(APP/'dist'),'BARTJIK_CONFIG':str(root/'config.json'),
         'BARTJIK_GTK_SOCKET':str(root/'gtk.sock'),'BARTJIK_GTK_ACTIVITY_FIXTURE':str(root/'voice.sock'),'GTK_A11Y':'none','GTK_USE_PORTAL':'0','GIO_USE_VFS':'local',
         'XDG_CACHE_HOME':str(root/'cache'),'XDG_STATE_HOME':str(root/'state')}
    env.pop('BARTJIK_GTK_COMPARE_FILE',None)
    env.pop('HYPRLAND_INSTANCE_SIGNATURE',None)
    with (root/'gtk.log').open('w') as log:
        process=subprocess.Popen(['dbus-run-session','--',str(APP/'dist/bin/bartjik-gtk'),'serve','--demo'],env=env,stdout=log,stderr=log,start_new_session=True)
        def until(predicate,timeout=10):
            end=time.monotonic()+timeout
            while time.monotonic()<end:
                try:
                    state=request(root/'gtk.sock',{'action':'status'})
                    if predicate(state):return state
                except (OSError,ValueError):pass
                assert process.poll() is None,(root/'gtk.log').read_text()
                time.sleep(.025)
            raise AssertionError({'ready':state.get('backendReady'),'error':state.get('error'),'log':(root/'gtk.log').read_text()[-1200:]})
        def texts(state,module):
            return [text for bar in state['bars'] for view in bar['motion'] if view['id']==module for v in view['visuals'] for text in v['texts']]
        def action(module,item,part,event):
            state=until(lambda s:s.get('backendReady'))
            bar=next(b for b in state['bars'] if any(v['id']==module for v in b['motion']))
            return request(root/'gtk.sock',{'action':'input','context':{'barKey':bar['key'],'module':module,'item':item,'part':part,'event':event}})
        try:
            state=until(lambda s:s.get('backendReady') and '21:34' in texts(s,'time'))
            assert state['demo'] and state['nativeRuntime'] and not state['reserve']
            action('time','demo','text','leftClick')
            until(lambda s:'Mon 15 Jun' in texts(s,'time'))
            action('time','demo','text','leftClick')
            until(lambda s:'21:34' in texts(s,'time') and 'Mon 15 Jun' not in texts(s,'time'))
            def recording(state):
                return any(icon['state']['recording'] for bar in state['bars'] for view in bar['motion'] for icon in view['icons'])
            started=time.monotonic()
            activity[0]='recording'
            until(recording, timeout=1)
            latency=(time.monotonic()-started)*1000
            assert latency < 400, latency
            activity[0]='idle'
            until(lambda s:not recording(s))
            print(f'PASS direct synthetic voice state to GTK: {latency:.1f}ms (includes 150ms helper cadence)')
            for event in ['scrollDown','scrollUp','doubleClick','rightClick']:
                action('music','demo-player','track',event)
                until(lambda s:any(i['context']['event']==event for i in s['intents']))
            count=len(until(lambda s:True)['intents'])
            action('music','demo-player','track','middleClick')
            time.sleep(.15)
            assert len(until(lambda s:True)['intents'])==count,'Null override dispatched an action'
            gtk_pid=state['pid']
            children={pid for path in Path(f'/proc/{gtk_pid}/task').glob('*/children') for pid in path.read_text().split()}
            core=next(int(pid) for pid in children if Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')[1:2] == [b'serve'])
            os.kill(core,signal.SIGKILL)
            until(lambda s:s.get('backendReady') is False)
            until(lambda s:s.get('backendReady') is True)
            assert '21:34' in texts(until(lambda s:True),'time')
            action('music','demo-player','track','scrollDown')
            until(lambda s:len(s['intents'])==count+1)
            print('PASS native GTK time/date roundtrip, media/configured/null intents, fake compositor and owned-core restart')
        finally:
            try:request(root/'gtk.sock',{'action':'quit'})
            except OSError:pass
            try:process.wait(timeout=5)
            except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGTERM);process.wait(timeout=5)
