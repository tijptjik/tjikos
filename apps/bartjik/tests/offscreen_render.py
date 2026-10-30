"""Private-bus synthetic preview inset rendering; no desktop content captured."""

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

ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT),str(ROOT/'tests')]
from bartjik import ipc
from render import wait_for

assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
dest=Path(sys.argv[1]).resolve();dest.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='bartjik-inset-') as td:
    temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
    p=temp/'qml/shell.qml';s=p.read_text().replace('    id: root','    id: root\n    property var testOffscreen:({left:[],right:[]})')
    s=s.replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}","if(msg.kind==='pointer'){}")
    s=s.replace('return (state.providers[bar.sources[id]] || {}).items || [];', '''const rows=(state.providers[bar.sources[id]] || {}).items || [];
        if(demo && m.provider==='workspaces')return rows.map(r=>Object.assign({},r,{offscreen:r.id==='2'?{left:[],right:[]}:testOffscreen}));
        return rows;''')
    s=s.replace("case 'status':break;", '''case 'offscreenTest':
            if(req.windows)testOffscreen=req.windows;
            if(req.interaction)workspacePreview.item.interactOffscreen(req.interaction);
            return {ok:true,probe:workspacePreview.item?.offscreenProbe(),barIcons:panels.instances.map(p=>p.offscreenBarIcons())};
        case 'status':break;''');p.write_text(s)
    p=temp/'qml/PanelButton.qml';p.write_text(p.read_text().replace('PanelToolTip {', "PanelToolTip {objectName:'test-panel-tooltip'"))
    p=temp/'qml/WorkspacePreview.qml';s=p.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1).replace('    id:popup', "    id:popup\n    TestEvent {id:offscreenInput}\n    property string lastTestFocus:''").replace('if(!controller.demo)Hyprland.dispatch', 'lastTestFocus=address;if(!controller.demo)Hyprland.dispatch').replace('    function inspect()', '''    function offscreenButtons() {
        let buttons=[];
        function visit(q) {if(q.objectName==='offscreen-app-button')buttons.push(q);for(const c of q.children || [])visit(c);}
        visit(offscreenInset);return buttons;
    }
    function interactOffscreen(action) {
        const button=offscreenButtons()[0];
        if(action==='hover')offscreenInput.mouseMove(button,button.width/2,button.height/2,0,Qt.NoButton,Qt.NoModifier);
        else if(action==='click')offscreenInput.mouseClick(button,button.width/2,button.height/2,Qt.LeftButton,Qt.NoModifier,0);
    }
    function offscreenProbe() {
        let icons=[];
        function visit(q) {
            if(q.objectName==='offscreen-app-icon')icons.push(q);
            for(const c of q.children || [])visit(c);
        }
        visit(offscreenInset);
        const button=offscreenButtons()[0];
        const tip=button?.data.find(c=>c.objectName==='test-panel-tooltip');
        return {focus:lastTestFocus,tooltip:tip?{hovered:button.hovered,enabled:button.enabled,visible:tip.visible,text:tip.text,color:String(tip.background.color),radius:tip.background.radius}:null,state:inspect(),icons:icons.map(i=>({status:i.status,x:i.mapToItem(offscreenInset,0,0).x})),
            midpoint:offscreenInset.width/2,desktopBottom:desktop.y+desktop.height};
    }
    function inspect()''');p.write_text(s)
    p=temp/'qml/Bar.qml';s=p.read_text().replace('    id: panel','''    id: panel
    function offscreenBarIcons() {
        let count=0;
        function visit(q) {if(q.objectName==='offscreen-app-icon')++count;for(const c of q.children || [])visit(c);}
        visit(contentItem);return count;
    }''');p.write_text(s)
    cfg=json.loads((ROOT/'examples/comparison.json').read_text())
    cfg['bars']=cfg['bars'][:1];cfg['bars'][0].update(monitors='all',autoHide=False)
    (temp/'config.json').write_text(json.dumps(cfg))
    os.environ.update(BARTJIK_QML=str(temp/'qml'),BARTJIK_CONFIG=str(temp/'config.json'))
    with (dest/'render.log').open('w') as log:
        process=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
        try:
            wait_for(lambda:ipc({'op':'status'}).get('ready'))
            for monitor in ipc({'op':'monitors'})['monitors']:
                for name, windows in [
                    ('both',{'left':[{'app':'kitty','address':'0xa1','title':'Project notes — Kitty'},{'app':'zen','address':'0xa2'}],'right':[{'app':'dev.zed.Zed-Preview','address':'0xb1'}]}),
                    ('right',{'left':[],'right':[{'app':'kitty','address':'0xa1','title':'Project notes — Kitty'},{'app':'not-a-real-app','address':'0xb2'}]}),
                    ('left',{'left':[{'app':'zen','address':'0xa2'}],'right':[]}),
                    ('empty',{'left':[],'right':[]}),
                ]:
                    ipc({'op':'offscreenTest','windows':windows})
                    ipc({'op':'workspacePreviewTest','module':'spaces','item':'1','x':500,'monitor':monitor['name']})
                    wait_for(lambda:ipc({'op':'workspacePreviewState'})['preview']['opened'])
                    time.sleep(.25)
                    result=ipc({'op':'offscreenTest'});p=result['probe'];state=p['state'];inset=state['offscreen']
                    assert all(n==0 for n in result['barIcons']),result
                    assert inset['visible']==(name!='empty'),p
                    assert len(p['icons'])==len(windows['left'])+len(windows['right']),p
                    assert all(i['status']==1 for i in p['icons']),p
                    assert abs(inset['y']+inset['height']-p['desktopBottom'])<.1,p
                    assert abs(inset['x']+inset['width']/2-state['width']/2)<1,p
                    for notch in inset['notches']:
                        assert notch['visible']==bool(windows[notch['side']]),p
                        if notch['side']=='left':assert notch['x']+notch['width']<p['midpoint'],p
                        else:assert notch['x']>p['midpoint'],p
                    for i,icon in enumerate(p['icons']):assert (icon['x']<p['midpoint'])==(i<len(windows['left'])),p
                    path=dest/(name+'-'+monitor['name']+'.png')
                    ipc({'op':'captureWorkspace','path':str(path)})
                    wait_for(lambda:path.exists() and path.read_bytes().endswith(b'IEND\xaeB`\x82'))
                    (dest/(name+'-'+monitor['name']+'.json')).write_text(json.dumps(result,indent=2))
                    if name=='both':
                        ipc({'op':'offscreenTest','interaction':'hover'})
                        time.sleep(.8)
                        tip=ipc({'op':'offscreenTest'})['probe']['tooltip']
                        assert tip and tip['visible'] and tip['color']=='#1f1d2e' and tip['radius']==8,tip
                        assert tip['text']=='Project notes — Kitty',tip
                        clicked=ipc({'op':'offscreenTest','interaction':'click'})['probe']
                        assert clicked['focus']=='0xa1' and not clicked['state']['opened'],clicked
                # Switching the hovered workspace must replace the inset's content.
                ipc({'op':'offscreenTest','windows':{'left':[{'app':'kitty','address':'0xa1','title':'Project notes — Kitty'}],'right':[]}})
                ipc({'op':'workspacePreviewTest','module':'spaces','item':'2','x':500,'monitor':monitor['name']})
                wait_for(lambda:ipc({'op':'workspacePreviewState'})['preview']['key'].endswith('/2'))
                assert not ipc({'op':'workspacePreviewState'})['preview']['offscreen']['visible']
        finally:
            process.terminate();process.wait(timeout=5)
errors=[s for s in (dest/'render.log').read_text().splitlines() if any(e in s for e in ('WARN scene','ERROR','ReferenceError','TypeError','Binding loop'))]
assert not errors,errors
print('PASS: independent left/right preview notches, single-side and empty states, exact-window clicks, themed tooltips, hovered workspace changes, and no bar icons on both monitors.')
