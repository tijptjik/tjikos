"""Native display transactions with isolated Hyprland and recovery-process fixtures."""
import copy
import json
import os
from pathlib import Path
import signal
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import unittest

APP=Path(__file__).resolve().parents[1]
BIN=APP/'target/release/bartjik-core'


class MonitorTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='bartjik-display-test-')
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        for path in ('bin','hypr/test','config/hypr'):(self.root/path).mkdir(parents=True)
        self.env={**os.environ,'XDG_RUNTIME_DIR':str(self.root),'XDG_CONFIG_HOME':str(self.root/'config'),'XDG_CACHE_HOME':str(self.root/'cache'),'XDG_STATE_HOME':str(self.root/'state'),'HYPRLAND_INSTANCE_SIGNATURE':'test','PATH':str(self.root/'bin')+os.pathsep+os.environ['PATH']}
        self.live=self.root/'live.json';self.events=self.root/'events.jsonl';self.pending=self.root/'bartjik-monitors-pending.json'
        self.saved=self.root/'config/bartjik/monitors.lua'
        self.original=[dict(name='DP-1',model='Test primary',width=1920,height=1080,refreshRate=60.,x=0,y=0,scale=1,transform=0,disabled=False,availableModes=['1920x1080@60.00Hz','1920x1080@120.00Hz'],hardwareDetails=dict(hdr=True,vrrCapable=True),colorManagementPreset='srgb'),
                       dict(name='HDMI-A-1',model='Test TV',width=0,height=0,refreshRate=60.,x=-1,y=-1,scale=1,transform=0,disabled=True,availableModes=['3840x2160@60.00Hz'],hardwareDetails=dict(hdr=True,vrrCapable=True))]
        for index,row in enumerate(self.original):row.update(id=index,mirrorOf='none')
        self.original.append(dict(self.original[1],id=2,name='DP-2',model='Test copy',availableModes=['1920x1080@60.00Hz']))
        self.live.write_text(json.dumps(self.original))
        (self.root/'config/hypr/monitors.lua').write_text('hl.monitor({\noutput = "DP-1",\nmode = "1920x1080@60",\nposition = "0x0",\nscale = 1,\n})\nhl.monitor({\noutput = "HDMI-A-1",\nmode = "3840x2160@60",\nposition = "-3840x-360",\nscale = 1,\ndisabled = true,\n})\n')
        with (self.root/'config/hypr/monitors.lua').open('a') as config:
            config.write('hl.monitor({\noutput = "DP-2",\nmode = "1920x1080@60",\nposition = "1920x0",\nscale = 1,\ndisabled = true,\n})\n')
        root=self.root
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                assert self.request.recv(8192)==b'j/monitors all'
                self.request.sendall((root/'live.json').read_bytes())
        server=socketserver.UnixStreamServer(str(self.root/'hypr/test/.socket.sock'),Handler)
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        self.addCleanup(lambda:(server.shutdown(),server.server_close(),thread.join(timeout=2)))
        self.script('hyprctl',r'''
import json, os, re, sys
from pathlib import Path
root=Path(os.environ['XDG_RUNTIME_DIR'])
rows=json.loads((root/'live.json').read_text())
with (root/'events.jsonl').open('a') as output:output.write(json.dumps(sys.argv[1:])+'\n')
assert sys.argv[1]=='eval'
for body in re.findall(r'hl\.monitor\(\{([^}]+)\}\)',sys.argv[2]):
    fields={key:json.loads(value) for key,value in re.findall(r'(\w+)=("[^"]*"|true|false|-?\d+(?:\.\d+)?)',body)}
    row=next(row for row in rows if row['name']==fields['output'])
    for key,value in fields.items():
        if key in ('scale','transform','disabled'):row[key]=value
        elif key=='position':row['x'],row['y']=map(int,value.split('x'))
        elif key=='mode' and not (root/'reject-mode').exists():
            dimensions,refresh=value.split('@');row['width'],row['height']=map(int,dimensions.split('x'));row['refreshRate']=float(refresh)
        elif key=='cm':row['colorManagementPreset']=value if value!='auto' else 'srgb'
        elif key=='mirror':row['mirrorOf']=str(next(m['id'] for m in rows if m['name']==value)) if value else 'none'
for row in rows:
    source=next((m for m in rows if str(m['id'])==row['mirrorOf'] and not m['disabled']),None)
    if source and not row['disabled']:row['x'],row['y']=source['x'],source['y']
    elif not source:row['mirrorOf']='none'
(root/'live.next').write_text(json.dumps(rows));(root/'live.next').replace(root/'live.json')
print('ok')
''')
        self.script('systemd-run',r'''
import os, subprocess, sys
from pathlib import Path
root=Path(os.environ['XDG_RUNTIME_DIR'])
if (root/'reject-guard').exists():sys.exit(1)
args=sys.argv[sys.argv.index('--')+1:]
with (root/'guard.log').open('ab') as log:
    child=subprocess.Popen(args,env=os.environ,stdout=log,stderr=log,start_new_session=True)
(root/('guard-'+str(child.pid)+'.pid')).write_text(str(child.pid))
''')
        def stop_guards():
            for marker in self.root.glob('guard-*.pid'):
                try:os.kill(int(marker.read_text()),signal.SIGTERM)
                except ProcessLookupError:pass
        self.addCleanup(stop_guards)

    def script(self,name,source):
        path=self.root/'bin'/name;path.write_text(f'#!{sys.executable}\n'+source);path.chmod(0o755)

    def call(self,*args,input=None,success=True):
        result=subprocess.run([str(BIN),'helper','monitors',*args],input=json.dumps(input) if input else None,env=self.env,capture_output=True,text=True,timeout=18)
        if success:self.assertEqual(result.returncode,0,result.stderr)
        else:self.assertNotEqual(result.returncode,0)
        return json.loads(result.stdout if result.returncode==0 else result.stderr)

    def status(self):return self.call('status')['panel']
    def apply(self,edits,**kwargs):
        return self.call('apply',input=dict(revision=self.status()['revision'],edits=edits),**kwargs)
    def wait(self,predicate):
        deadline=time.monotonic()+7
        while time.monotonic()<deadline:
            if predicate():return
            time.sleep(.05)
        self.fail('Display transaction did not settle: '+(self.root/'guard.log').read_text())
    def expire(self):
        pending=json.loads(self.pending.read_text());pending['deadline']=time.time()-.1
        stage=self.root/'pending.next';stage.write_text(json.dumps(pending));stage.replace(self.pending)

    def test_guard_failure_and_last_screen_protection_do_not_apply(self):
        result=self.apply([dict(name='DP-1',settings=dict(enabled=False))],success=False)
        self.assertIn('at least one',result['error'])
        (self.root/'reject-guard').touch()
        result=self.apply([dict(name='DP-1',settings=dict(x=200))],success=False)
        self.assertIn('no changes were applied',result['error'])
        self.assertFalse(self.events.exists())
        self.assertFalse(self.pending.exists())
        self.assertFalse(self.saved.exists())

    def test_keep_persists_only_edited_fields_and_revert_preserves_saved_settings(self):
        self.apply([dict(name='DP-1',settings=dict(x=100))])
        self.assertFalse(self.saved.exists())
        pending=json.loads(self.pending.read_text())
        self.assertEqual(json.loads(self.live.read_text())[0]['x'],100)
        self.call('decide',pending['token'],'keep')
        self.assertFalse(self.pending.exists())
        data=json.loads(self.saved.read_text().splitlines()[0].removeprefix('-- bartjik-monitors-v1 '))
        self.assertEqual(data,{'DP-1':{'x':100,'y':0}})
        self.assertEqual(self.saved.stat().st_mode & 0o777,0o600)
        saved=self.saved.read_bytes()
        self.apply([dict(name='DP-1',settings=dict(x=400))])
        pending=json.loads(self.pending.read_text())
        self.call('decide',pending['token'],'revert')
        self.assertEqual(json.loads(self.live.read_text())[0]['x'],100)
        self.assertEqual(self.saved.read_bytes(),saved)

    def test_expiry_recovers_after_initiating_process_exits(self):
        self.apply([dict(name='DP-1',settings=dict(scale=2))])
        self.assertEqual(json.loads(self.live.read_text())[0]['scale'],2)
        self.expire()
        self.wait(lambda:not self.pending.exists())
        self.assertEqual(json.loads(self.live.read_text())[0]['scale'],1)
        self.assertFalse(self.saved.exists())

    def test_tv_enable_is_verified_and_failed_mode_rolls_back(self):
        self.call('preset','game')
        pending=json.loads(self.pending.read_text());rows=json.loads(self.live.read_text())
        self.assertFalse(rows[1]['disabled']);self.assertEqual(rows[1]['x'],-3840)
        self.call('decide',pending['token'],'revert')
        self.assertTrue(json.loads(self.live.read_text())[1]['disabled'])
        (self.root/'reject-mode').touch()
        result=self.apply([dict(name='DP-1',settings=dict(mode='1920x1080@120.00'))],success=False)
        self.assertIn('did not accept',result['error'])
        self.assertFalse(self.pending.exists())
        self.assertFalse(self.saved.exists())

    def test_reload_or_hotplug_revision_is_not_silently_overwritten(self):
        revision=self.status()['revision']
        rows=copy.deepcopy(self.original);rows[0]['x']=250;self.live.write_text(json.dumps(rows))
        result=self.call('apply',input=dict(revision=revision,edits=[dict(name='DP-1',settings=dict(x=500))]),success=False)
        self.assertIn('changed while',result['error'])
        self.assertFalse(self.events.exists())

    def test_hot_unplug_during_preview_keeps_surviving_screen_on(self):
        self.call('preset','game')
        rows=json.loads(self.live.read_text())
        self.live.write_text(json.dumps([rows[1]]))
        self.expire()
        self.wait(lambda:not self.pending.exists())
        self.assertFalse(json.loads(self.live.read_text())[0]['disabled'])
        self.assertFalse(self.saved.exists())

    def test_previous_session_preview_does_not_block_new_session(self):
        self.pending.write_text(json.dumps(dict(token='old',instance='gone',deadline=time.time()+100)))
        self.assertIsNone(self.status()['pending'])
        self.apply([dict(name='DP-1',settings=dict(x=200))])
        pending=json.loads(self.pending.read_text())
        self.call('decide',pending['token'],'revert')

    def test_mirror_preview_keep_and_desktop_restore(self):
        self.call('preset','mirror')
        panel=self.status();copy=next(m for m in panel['monitors'] if m['name']=='DP-2')
        self.assertTrue(copy['enabled']);self.assertEqual(copy['settings']['mirror'],'HDMI-A-1')
        self.assertEqual(copy['settings']['x'],1920)
        self.call('decide',panel['pending']['token'],'keep')
        self.assertIn('mirror="HDMI-A-1"',self.saved.read_text())
        self.call('preset','desktop')
        panel=self.status();copy=next(m for m in panel['monitors'] if m['name']=='DP-2')
        self.assertEqual(copy['settings']['mirror'],'')
        self.assertEqual(copy['settings']['x'],1920)
        self.assertTrue(json.loads(self.live.read_text())[1]['disabled'])
        self.call('decide',panel['pending']['token'],'revert')
        copy=next(m for m in self.status()['monitors'] if m['name']=='DP-2')
        self.assertEqual(copy['settings']['mirror'],'HDMI-A-1')

    def test_mirror_timeout_recovers_original_layout(self):
        self.call('preset','mirror');self.expire()
        self.wait(lambda:not self.pending.exists())
        rows=json.loads(self.live.read_text())
        self.assertFalse(rows[0]['disabled']);self.assertTrue(rows[1]['disabled']);self.assertTrue(rows[2]['disabled'])
        self.assertEqual(rows[2]['mirrorOf'],'none')


if __name__=='__main__':unittest.main()
