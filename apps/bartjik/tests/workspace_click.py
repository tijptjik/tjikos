"""Real Qt clicks in scaled workspace thumbnails, on a private synthetic surface."""
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
sys.path.insert(0, str(ROOT / 'tests/reference'))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-window-click-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        path = temp / 'qml/WorkspacePreview.qml'
        source = path.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        source = source.replace('    id:popup', '''    id:popup
    TestEvent {id:testInput}
    property var testWindows:null
    property var testDispatches:[]
    function testGesture(req){
        if(req.windows!==undefined)testWindows=req.windows;
        const x=desktop.x+desktop.width*(req.x ?? .2),y=desktop.y+desktop.height*(req.y ?? .5);
        if(req.action==='click')testInput.mouseClick(body,x,y,Qt.LeftButton,Qt.NoModifier,0);
        if(req.action==='press')testInput.mousePress(body,x,y,Qt.LeftButton,Qt.NoModifier,0);
        if(req.action==='move')testInput.mouseMove(body,x,y,0,Qt.LeftButton,Qt.NoModifier);
        if(req.action==='release')testInput.mouseRelease(body,x,y,Qt.LeftButton,Qt.NoModifier,0);
        return {ok:true,opened:opened,dragging:dragging,detached:detached,actions:testDispatches};
    }''', 1)
        source = source.replace('controller.demo ? [', 'controller.demo ? testWindows || [', 1)
        # Record the dispatch boundary; never send focus commands to any compositor.
        source = source.replace("if(!controller.demo)Hyprland.dispatch('hl.dsp.focus", "testDispatches.push('hl.dsp.focus", 1)
        path.write_text(source)
        path = temp / 'qml/shell.qml'
        source = path.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        source = source.replace("case 'status':break;", "case 'windowGesture':return workspacePreview.item.testGesture(req);\n        case 'status':break;", 1)
        path.write_text(source)
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        cfg['bars'] = [dict(cfg['bars'][0], monitors='all', autoHide=False)]
        config = temp / 'config.json'
        config.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp / 'qml'), BARTJIK_CONFIG=str(config))
        log_path = destination / 'workspace-click.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc({'op':'status'}).get('ready'))
                def state():
                    return ipc({'op':'workspacePreviewState'})['preview']
                def gesture(action='state', **kw):
                    return ipc(dict(op='windowGesture', action=action, **kw))
                def show():
                    ipc({'op':'workspacePreviewTest', 'module':'spaces', 'item':'2', 'x':600})
                    wait_for(lambda: state()['opened'])
                    time.sleep(.2)
                def expect_focus(x, address):
                    before = len(gesture()['actions'])
                    result = gesture('click', x=x)
                    assert not result['opened'], result
                    assert result['actions'][before:] == ['hl.dsp.focus({ window = "address:'+address+'" })'], result
                show()
                expect_focus(.2, 'demo-editor')
                show()
                expect_focus(.85, 'demo-terminal')
                screen = ipc({'op':'monitors'})['monitors'][0]
                def window(address, x, width):
                    return dict(address=address, workspace={'name':'2'}, lastIpcObject={
                        'at':[screen['width']*x, 0], 'size':[screen['width']*width, screen['height']], 'class':'Fixture'})
                # An overlapping foreground window wins; a partially offscreen
                # window stays clickable only inside the clipped desktop.
                show()
                gesture(windows=[window('0x11', -.2, .8), window('0x22', .3, .5)])
                time.sleep(.1)
                expect_focus(.4, '0x22')
                show()
                expect_focus(.1, '0x11')
                show()
                before = gesture()['actions']
                for x in [-.01, .95]:
                    result = gesture('click', x=x)
                    assert result['opened'] and result['actions']==before, result
                # Moving beyond the detach threshold never focuses the window.
                gesture('press', x=.1)
                gesture('move', x=.25, y=.3)
                assert gesture()['dragging'], gesture()
                gesture('release', x=.25, y=.3)
                wait_for(lambda: gesture()['detached'])
                assert gesture()['actions']==before and gesture()['opened'], gesture()
                wait_for(lambda: not state()['dragging'])
                # Click regions follow the body into the detached native window.
                time.sleep(.4)
                expect_focus(.1, '0x11')
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines()
                  if any(s in line for s in ('WARN scene', 'ERROR', 'Traceback', 'Binding loop', 'TypeError', 'ReferenceError'))]
        assert not errors, errors[:20]
    print('Workspace clicks passed: scaled bounds, stacking, clipping, gaps, drag suppression and detached clicks')


if __name__ == '__main__':
    main()
