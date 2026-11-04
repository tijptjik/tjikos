"""Focus real synthetic windows through the unmocked preview action on private Hyprland."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from render import wait_for

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tests/reference'))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
    assert os.environ.get('XDG_RUNTIME_DIR','').startswith('/tmp/')
    destination=Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bf-') as td:
        temp=Path(td)
        shutil.copytree(ROOT/'qml',temp/'qml')
        path=temp/'qml/WorkspacePreview.qml'
        source=path.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1)
        source=source.replace('    id:popup','''    id:popup
    TestEvent {id:testInput}
    function testFocus(req) {
        const thumb=thumbs.children.find(t=>t.clientData?.title===req.title);
        if(!thumb)throw Error('missing synthetic thumbnail '+req.title);
        const point=thumb.mapToItem(body,thumb.width/2,thumb.height/2);
        if(req.click)testInput.mouseClick(body,point.x,point.y,Qt.LeftButton,Qt.NoModifier,0);
        return {ok:true,key:thumb.key,raw:thumb.clientData.address,opened:opened,point:{x:point.x,y:point.y},width:body.width,height:body.height};
    }
    Timer {interval:50;running:popup.opened;repeat:true;onTriggered:Hyprland.refreshToplevels()}
''',1)
        # Keep every backend/provider in demo mode, but use actual synthetic
        # compositor windows and the production focus action without a recorder.
        source=source.replace('!opened ? [] : controller.demo ? [','!opened ? [] : false ? [',1)
        source=source.replace('popup.controller.demo ? {x:0,y:0}','false ? {x:0,y:0}',1)
        start=source.index('    function focusWindow(')
        end=source.index('    function closeOffscreen(',start)
        source=source[:start]+source[start:end].replace('controller.demo','false')+source[end:]
        path.write_text(source)
        path=temp/'qml/shell.qml'
        source=path.read_text().replace('    id: root','''    id: root
    FloatingWindow {visible:true;title:'Preview focus left';implicitWidth:350;implicitHeight:400}
    FloatingWindow {visible:true;title:'Preview focus right';implicitWidth:350;implicitHeight:400}
''',1)
        source=source.replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}","if(msg.kind==='pointer'){}")
        source=source.replace("case 'status':break;","case 'focusFixture':return workspacePreview.item.testFocus(req);\n        case 'status':break;",1)
        path.write_text(source)
        cfg=json.loads((ROOT/'examples/comparison.json').read_text())
        cfg['bars']=[dict(cfg['bars'][0],monitors='all',autoHide=False)]
        config=temp/'config.json'
        config.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp/'qml'),BARTJIK_CONFIG=str(config))
        def query(kind):
            return json.loads(subprocess.check_output(['hyprctl','-j',kind],text=True))
        def eval_lua(code):
            result=subprocess.check_output(['hyprctl','eval',code],text=True).strip()
            assert result=='ok',result
        log_path=destination/'workspace-focus.log'
        with log_path.open('w') as log:
            process=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({'op':'status'}).get('ready'))
                eval_lua('hl.config({animations={enabled=false}})')
                windows=wait_for(lambda:[c for c in query('clients') if c['title'].startswith('Preview focus ')] or None)
                assert len(windows)==2,windows
                monitor=query('monitors')[0]
                by_title={c['title']:c for c in windows}
                for i,title in enumerate(['Preview focus left','Preview focus right']):
                    address=by_title[title]['address']
                    target=json.dumps('address:'+address)
                    eval_lua('local w=hl.get_window('+target+');hl.dispatch(hl.dsp.window.float({window=w}));hl.dispatch(hl.dsp.window.resize({window=w,x=350,y=400,relative=false}));hl.dispatch(hl.dsp.window.move({window=w,x='+str(monitor['x']+30+i*550)+',y='+str(monitor['y']+80)+',relative=false}))')
                time.sleep(.5)
                for title in ['Preview focus left','Preview focus right']:
                    other='Preview focus right' if title.endswith('left') else 'Preview focus left'
                    eval_lua('hl.dispatch(hl.dsp.focus({window='+json.dumps('address:'+by_title[other]['address'])+'}))')
                    wait_for(lambda:query('activewindow').get('address')==by_title[other]['address'])
                    ipc({'op':'workspacePreviewTest','module':'spaces','item':str(by_title[title]['workspace']['id']),'x':600})
                    wait_for(lambda:ipc({'op':'workspacePreviewState'})['preview']['opened'])
                    wait_for(lambda:ipc({'op':'workspacePreviewState'})['preview']['windows']==2)
                    time.sleep(.2)
                    probe=ipc({'op':'focusFixture','title':title,'click':True})
                    assert probe['raw']=='0x'+probe['key'],probe
                    assert not probe['opened'],probe
                    wait_for(lambda:query('activewindow').get('address')==by_title[title]['address'])
                    wait_for(lambda:not ipc({'op':'workspacePreviewState'})['preview']['opened'])
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in log_path.read_text().splitlines() if any(s in line for s in ('WARN scene','ERROR','TypeError','ReferenceError'))]
        assert not errors,errors[:20]
    print('Preview focus passed: real Quickshell addresses, unmocked compositor actions and confirmed active windows')


if __name__=='__main__':
    main()
