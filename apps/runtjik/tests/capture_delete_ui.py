"""Sample derez and FLIP frames in the real renderer, using only --demo fixtures."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'DEREZ'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'DEREZ' for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    subprocess.run(['hyprctl', 'eval', 'hl.monitor({output="DEREZ",mode="1000x680@60",position="-3000x0",scale=1})'], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="DEREZ"}))'], check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    function receive(msg) {', '''
    property var galleryFixture: null
    property var sampledFrames: []
    property bool sampling: false
    property string capturePrefix: ''
    function sampleGallery() {
        return {active:activeGalleryRemovals,message:message,empty:galleryEmpty,pending:providerPending,
            rows:results.map(r=>r.id),
            tiles:Array.from(items.contentItem.children).filter(c=>c.modelData && !c.derezzing && !c.modelData.empty_state).map(c=>({id:c.modelData.id,x:c.mapToItem(items,0,0).x,y:c.mapToItem(items,0,0).y})),
            effects:Array.from(items.contentItem.children).filter(c=>c.objectName==='deleteDerez').map(c=>({progress:c.progress}))};
    }
    FrameAnimation {running:root.sampling;onTriggered:root.sampledFrames.push(root.sampleGallery())}
    Timer {id:derezCapture;interval:180;onTriggered:frame.grabToImage(image=>image.saveToFile(root.capturePrefix+'-derez.png'))}
    Timer {id:flipCapture;interval:480;onTriggered:frame.grabToImage(image=>image.saveToFile(root.capturePrefix+'-flip.png'))}
    function receive(msg) {
        if(msg.kind==='results' && ((msg.query || '').includes('history') || (msg.query || '').startsWith('/cliphist')))galleryFixture=JSON.parse(JSON.stringify(msg));
''').replace('        function testState(): string {', '''
        function animationState():string {return JSON.stringify({current:root.sampleGallery(),frames:root.sampledFrames,query:root.resultQuery});}
        function removeDemo(index:int,prefix:string):void {
            root.capturePrefix=prefix;root.sampledFrames=[];root.sampling=true;
            const msg=JSON.parse(JSON.stringify(root.galleryFixture));
            msg.serial=root.serial;msg.pending=false;msg.removed_entry=msg.results[index].id;
            msg.results.splice(index,1);msg.total=msg.results.length;
            if(!msg.results.length)msg.results=[{id:'empty',provider:'Screenshots',name:'No screenshots',subtitle:'Take a screenshot',empty_state:true,disabled:true}];
            msg.message='Deleted clipboard item';root.receive(msg);
            derezCapture.restart();flipCapture.restart();
        }
        function rejectDemo():void {root.receive({kind:'error',message:'Synthetic deletion rejected'});}
        function singleDemo():void {const msg=JSON.parse(JSON.stringify(root.galleryFixture));msg.serial=root.serial;msg.results=msg.results.slice(0,1);msg.removed_entry='';msg.message='';root.receive(msg);}
        function testState(): string {'''))
    config = temp / 'runtjik.conf'
    config.write_text('[ui]\nmonitor="DEREZ"\n[index]\nrefresh_seconds=300\n[plugins]\nenabled=["Screenshots","Recorder","Clipboard"]\n')
    env = {**os.environ, 'RUNTJIK_HOME': str(home), 'RUNTJIK_CONFIG': str(config),
           'RUNTJIK_DEMO': '1', 'XDG_STATE_HOME': str(temp / 'state'), 'QT_QUICK_CONTROLS_STYLE': 'Basic'}

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'runtjik', *map(str, args)], env=env, text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('animationState'))
        except subprocess.CalledProcessError:
            return {}

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('current', {}).get('rows'), timeout=20)
            wait_for(lambda: json.loads(ipc('status')).get('ready'))
            ipc('test', '/grab history')
            wait_for(lambda: state().get('query') == '/grab history')
            time.sleep(.3)
            before = state()['current']
            ipc('capture', temp / 'before.png')
            ipc('removeDemo', 0, temp / 'first')
            time.sleep(.9)
            result = state()
            frames = result['frames']
            assert len(result['current']['rows']) == 8, result
            assert all(not f['message'] for f in frames), frames
            assert any(any(.1 < e['progress'] < .9 for e in f['effects']) for f in frames), 'No intermediate derez frames'
            moved = before['rows'][1]
            original = next(t for t in before['tiles'] if t['id'] == moved)
            final = next(t for t in result['current']['tiles'] if t['id'] == moved)
            during = [next((t for t in f['tiles'] if t['id'] == moved), None) for f in frames if f['active']]
            assert all(t is None or (t['x'], t['y']) == (original['x'], original['y']) for t in during), during
            positions = {(round(t['x'], 2), round(t['y'], 2)) for f in frames for t in f['tiles'] if t['id'] == moved}
            assert len(positions) > 4 and (final['x'], final['y']) != (original['x'], original['y']), positions
            assert result['current']['active'] == 0 and not result['current']['effects']
            ipc('capture', temp / 'after.png')
            ipc('rejectDemo')
            rejected = state()['current']
            assert rejected['message'] == 'Synthetic deletion rejected' and len(rejected['rows']) == 8 and not rejected['effects']
            ipc('removeDemo', 0, temp / 'rapid-first')
            time.sleep(.1)
            ipc('removeDemo', 0, temp / 'rapid-second')
            time.sleep(.9)
            rapid = state()['current']
            assert len(rapid['rows']) == 6 and rapid['active'] == 0 and not rapid['effects'], rapid
            ipc('singleDemo')
            time.sleep(.3)
            ipc('removeDemo', 0, temp / 'last')
            time.sleep(.9)
            last = state()
            assert last['current']['empty'] and last['current']['active'] == 0
            assert any(f['effects'] for f in last['frames']), last
            ipc('capture', temp / 'empty.png')
            # A query switch during removal must dispose of the retained tile/effect.
            ipc('test', '/rec history')
            wait_for(lambda: state().get('query') == '/rec history')
            ipc('removeDemo', 0, temp / 'switch')
            ipc('test', '/grab')
            wait_for(lambda: state().get('query') == '/grab')
            time.sleep(.3)
            assert state()['current']['active'] == 0 and not state()['current']['effects']
            ipc('test', '/cliphist')
            wait_for(lambda: state().get('query') == '/cliphist')
            wait_for(lambda: not state().get('current', {}).get('pending', True))
            time.sleep(.3)
            ipc('removeDemo', 0, temp / 'clipboard')
            time.sleep(.9)
            clipboard = state()
            assert len(clipboard['current']['rows']) == 2 and not clipboard['current']['message']
            assert any(any(.1 < e['progress'] < .9 for e in f['effects']) for f in clipboard['frames'])
            positions = {(round(t['x'], 2), round(t['y'], 2)) for f in clipboard['frames'] for t in f['tiles'] if t['id'] == 'Clipboard:2'}
            assert len(positions) > 4, positions
            errors = (temp / 'launcher.log').read_text()
            assert not any(word in errors for word in ['ERROR', 'Error:', 'TypeError', 'ReferenceError', 'Binding loop']), errors
            print('PASS gallery and clipboard derez/FLIP, no success text, failure text, rapid deletion, last item and query-switch cleanup:', temp)
        except Exception:
            print((temp / 'launcher.log').read_text(), flush=True)
            raise
        finally:
            process.terminate()
            process.wait(timeout=5)
