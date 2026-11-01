"""Check rendered grid replacement with delayed results and synthetic demo data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
from private_compositor import private_compositor, wait_for

app = Path(os.environ.get('RUNTJIK_TEST_HOME', Path(__file__).resolve().parents[1]))
with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'GRID'],
                   check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'GRID' for m in json.loads(
        subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="GRID"}))'],
                   check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    function receive(msg) {', '''
    property int syntheticPolls: 0
    property var syntheticApps: []
    property bool holdGridResults: false
    property var heldGridResults: null
    property bool watchGridFrames: false
    property int blankGridFrames: 0
    FrameAnimation {
        running: root.watchGridFrames
        onTriggered: {
            const tiles=Array.from(items.contentItem.children).filter(c=>c.modelData && c.visible && c.iconReady);
            if(root.results.length && (!items.visible || !tiles.length))root.blankGridFrames++;
        }
    }
    function receive(msg) {
        if(holdGridResults && msg.kind==='results'){heldGridResults=msg;return;}
        if(msg.kind==='results' && msg.query==='') {
            const seeds=msg.results;
            msg.results=Array.from({length:100},(_,i)=>Object.assign({},seeds[i%seeds.length],{id:'synthetic:'+i,name:'Synthetic app '+i}));
            msg.total=100;root.syntheticApps=msg.results;
        }
        if(msg.kind==='results' && msg.query==='?'){msg.results=(root.syntheticPolls++===0?root.syntheticApps.slice(0,15):root.syntheticApps.slice(10,15)).map(row=>Object.assign({},row,{windows:1}));msg.total=5;}
        if(msg.kind==='results' && (msg.query || '').trim()==='/rec')
            msg.results=msg.results.filter(row=>row.id!=='Recorder:stop');
''').replace('        function testState(): string {', '''
        function pollResults(): void {root.search();}
        function holdResults(hold: bool): void {
            root.holdGridResults=hold;
            if(!hold && root.heldGridResults!==null){const msg=root.heldGridResults;root.heldGridResults=null;root.receive(msg);}
        }
        function watchFrames(): void {root.blankGridFrames=0;root.watchGridFrames=true;}
        function testState(): string {
''').replace('statistics:root.statistics,', '''
                originY:items.originY,gridVisible:items.visible,held:root.heldGridResults!==null,cellWidth:items.cellWidth,blankFrames:root.blankGridFrames,
                delegates:Array.from(items.contentItem.children).filter(c=>c.modelData && c.visible).map(c=>({id:c.modelData.id,index:c.index,x:c.x,y:c.y,width:c.width,height:c.height})),
                statistics:root.statistics,''').replace('property string iconSource:', 'readonly property bool iconReady: art.status===Image.Ready\n                                property string iconSource:'))
    config = temp / 'runtjik.conf'
    config.write_text('[ui]\ncolumns=5\nmonitor="GRID"\n[plugins]\nenabled=["Screenshots","Recorder"]\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'), QT_QUICK_CONTROLS_STYLE='Basic')

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
                                        'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    def check_tiles():
        current = state()
        tiles = current['delegates']
        expected = [row['id'] for row in current['rows']]
        if sorted(tile['id'] for tile in tiles) != sorted(expected):
            return False
        for tile in tiles:
            assert tile['index'] == expected.index(tile['id']), current
            for other in tiles:
                if tile is other:
                    continue
                overlap = (min(tile['x'] + tile['width'], other['x'] + other['width'])
                           - max(tile['x'], other['x']) > 1
                           and min(tile['y'] + tile['height'], other['y'] + other['height'])
                           - max(tile['y'], other['y']) > 1)
                assert not overlap, (tile, other)
        return True

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], stdout=log, stderr=log)
        try:
            wait_for(lambda: len(state().get('rows', [])) == 100)
            wait_for(lambda: json.loads(ipc('status')).get('ready'))
            wait_for(lambda: len(state().get('delegates', [])) > 0)
            ipc('test', '?')
            wait_for(lambda: state().get('resultQuery') == '?')
            wait_for(check_tiles)
            ipc('pollResults')
            import time
            time.sleep(.6)
            current = state()
            assert current['resultQuery'] == '?', current
            first = min(current['delegates'], key=lambda tile: tile['index'])
            assert abs(first['y'] - current['scrollY']) < 1, current
            ipc('capture', str(temp / 'running.png'))
            wait_for(lambda: (temp / 'running.png').exists())
            print('PASS: filtered running apps stay at the top after refresh; screenshot:', temp / 'running.png')
        finally:
            process.terminate()
            process.wait(timeout=5)
