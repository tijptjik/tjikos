"""Panel framework checks using synthetic pointers/providers in the stacked layout.

No host pointer movement, device actions, nested compositor, or focus changes.
Close the comparison first; the existing prepared comparison config is reused.
"""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time

APP=Path(__file__).resolve().parents[1]
ROOT=Path(os.environ['XDG_RUNTIME_DIR'])/'bartjik-gtk-comparison'
ARTIFACT=ROOT/'panel-framework.png'
with tempfile.TemporaryDirectory(prefix='bartjik-panels-') as tmp:
    root=Path(tmp)
    cfg=json.loads((ROOT/'config.json').read_text())
    for module in cfg['modules'].values():
        module.setdefault('panel',{}).update(hover=True,delay=200,closeDelay=160)
    cfg['modules']['music']['panel']['hover']=False
    (root/'config.json').write_text(json.dumps(cfg))
    env={**os.environ,'BARTJIK_HOME':str(APP/'dist'),'BARTJIK_CONFIG':str(root/'config.json'),'BARTJIK_GTK_SOCKET':str(root/'gtk.sock'),'GTK_A11Y':'none','GIO_USE_VFS':'local','XDG_CACHE_HOME':str(root/'cache'),'XDG_STATE_HOME':str(root/'state')}
    env.pop('BARTJIK_GTK_COMPARE_FILE',None)
    with (root/'gtk.log').open('w') as log:
        p=subprocess.Popen(['dbus-run-session','--',str(APP/'dist/bin/bartjik-gtk'),'serve','--demo'],env=env,stdout=log,stderr=log,start_new_session=True)
        def request(value):
            with socket.socket(socket.AF_UNIX) as stream:
                stream.settimeout(3);stream.connect(str(root/'gtk.sock'));stream.sendall((json.dumps(value)+'\n').encode());return json.loads(stream.makefile().readline())
        def status():return request({'action':'status'})
        def wait(predicate,timeout=5):
            until=time.monotonic()+timeout
            state={}
            while time.monotonic()<until:
                try:
                    state=status()
                    if predicate(state):return state
                except (OSError,ValueError):pass
                assert p.poll() is None,(root/'gtk.log').read_text()[-1500:]
                time.sleep(.02)
            raise AssertionError(({k:v for k,v in state.get('panelDeck',{}).items() if k!='targets'},(root/'gtk.log').read_text()[-1000:]))
        def move(point):return request({'action':'pointer','point':point})
        def center(r):return [r['x']+r['width']/2,r['y']+r['height']/2]
        try:
            state=wait(lambda s:s.get('backendReady') and any(t['module']=='time' for t in s['panelDeck']['targets']))
            time.sleep(.3)
            state=status()
            targets=state['panelDeck']['targets']
            clock=next(t for t in targets if t['module']=='time')
            sound=next(t for t in targets if t['module']=='sound')
            move(center(clock['bounds']))
            assert status()['panelDeck']['active'] is None,'Dwell skipped'
            time.sleep(.08)
            move([clock['screen']['x']+15,clock['screen']['y']+15])
            time.sleep(.25)
            assert status()['panelDeck']['active'] is None,'Cancelled dwell opened a panel'
            move(center(clock['bounds']))
            state=wait(lambda s:s['panelDeck']['active']==clock['key'] and s['panelDeck']['surface']['allocation'] is not None)
            state=wait(lambda s:not s['panelDeck']['surface']['transitioning'])
            surface=state['panelDeck']['surface'];r=surface['rect'];allocated=surface['allocation']
            for k in ['x','y','width','height']:assert abs(r[k]-allocated[k])<=2,(k,r,allocated)
            assert surface['keyboard']=='None'
            follow=center(clock['bounds']);follow[0]+=8
            move(follow)
            wait(lambda s:abs(s['panelDeck']['surface']['target']['x']-r['x']-8)<2)
            state=wait(lambda s:not s['panelDeck']['surface']['transitioning'])
            r=state['panelDeck']['surface']['rect']
            # The eight-pixel travel strip is retention only; no fullscreen input mask.
            corridor=[max(r['x'],clock['source']['x'])+8,(r['y']+r['height']+clock['source']['y'])/2]
            move(corridor);time.sleep(.4)
            assert status()['panelDeck']['active']==clock['key'],'Corridor dismissed panel'
            move(center(r));time.sleep(.35)
            assert status()['panelDeck']['active']==clock['key'],'Panel body dismissed itself'
            # Same-island whitespace resolves to the nearest row; handoff has no new dwell.
            gap=[(sound['bounds']['x']+sound['bounds']['width']+clock['bounds']['x'])/2+2,center(clock['bounds'])[1]]
            move(gap);time.sleep(.05)
            assert status()['panelDeck']['active'] in [clock['key'],sound['key']]
            move(center(sound['bounds']))
            assert status()['panelDeck']['active']==sound['key'],'Same-bar handoff restarted dwell'
            # Outside an island: grace retains the shell, but a sustained exit dismisses.
            outside=[clock['screen']['x']+15,clock['screen']['y']+15]
            move(outside);time.sleep(.1)
            assert status()['panelDeck']['active'] is not None
            wait(lambda s:s['panelDeck']['active'] is None)
            music=next(t for t in targets if t['module']=='music')
            move(center(music['bounds']));time.sleep(.35)
            assert status()['panelDeck']['active'] is None,'Disabled hover acquired a panel'
            move(center(clock['bounds']));state=wait(lambda s:s['panelDeck']['active']==clock['key'])
            request({'action':'panel-command','command':'pin'})
            assert status()['panelDeck']['pinned']
            move(outside);time.sleep(.4)
            assert status()['panelDeck']['active']==clock['key'],'Pinned panel followed hover dismissal'
            request({'action':'panel-command','command':'pin'})
            wait(lambda s:s['panelDeck']['active'] is None)
            move(center(clock['bounds']));state=wait(lambda s:s['panelDeck']['active']==clock['key'])
            wait(lambda s:not s['panelDeck']['surface']['transitioning'])
            # Save just the synthetic panel, without capturing surrounding desktop content.
            r=status()['panelDeck']['surface']['rect']
            subprocess.run(['grim','-g',f"{int(r['x'])},{int(r['y'])} {int(r['width'])}x{int(r['height'])}",str(ARTIFACT)],check=True)
            request({'action':'panel-close'})
            time.sleep(.35)
            assert status()['panelDeck']['active'] is None,'Dismissal reopened beneath stationary pointer'
            move(None)
            wait(lambda s:not s['pointerSampling'])
            assert status()['panelDeck']['surface'] is None
            print('PASS dwell/cancel, card allocation, corridor retention, gap handoff, disabled targets, dismissal and idle sampler teardown')
            print('Synthetic panel artifact:',ARTIFACT)
        finally:
            try:request({'action':'quit'})
            except OSError:pass
            try:p.wait(timeout=5)
            except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGTERM);p.wait(timeout=5)
