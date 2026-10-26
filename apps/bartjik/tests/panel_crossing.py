"""Real Qt hover crossings reuse one native shell across preview providers."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
from render import wait_for
from details import committed_shell_regions

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-crossing-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        for name in ('CryptoPanel.qml', 'DetailPanel.qml', 'WorkspacePreview.qml', 'MediaDetails.qml'):
            p = temp / 'qml' / name
            p.write_text(p.read_text().replace('HoverHandler {', 'HoverHandler {enabled:false;'))
        p = temp / 'qml/Bar.qml'
        s = p.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        s = s.replace('    id: panel', '''    id: panel
    TestEvent {id:crossInput}
    function crossHover(name){
        if(!name){crossInput.mouseMove(contentItem,0,0,0,Qt.NoButton,Qt.NoModifier);return {ok:true};}
        const island=groups.children.find(c=>c.islandId===name),target=island.previewTargets()[0];
        crossInput.mouseMove(island,target.x+target.width/2,target.y+target.height/2,0,Qt.NoButton,Qt.NoModifier);
        return {ok:true};
    }''')
        p.write_text(s)
        p = temp / 'qml/shell.qml'
        s = p.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        s = s.replace('    id: root', '''    id: root
    property var crossingSamples:[]
    function crossingState(){
        const p=panelShell.activePanel,s=panelShell.surfaceFor(p?.panelScreen),f=s?.frame;
        return {opened:!!s?.active,kind:p===cryptoPanel?'crypto':p===detailPanel?'album':p?'workspace':'',
            x:f?.x || 0,y:f?.y || 0,width:f?.width || 0,height:f?.height || 0,
            edge:p?.anchor.rect.y || 0,progress:panelShell.contentProgress,
            surface:s?{width:s.width,height:s.height}:null,
            windows:workspacePreview.item?.sourceList.length || 0};
    }
    Timer {id:crossSamples;interval:8;repeat:true;onTriggered:root.crossingSamples.push(root.crossingState())}''')
        s = s.replace("case 'status':break;", '''case 'crossHover':return panels.instances[0].crossHover(req.island);
        case 'crossState':return {ok:true,panel:crossingState()};
        case 'crossSamples':if(req.start){crossingSamples=[];crossSamples.start();}else crossSamples.stop();return {ok:true,samples:crossingSamples};
        case 'status':break;''')
        p.write_text(s)
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        cfg['bars'] = cfg['bars'][:1]
        cfg['bars'][0].update(monitors=[1], autoHide=False)
        path = temp / 'config.json'
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_CONFIG=str(path), BARTJIK_QML=str(temp / 'qml'))
        log_path = destination / 'crossing.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], stdout=log, stderr=log,
                                       env={**os.environ, 'WAYLAND_DEBUG':'client'})
            try:
                wait_for(lambda: ipc({'op':'status'}).get('ready'))
                def state():
                    return ipc({'op':'crossState'})['panel']
                def move(island=None):
                    return ipc({'op':'crossHover','island':island})
                move('markets')
                time.sleep(.08)
                assert not state()['opened'], 'Cold opening lost its dwell delay'
                wait_for(lambda: state()['opened'])
                time.sleep(.2)
                log_start = len(log_path.read_text())
                states = []
                for island, kind in [('media','album'), ('workspaces','workspace'), ('markets','crypto'),
                                     ('workspaces','workspace'), ('media','album'), ('markets','crypto')]:
                    before = state()
                    ipc({'op':'crossSamples','start':True})
                    move()
                    time.sleep(.15)
                    assert state()['opened'], 'Source closed before crossing the gap'
                    started = time.monotonic()
                    move(island)
                    wait_for(lambda: state()['kind'] == kind and state()['progress'] == 1)
                    assert time.monotonic() - started < .3, 'Handoff repeated the cold opening delay'
                    samples = ipc({'op':'crossSamples'})['samples']
                    assert all(s['opened'] for s in samples), samples
                    assert any(0 < s['progress'] < 1 for s in samples), samples
                    assert len({(s['surface']['width'],s['surface']['height']) for s in samples}) == 1
                    assert all(abs(s['y'] + s['height'] - s['edge']) < 1 for s in samples), samples
                    if kind != 'workspace':
                        assert state()['windows'] == 0, 'Workspace captures survived the handoff'
                    after = state()
                    assert any(min(before['width'], after['width']) < s['width'] < max(before['width'], after['width']) for s in samples), samples
                    states.extend(samples)
                # Reverse direction while earlier snapshots/fades are still pending.
                for island in ('media', 'workspaces', 'markets', 'media', 'workspaces'):
                    move(island)
                    time.sleep(.025)
                wait_for(lambda: state()['kind'] == 'workspace' and state()['progress'] == 1)
                move()
                time.sleep(.15)
                assert state()['opened']
                wait_for(lambda: not state()['opened'])
                time.sleep(.1)
                trace = log_path.read_text()
                assert not re.search(r'get_layer_surface\(|get_popup\(|\.reposition\(', trace[log_start:])
                last = dict(committed_shell_regions(trace))
                assert last and all(region == [] for region in last.values()), last
                move('workspaces')
                time.sleep(.08)
                assert not state()['opened']
                move()
                time.sleep(.35)
                assert not state()['opened'], 'Cancelled cold entry reopened the shell'
                (destination / 'samples.json').write_text(json.dumps(states, indent=2))
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines()
                  if any(k in line for k in ('WARN scene', 'ERROR', 'Traceback', 'Binding loop'))]
        assert not errors, errors[:20]
    print('Crypto/album/workspace: six real hover crossings, 150ms gaps, continuous shell, crossfade, anchored geometry, capture release and cold dwell passed')


if __name__ == '__main__':
    main()
