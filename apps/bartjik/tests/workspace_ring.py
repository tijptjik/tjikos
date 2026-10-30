"""Private-bus synthetic per-monitor labels, click targets and moving ring."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
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
sys.path.insert(0,str(ROOT))
from bartjik import ipc
from bartjik_core import workspaces


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
    destination=Path(sys.argv[1]).resolve();destination.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-ring-') as td:
        temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
        p=temp/'qml/Bar.qml';s=p.read_text().replace('import QtQuick','import QtQuick\nimport QtTest',1)
        s=s.replace('    id: panel','''    id: panel
    TestEvent {id:ringInput}
    function ringProbe(click,hover) {
        let ring=null,parts=[];
        function visit(q) {
            if(q.objectName==='workspace-active-ring')ring=q;
            if(q.kind==='text' && q.moduleConfig?.provider==='workspaces' && q.visible)parts.push(q);
            for(const c of q.children || [])visit(c);
        }
        visit(contentItem);
        if(hover==='leave')ringInput.mouseMove(contentItem,0,0,0,Qt.NoButton,Qt.NoModifier);
        else if(hover){const p=parts.find(p=>p.row.id===hover);ringInput.mouseMove(p,p.width/2,p.height/2,0,Qt.NoButton,Qt.NoModifier);}
        if(click){const p=parts.find(p=>p.row.id===click);ringInput.mouseClick(p,2,p.height/2,Qt.LeftButton,Qt.NoModifier,0);}
        return {ring:{x:ring.x,y:ring.y,width:ring.width,height:ring.height,visible:ring.visible,color:String(ring.border.color)},
            parts:parts.map(p=>({id:p.row.id,label:p.label,active:p.row.state.active,dot:p.workspaceActivation,dotColor:String(p.children.find(c=>c.objectName==='workspace-active-dot').color),hovered:p.hovered,opacity:p.opacity,x:p.mapToItem(ring.parent,0,0).x,width:p.width}))};
    }''');p.write_text(s)
        p=temp/'qml/shell.qml';s=p.read_text().replace('    id: root','    id: root\n    property var testWorkspaceRows:({})\n    property var testActions:[]')
        s=s.replace('if(demo)return;',"if(demo){testActions=testActions.concat([{action:action,id:row.id,workspace:row.workspace,monitor:context.monitor}]);return;}")
        s=s.replace('const m=bar.moduleConfigs[id];if(!m)return [];',"const m=bar.moduleConfigs[id];if(!m)return [];if(m.provider==='workspaces' && testWorkspaceRows[bar.screen])return testWorkspaceRows[bar.screen];")
        s=s.replace("case 'status':break;",'''case 'ringTest':
            if(req.rows)testWorkspaceRows=req.rows;
            return {ok:true,actions:testActions,panels:panels.instances.filter(p=>p.bar.id==='workspace-bar').map(p=>Object.assign({monitor:p.bar.screen},p.ringProbe(req.monitor===p.bar.screen?req.click:'',req.monitor===p.bar.screen?req.hover:'')))};
        case 'status':break;''');p.write_text(s)
        cfg=json.loads((ROOT/'examples/comparison.json').read_text())
        for bar in cfg['bars']:bar['autoHide']=False
        (temp/'config.json').write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp/'qml'),BARTJIK_CONFIG=str(temp/'config.json'))
        labels={'10':'X',**{str(i+10):str(i) if i<10 else 'X' for i in range(1,11)}}
        def rows(first,second):
            snap={'monitors':[{'name':'DP-1','activeWorkspace':{'id':first}}, {'name':'DP-2','activeWorkspace':{'id':second}}],
                  'workspaces':[{'id':n,'name':str(n),'monitor':'DP-1' if n<=10 else 'DP-2','windows':1} for n in (1,2,3,6,10,11,12,14,20)]}
            return {screen:workspaces.view(snap,screen,{'labels':labels,'occupiedOnly':True}) for screen in ('DP-1','DP-2')}
        def probe():return {p['monitor']:p for p in ipc({'op':'ringTest'})['panels']}
        def target(p):
            active=next(v for v in p['parts'] if v['active'])
            return active['x']+(active['width']-p['ring']['width'])/2
        def settled():
            value=probe()
            return value if all(p['ring']['visible'] and abs(p['ring']['x']-target(p))<.05 and all(abs(v['dot']-(1 if v['active'] else 0))<.01 for v in p['parts']) for p in value.values()) else False
        with (destination/'ring.log').open('w') as log:
            process=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({'op':'status'}).get('ready'))
                ipc({'op':'ringTest','rows':rows(1,14)})
                start=wait_for(settled)
                assert [p['label'] for p in start['DP-1']['parts']]==['1','2','3','6','X']
                assert [p['label'] for p in start['DP-2']['parts']]==['1','2','4','X']
                assert start['DP-1']['ring']['width']==start['DP-1']['ring']['height']==22
                ipc({'op':'ringTest','rows':rows(10,14)})
                samples=[];until=time.monotonic()+.5
                while time.monotonic()<until:
                    samples.append(probe());time.sleep(.015)
                end=wait_for(settled)
                left=start['DP-1']['ring']['x'];right=end['DP-1']['ring']['x']
                assert any(left+1<p['DP-1']['ring']['x']<right-1 for p in samples),samples
                assert all(abs(p['DP-2']['ring']['x']-start['DP-2']['ring']['x'])<.05 for p in samples)
                assert all(p['DP-1']['ring']['visible'] for p in samples)
                assert any(0<v['dot']<1 for p in samples for v in p['DP-1']['parts'])
                # Reverse direction while moving; the same ring must settle on the latest target.
                ipc({'op':'ringTest','rows':rows(1,20)});time.sleep(.05)
                ipc({'op':'ringTest','rows':rows(6,20)})
                wait_for(settled)
                ipc({'op':'ringTest','monitor':'DP-2','click':'20'})
                event=ipc({'op':'ringTest'})['actions'][-1]
                assert event=={'action':'workspace.activate','id':'20','workspace':20,'monitor':'DP-2'},event
                for hover in ('20','leave'):
                    ipc({'op':'ringTest','monitor':'DP-2','hover':hover})
                    active=next(p for p in probe()['DP-2']['parts'] if p['id']=='20')
                    assert active['hovered']==(hover=='20'),active
                    assert active['dot']==1 and active['dotColor']=='#ffffff',active
                ipc({'op':'ringTest','monitor':'DP-2','hover':'11'})
                inactive=next(p for p in probe()['DP-2']['parts'] if p['id']=='11')
                assert inactive['hovered'] and inactive['opacity']==1,inactive
                ipc({'op':'ringTest','monitor':'DP-2','hover':'leave'})
                inactive=next(p for p in probe()['DP-2']['parts'] if p['id']=='11')
                assert not inactive['hovered'] and inactive['opacity']==.65,inactive
                for monitor in ('DP-1','DP-2'):
                    image=destination/(monitor+'-ring.png')
                    ipc({'op':'capture','bar':monitor+'/workspace-bar','path':str(image)})
                    wait_for(lambda:image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in (destination/'ring.log').read_text().splitlines() if any(s in line for s in ('WARN scene','ERROR','ReferenceError','TypeError','Binding loop'))]
        assert not errors,errors
    print('PASS: moving rings, reversible number-to-dot transitions, white active dots on hover, per-monitor labels and original click IDs.')


if __name__=='__main__':main()
