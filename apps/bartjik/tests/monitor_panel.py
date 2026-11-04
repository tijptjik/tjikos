"""Actual display panel interaction and recovery banner on a private synthetic bar."""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tests'))
from render import wait_for


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
    dest=Path(sys.argv[1]).resolve();dest.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-displays-') as td:
        temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
        for name in ('Part.qml','PreviewHover.qml','DetailPanel.qml'):
            p=temp/'qml'/name;p.write_text(p.read_text().replace('HoverHandler {','HoverHandler {enabled:false;'))
        p=temp/'qml/MonitorDetails.qml'
        s=p.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1)
        s=s.replace('    id:details','''    id:details
    TestEvent {id:testInput}
    function testControl(req){
        const all=[];function walk(i){all.push(i);for(const c of i.children || [])walk(c);};walk(details);
        const target=all.find(i=>i.objectName===req.name);
        if(req.name && !target)throw Error('Missing control '+req.name);
        if(req.scroll){const view=details.parent.parent,y=target.mapToItem(details,0,0).y;view.contentY=Math.max(0,Math.min(y-20,view.contentHeight-view.height));}
        if(req.click)testInput.mouseClick(target,target.width/2,target.height/2,Qt.LeftButton,Qt.NoModifier,0);
        if(req.end){target.forceActiveFocus();testInput.keyClick(Qt.Key_End,Qt.NoModifier,0);testInput.keyClick(Qt.Key_Return,Qt.NoModifier,0);}
        if(req.drag){const origin=target.mapToItem(map,target.width/2,target.height/2);testInput.mousePress(map,origin.x,origin.y,Qt.LeftButton,Qt.NoModifier,0);testInput.mouseMove(map,origin.x+req.drag.x,origin.y+req.drag.y,20,Qt.LeftButton,Qt.NoModifier);testInput.mouseRelease(map,origin.x+req.drag.x,origin.y+req.drag.y,Qt.LeftButton,Qt.NoModifier,0);}
        return {state:inspect(),control:target?{enabled:target.enabled,visible:target.visible,currentIndex:target.currentIndex,displayText:target.displayText,checked:target.checked}:null};
    }
''',1);p.write_text(s)
        p=temp/'qml/DetailPanel.qml';s=p.read_text().replace('    id:popup','''    id:popup
    property var testPanel:null
    property var testEvents:[]
    function displayTest(req){if(req.panel)testPanel=req.panel;if(req.reset)testEvents=[];return {result:monitorDetails.testControl(req),events:testEvents,panel:panelData,inspect:inspect()};}
''',1).replace('readonly property var panelData:row?.panel ||','readonly property var panelData:testPanel || row?.panel ||')
        s=s.replace('function action(name,selection){actionFor(selected?.context.module,name,selection);}', 'function action(name,selection){testEvents=testEvents.concat([{name:name,selection:selection}]);}')
        p.write_text(s)
        p=temp/'qml/MonitorConfirmation.qml';s=p.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1)
        s=s.replace('    id:confirmation','''    id:confirmation
    TestEvent {id:confirmInput}
    function testConfirm(name){
        const windows=confirmWindows.instances;
        if(name && windows.length){const all=[];function walk(i){all.push(i);for(const c of i.children || [])walk(c);};walk(windows[0].contentItem);const button=all.find(i=>i.objectName===name);confirmInput.mouseClick(button,button.width/2,button.height/2,Qt.LeftButton,Qt.NoModifier,0);}
        return {count:windows.length,remaining:remaining};
    }
''',1).replace('    Variants {','    Variants {\n        id:confirmWindows',1).replace("'bartjik-display-confirmation'", "'bartjik-display-confirmation-test'");p.write_text(s)
        p=temp/'qml/shell.qml';s=p.read_text().replace('    id: root','    id: root\n    property var displayDecisions:[]',1)
        s=s.replace('MonitorConfirmation {controller:root}','MonitorConfirmation {id:testConfirmation;controller:root}')
        s=s.replace("function send(message) {if(backend.running)","function send(message) {if(message.op==='monitorDecision'){displayDecisions=displayDecisions.concat([message]);return;}if(backend.running)")
        s=s.replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}","if(msg.kind==='pointer'){}")
        s=s.replace("case 'status':break;","""case 'displayTest':return {ok:true,probe:detailPanel.displayTest(req)};
        case 'displayConfirm':
            if(req.change!==undefined)state=Object.assign({},state,{monitorChange:req.change});
            if(req.hideBars)state=Object.assign({},state,{bars:[]});
            return {ok:true,confirmation:testConfirmation.testConfirm(req.name),decisions:displayDecisions};
        case 'status':break;""");p.write_text(s)
        config=temp/'config.json';config.write_text(json.dumps({'theme':'rose-punk','modules':{'display':{'provider':'monitor-layout'}},'bars':[{'id':'test','autoHide':False,'islands':[{'id':'controls','modules':['display']}]}]}))
        env={**os.environ,'BARTJIK_QML':str(temp/'qml'),'BARTJIK_CONFIG':str(config)}
        def ipc(**request):
            r=subprocess.run(['quickshell','ipc','-p',str(temp/'qml'),'call','bartjik','request',json.dumps(request)],env=env,capture_output=True,text=True,timeout=5)
            if r.returncode:raise ValueError(r.stderr)
            v=json.loads(r.stdout);assert v.get('ok'),v;return v
        def probe(**request):return ipc(op='displayTest',**request)['probe']
        def control(name,**request):
            probe(name='display-'+name,scroll=True);time.sleep(.03)
            return probe(name='display-'+name,**request)
        def click(name):return control(name,click=True)
        def capture(name):
            image=dest/(name+'.png');time.sleep(.3);ipc(op='captureDetail',path=str(image));wait_for(lambda:image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
        log_path=dest/'render.log'
        with log_path.open('w') as log:
            p=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],env=env,stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc(op='status').get('ready'))
                ipc(op='detailTest',module='display')
                wait_for(lambda:ipc(op='detailState')['panel']['opened'])
                baseline=copy.deepcopy(probe()['panel'])
                assert len(baseline['monitors'])==3
                assert probe()['inspect']['provider']=='monitor-layout'
                assert control('scale')['result']['control']['displayText']=='100%'
                control('map');capture('display-arrangement')
                click('select-HDMI-A-1')
                click('enabled')
                state=probe()['result']['state']
                assert state['dirty'] and state['monitors'][2]['settings']['enabled']
                assert probe()['events']==[]
                click('apply')
                request=json.loads(probe()['events'][-1]['selection'])
                assert request['edits']==[{'name':'HDMI-A-1','settings':{'enabled':True}}],request
                click('reset');probe(reset=True)
                click('tv-mirror')
                state=probe()['result']['state']
                assert state['monitors'][1]['settings']['mirror']=='HDMI-A-1'
                assert state['monitors'][2]['settings']['enabled']
                assert not control('position-x')['result']['control']['enabled']
                assert not probe(name='display-map-DP-2')['result']['control']['visible']
                control('map');capture('display-tv-mirror')
                click('apply')
                request=json.loads(probe()['events'][-1]['selection'])
                assert request['edits']==[{'name':'DP-2','settings':{'mirror':'HDMI-A-1'}},{'name':'HDMI-A-1','settings':{'enabled':True}}],request
                click('desktop');assert not probe()['result']['state']['dirty']
                probe(reset=True);click('select-DP-2');click('mirror');control('mirror',end=True)
                assert probe()['result']['state']['monitors'][1]['settings']['mirror']=='DP-1'
                click('reset')
                click('select-DP-1')
                click('mode');control('mode',end=True)
                assert probe()['result']['state']['monitors'][0]['settings']['mode']=='3440x1440@60.00'
                click('scale');control('scale',end=True)
                assert probe()['result']['state']['monitors'][0]['settings']['scale']==4
                click('rotation');control('rotation',end=True)
                assert probe()['result']['state']['monitors'][0]['settings']['transform']==3
                click('colour');control('colour',end=True)
                assert probe()['result']['state']['monitors'][0]['settings']['cm']=='hdr'
                click('reset')
                control('map-DP-2',drag={'x':-40,'y':-20})
                state=probe()['result']['state']
                assert state['dirty'] and state['monitors'][1]['settings']['x']!=3440,state
                assert probe()['events']==[]
                capture('display-arrangement-draft')
                click('reset')
                click('select-DP-1');click('enabled')
                click('select-DP-2');click('enabled')
                assert not control('apply')['result']['control']['enabled']
                click('reset')
                unconfigured=copy.deepcopy(baseline);unconfigured['monitors'][0]['configured']=False
                probe(panel=unconfigured);click('select-DP-1')
                assert not control('enabled')['result']['control']['enabled']
                disconnected=copy.deepcopy(baseline);disconnected['monitors'][0]['connected']=False
                probe(panel=disconnected);click('select-DP-1')
                assert not control('enabled')['result']['control']['enabled']
                capture('display-disconnected')
                count=len(ipc(op='monitors')['monitors'])
                ipc(op='displayConfirm',change={'token':'fixture','deadline':time.time()+20,'phase':'confirm','error':''})
                wait_for(lambda:ipc(op='displayConfirm')['confirmation']['count']==count)
                # Read actual layer placement from the compositor, not a QML formula.
                def centered():
                    monitors={m['name']:m for m in json.loads(subprocess.check_output(['hyprctl','-j','monitors'],text=True))}
                    layers=json.loads(subprocess.check_output(['hyprctl','-j','layers'],text=True))
                    found=[]
                    for name,screen in layers.items():
                        for level in screen['levels'].values():
                            for layer in level:
                                if layer.get('namespace')!='bartjik-display-confirmation-test':continue
                                m=monitors[name];w,h=m['width']/m['scale'],m['height']/m['scale']
                                if m['transform']%2:w,h=h,w
                                found.append(abs(layer['x']+layer['w']/2-(m['x']+w/2))<=1 and abs(layer['y']+layer['h']/2-(m['y']+h/2))<=1)
                    return len(found)==count and all(found)
                wait_for(centered)
                result=ipc(op='displayConfirm',name='display-keep')
                assert result['decisions'][-1]['decision']=='keep',result
                # Recovery controls remain on active outputs even when the bar's output disappears.
                ipc(op='displayConfirm',hideBars=True)
                wait_for(lambda:ipc(op='displayConfirm')['confirmation']['count']==count)
                result=ipc(op='displayConfirm',name='display-revert')
                assert result['decisions'][-1]['decision']=='revert',result
                ipc(op='displayConfirm',change=None)
                wait_for(lambda:ipc(op='displayConfirm')['confirmation']['count']==0)
            finally:p.terminate();p.wait(timeout=5)
        errors=[line for line in log_path.read_text().splitlines() if any(word in line for word in ('WARN scene','ERROR','ReferenceError','TypeError','Binding loop'))]
        assert not errors,errors[:20]
    print('Displays: actual map, selection, staged toggles, modes, scaling, rotation, HDR, dragging, last-screen guard and confirmation on surviving screens passed')


if __name__=='__main__':main()
