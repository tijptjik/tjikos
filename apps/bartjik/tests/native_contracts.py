"""Exercise the Rust executable with isolated synthetic configuration and peers."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/helpers"))
import json, os, select, subprocess, tempfile, time, unittest, sys
from pathlib import Path
APP=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(APP))
from bartjik_core import config as reference
BIN=APP/'target/release/bartjik-core'
class NativeContracts(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='bartjik-native-')
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        self.path=self.root/'config.json'
        self.env={**os.environ,'BARTJIK_HOME':str(APP),'BARTJIK_CONFIG':str(self.path),'XDG_CONFIG_HOME':str(self.root/'config'),'XDG_CACHE_HOME':str(self.root/'cache'),'XDG_STATE_HOME':str(self.root/'state'),'XDG_RUNTIME_DIR':str(self.root/'runtime')}
        self.env.pop('HYPRLAND_INSTANCE_SIGNATURE',None)
        (self.root/'runtime').mkdir()
    def call(self,*args):
        return subprocess.run([str(BIN),*args],env=self.env,capture_output=True,text=True,timeout=10)
    def worker(self,demo=False):
        p=subprocess.Popen([str(BIN),'serve',*(['--demo'] if demo else [])],env=self.env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
        def close():
            if p.poll() is None:
                p.stdin.close();p.wait(timeout=8)
            p.stdout.close();p.stderr.close()
        self.addCleanup(close)
        p.stdin.write(json.dumps({'op':'screens','screens':[{'name':'SYNTHETIC','width':1920,'height':1080,'scale':1}]})+'\n');p.stdin.flush()
        return p
    def receive(self,p,predicate,timeout=8):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            if select.select([p.stdout],[],[],max(0,end-time.monotonic()))[0]:
                line=p.stdout.readline()
                self.assertTrue(line, 'worker exited')
                value=json.loads(line)
                if predicate(value):return value
        self.fail('worker response timed out')
    def test_effective_presets_match_reference_without_rewriting(self):
        for preset in APP.glob('examples/*.json'):
            raw=preset.read_bytes();self.path.write_bytes(raw)
            result=self.call('config','--validate')
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(json.loads(result.stdout)['config'],reference.load(self.path),preset.name)
            self.assertEqual(self.path.read_bytes(),raw)
    def test_invalid_config_rejected(self):
        for cfg in [{'bars':'bad'},{'modules':{'x':{'provider':'process','command':'echo bad'}}},{'style':{'fontSize':True}},{'modules':{'x':{'provider':'datetime','timezone':'not/a/timezone'}}}]:
            self.path.write_text(json.dumps(cfg));self.assertNotEqual(self.call('config','--validate').returncode,0)
    def test_crypto_retry_wakes_only_selected_provider(self):
        self.env.pop('COINMARKETCAP_API_KEY', None)
        self.path.write_text(json.dumps({
            'modules': {
                'prices': {'provider': 'crypto', 'interval': 600},
                'other': {'provider': 'process', 'command': ['printf', '{"text":"Ready"}'], 'interval': 600},
            },
            'bars': [{'id': 'main', 'islands': [{'id': 'one', 'modules': ['prices', 'other']}]}],
        }))
        p = self.worker()
        state = self.receive(p, lambda v: len(v.get('providers', {})) == 2 and
                             all(x.get('polls') == 1 for x in v['providers'].values()))
        bar = state['bars'][0]
        source = bar['sources']['prices']
        other = bar['sources']['other']
        self.assertEqual(state['providers'][source]['error'], 'COINMARKETCAP_API_KEY is not set')
        context = {'barKey': bar['key'], 'monitor': bar['screen'], 'module': 'prices', 'item': 'unavailable'}
        for count in (2, 3):
            p.stdin.write(json.dumps({'op': 'cryptoRetry', 'context': context}) + '\n')
            p.stdin.flush()
            state = self.receive(p, lambda v: v.get('providers', {}).get(source, {}).get('polls') == count, timeout=2)
            self.assertEqual(state['providers'][source]['state'], 'unavailable')
            self.assertEqual(state['providers'][other]['polls'], 1)
        context['module'] = 'other'
        p.stdin.write(json.dumps({'op': 'cryptoRetry', 'context': context}) + '\n')
        p.stdin.flush()
        state = self.receive(p, lambda v: v.get('error') == 'Invalid retry context')
        self.assertEqual(state['providers'][other]['polls'], 1)
    def test_deduplicates_and_keeps_valid_config_on_reload_failure(self):
        cfg={'modules':{'a':{'provider':'process','command':['printf','[{"id":"value","type":"text","text":{"value":"Ready"},"actions":{"leftClick":["bad"]}}]'],'interval':30},'b':{'provider':'process','command':['printf','[{"id":"value","type":"text","text":{"value":"Ready"},"actions":{"leftClick":["bad"]}}]'],'interval':30,'style':{'color':'red'}}},'bars':[{'id':'main','islands':[{'id':'one','modules':['a','b']}]}]}
        self.path.write_text(json.dumps(cfg));p=self.worker()
        state=self.receive(p,lambda v:v.get('ready') and any(x.get('state')=='ready' for x in v.get('providers',{}).values()))
        self.assertEqual(len(state['providers']),1)
        row=next(iter(state['providers'].values()))['items'][0]
        self.assertNotIn('actions',row)
        self.path.write_text('{ broken')
        state=self.receive(p,lambda v:v.get('error'))
        self.assertTrue(state['ready']);self.assertEqual(len(state['bars']),1)
        self.path.write_text(json.dumps(cfg))
        self.receive(p,lambda v:v.get('ready') and not v.get('error'))
    def test_slow_provider_does_not_block_ready_provider(self):
        cfg={'modules':{'slow':{'provider':'process','command':['sleep','3'],'timeout':4},'fast':{'provider':'process','command':['printf','{"text":"Fast"}']}},'bars':[{'id':'main','islands':[{'id':'one','modules':['slow','fast']}]}]}
        self.path.write_text(json.dumps(cfg));started=time.monotonic();p=self.worker()
        self.receive(p,lambda v:any(x.get('state')=='ready' for x in v.get('providers',{}).values()),timeout=2)
        self.assertLess(time.monotonic()-started,2)
    def test_stream_dies_with_worker_and_rejects_actions(self):
        script=self.root/'stream.sh';pid=self.root/'child.pid'
        script.write_text('#!/bin/sh\necho $$ > '+str(pid)+'\nprintf \'{"text":"Streaming"}\\n\'\nsleep 60\n');script.chmod(0o755)
        self.path.write_text(json.dumps({'modules':{'s':{'provider':'process','mode':'stream','command':[str(script)],'timeout':60}},'bars':[{'id':'s','islands':[{'id':'s','modules':['s']}]}]}))
        p=self.worker();self.receive(p,lambda v:any(x.get('state')=='ready' for x in v.get('providers',{}).values()))
        child=int(pid.read_text());p.stdin.close();p.wait(timeout=5)
        end=time.monotonic()+2
        while Path(f'/proc/{child}').exists() and time.monotonic()<end:time.sleep(.05)
        self.assertFalse(Path(f'/proc/{child}').exists())
    def test_demo_never_runs_configured_commands(self):
        marker=self.root/'executed';self.path.write_text(json.dumps({'modules':{'s':{'provider':'process','command':['touch',str(marker)]}},'bars':[{'id':'s','islands':[{'id':'s','modules':['s']}]}]}))
        p=self.worker(True);self.receive(p,lambda v:v.get('ready'));self.assertFalse(marker.exists())
    def test_display_controls_follow_surviving_output_and_return_to_primary(self):
        self.path.write_text(json.dumps({'modules':{'display':{'provider':'monitor-layout'},'clock':{'provider':'datetime'}},'bars':[
            {'id':'controls','monitors':['DP-1'],'islands':[{'id':'controls','modules':['display']}]},
            {'id':'clock','monitors':['DP-1'],'islands':[{'id':'clock','modules':['clock']}]}]}))
        p=self.worker(True)
        state=self.receive(p,lambda v:v.get('ready') and len(v.get('bars',[]))==1)
        self.assertEqual([(b['id'],b['screen']) for b in state['bars']],[('controls','SYNTHETIC')])
        cfg=json.loads(self.path.read_text());cfg['bars'][0]['enabled']=False;self.path.write_text(json.dumps(cfg))
        self.receive(p,lambda v:v.get('ready') and not v.get('bars'))
        cfg['bars'][0]['enabled']=True;self.path.write_text(json.dumps(cfg))
        self.receive(p,lambda v:len(v.get('bars',[]))==1)
        p.stdin.write(json.dumps({'op':'screens','screens':[{'name':'SYNTHETIC','width':1920,'height':1080,'scale':1},{'name':'DP-1','width':3440,'height':1440,'scale':1}]})+'\n');p.stdin.flush()
        state=self.receive(p,lambda v:len(v.get('bars',[]))==2)
        self.assertTrue(all(b['screen']=='DP-1' for b in state['bars']))
        self.assertEqual({b['id'] for b in state['bars']},{'controls','clock'})
    def test_init_refuses_overwrite(self):
        self.assertEqual(self.call('init').returncode,0)
        before=self.path.read_bytes();self.assertNotEqual(self.call('init').returncode,0);self.assertEqual(before,self.path.read_bytes())
if __name__=='__main__':unittest.main()
