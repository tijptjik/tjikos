"""Bound pointer bursts in production hover panels, using real Qt input and demo data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from render import wait_for

ROOT = Path(__file__).resolve().parents[1]


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    with tempfile.TemporaryDirectory(prefix='bartjik-hover-motion-') as directory:
        temp = Path(directory)
        qml = temp / 'qml'
        shutil.copytree(ROOT / 'qml', qml)
        path = qml / 'Island.qml'
        source = path.read_text().replace('    function previewTargets() {',
            '    property int targetCalls:0\n    function previewTargets() {++targetCalls;', 1)
        path.write_text(source)
        for filename, prop in [('WorkspacePreview.qml', 'selected'),
                               ('DetailPanel.qml', 'selected'),
                               ('CryptoPanel.qml', 'selectedContext')]:
            path = qml / filename
            source = path.read_text().replace('    required property var controller',
                '    property int selectionChanges:0\n    on' + prop[0].upper() + prop[1:] +
                'Changed:++selectionChanges\n    required property var controller', 1)
            if filename == 'WorkspacePreview.qml':
                source = source.replace('    property bool opened: false',
                    '    property int sourceChanges:0\n    property bool opened: false', 1)
                source = source.replace('onSourceListChanged:Logic.sync(windowKeys,sourceList.map(t=>t.address))',
                    'onSourceListChanged:{++sourceChanges;Logic.sync(windowKeys,sourceList.map(t=>t.address));}', 1)
            path.write_text(source)
        path = qml / 'Bar.qml'
        source = path.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        source = source.replace('    id: panel', '''    id: panel
    TestEvent {id:motionInput}
    function motionTest(req) {
        if(req.inspect)return {ok:true};
        const island=groups.children.find(c=>c.islandId===req.island);
        const target=island.previewTargets()[req.index || 0];
        const x=target.x+target.width/2,y=target.y+target.height/2;
        const start=Date.now(),calls=island.targetCalls;
        if(req.leave)motionInput.mouseMove(contentItem,0,0,0,Qt.NoButton,Qt.NoModifier);
        else for(let j=0;j<(req.count || 1);++j)
            motionInput.mouseMove(island,x+(j%2),y,0,Qt.NoButton,Qt.NoModifier);
        return {ok:true,elapsed:Date.now()-start,targetCalls:island.targetCalls-calls,item:target.row.id};
    }''', 1)
        path.write_text(source)
        path = qml / 'shell.qml'
        source = path.read_text().replace(
            "if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}",
            "if(msg.kind==='pointer'){}", 1)
        source = source.replace("case 'status':break;", '''case 'motionTest': {
            const result=panels.instances.find(p=>p.bar.id==='workspace-bar').motionTest(req);
            result.workspace={opened:workspacePreview.item.opened,changes:workspacePreview.item.selectionChanges,
                sources:workspacePreview.item.sourceChanges,key:workspacePreview.item.selectedKey};
            result.detail={opened:detailPanel.opened,changes:detailPanel.selectionChanges};
            result.crypto={opened:cryptoPanel.opened,changes:cryptoPanel.selectionChanges};
            return result;
        }
        case 'status':break;''', 1)
        path.write_text(source)
        config = json.loads((ROOT / 'examples/comparison.json').read_text())
        config['layout']['centre_width'] = '100%'
        config['bars'] = [dict(config['bars'][0], monitors='all', autoHide=False,
                              position='c', alignment='c')]
        config['bars'][0]['islands'].append({'id': 'audio', 'modules': ['sound']})
        config['modules']['sound']['actions']['hoverEnter'] = 'audio.panel'
        config_path = temp / 'config.json'
        config_path.write_text(json.dumps(config))
        env = dict(os.environ, BARTJIK_QML=str(qml), BARTJIK_CONFIG=str(config_path))

        def ipc(req):
            call = subprocess.run(['quickshell', 'ipc', '--path', str(qml),
                'call', 'bartjik', 'request', json.dumps(req)], env=env, text=True,
                capture_output=True, timeout=10)
            if call.returncode:
                raise OSError(call.stderr or call.stdout)
            result = json.loads(call.stdout)
            assert result.get('ok'), result
            return result

        def state():
            return ipc({'op': 'motionTest', 'inspect': True})

        log_path = temp / 'motion.log'
        results = []
        with log_path.open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'],
                                       env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc({'op': 'status'}).get('ready'))
                time.sleep(.5)
                for island, panel in [('workspaces', 'workspace'), ('markets', 'crypto'),
                                      ('audio', 'detail')]:
                    ipc({'op': 'motionTest', 'island': island})
                    wait_for(lambda: state()[panel]['opened'])
                    time.sleep(.2)
                    before = state()[panel]
                    burst = ipc({'op': 'motionTest', 'island': island, 'count': 1000})
                    time.sleep(.08)
                    after = state()[panel]
                    assert after['opened'], after
                    assert after['changes'] == before['changes'], (island, before, after)
                    assert burst['targetCalls'] <= 2, burst
                    if panel == 'workspace':
                        assert after['sources'] == before['sources'], (before, after)
                        target = ipc({'op': 'motionTest', 'island': island, 'index': 1})['item']
                        wait_for(lambda: state()['workspace']['key'].endswith('/' + target + '/item'))
                    ipc({'op': 'motionTest', 'island': island, 'leave': True})
                    wait_for(lambda: not state()[panel]['opened'])
                    # A leave must cancel both the dwell and the pending motion sample.
                    ipc({'op': 'motionTest', 'island': island, 'count': 1000})
                    ipc({'op': 'motionTest', 'island': island, 'leave': True})
                    time.sleep(.45)
                    assert not state()[panel]['opened'], state()
                    results.append({'panel': panel, 'events': 1000, 'elapsedMs': burst['elapsed'],
                                    'selectionRebuilds': after['changes'] - before['changes']})
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines()
                  if any(word in line for word in ('WARN scene', 'ERROR', 'Binding loop'))]
        assert not errors, errors[:20]
        print(json.dumps(results), flush=True)
    print('Hover motion passed: bounded bursts, stable selections, workspace switching and leave cancellation')


if __name__ == '__main__':
    main()
