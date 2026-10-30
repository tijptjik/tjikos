"""Real Qt clicks in synthetic panels; commands are captured, never executed."""
import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from render import wait_for
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
    with tempfile.TemporaryDirectory(prefix='bartjik-panel-controls-') as td:
        temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
        for name in ('Part.qml','PreviewHover.qml','DetailPanel.qml'):
            p=temp/'qml'/name;p.write_text(p.read_text().replace('HoverHandler {','HoverHandler {enabled:false;'))
        p=temp/'qml/DetailPanel.qml';s=p.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1)
        s=s.replace('    id:popup', '''    id:popup
    TestEvent {id:testInput}
    property bool recordingFlags:false
    property var flagFrames:[]
    function findControl(name,node) {
        if(node.objectName===name)return node;
        for(const child of node.children || []){const match=findControl(name,child);if(match)return match;}
        return null;
    }
    function flagSnapshot() {
        const flag=findControl('mullvad-country-jp',body),pos=flag.mapToItem(body,0,0),origin=attachedPosition();
        const section=findControl('mullvad-status-surface',body),bottom=section.mapToItem(body,0,section.height);
        return {x:origin.x+pos.x,y:origin.y+pos.y,height:body.height,sectionBottom:origin.y+bottom.y};
    }
    function flagMotion(start) {
        recordingFlags=start;
        if(start)flagFrames=[flagSnapshot()];
        return {ok:true,frames:flagFrames};
    }
    FrameAnimation {running:popup.recordingFlags;onTriggered:popup.flagFrames=popup.flagFrames.concat([popup.flagSnapshot()])}
    function controlTest(name,inspectOnly) {
        function find(node){if(node.objectName===name)return node;for(const child of node.children || []){const match=find(child);if(match)return match;}return null;}
        const target=find(body);if(!target)throw Error('Control missing: '+name);
        const before={ok:true,enabled:target.enabled,visible:target.visible,selected:target.selected===true};
        if(!inspectOnly)testInput.mouseClick(target,target.width/2,target.height/2,Qt.LeftButton,Qt.NoModifier,0);
        return before;
    }''',1);p.write_text(s)
        p=temp/'qml/shell.qml';s=p.read_text().replace('id: root','id: root;property var testActions:[]',1)
        s=s.replace('function send(message) {if(backend.running)',"function send(message) {if(message.op==='panelAction'){testActions=testActions.concat([message.context]);return;}if(backend.running)")
        s=s.replace("case 'status':break;","case 'controlTest':return detailPanel.controlTest(req.name,req.inspectOnly);case 'flagMotion':return detailPanel.flagMotion(req.start===true);case 'testActions':return {ok:true,actions:testActions};case 'status':break;")
        s=s.replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}","if(msg.kind==='pointer'){}")
        p.write_text(s)
        cfg=json.loads((ROOT/'examples/comparison.json').read_text())
        for bar in cfg['bars']:bar.update(monitors=[1],autoHide=False)
        path=temp/'config.json';path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp/'qml'),BARTJIK_CONFIG=str(path))
        with (temp/'log').open('w') as log:
            process=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({'op':'status'}).get('ready'))
                for module,controls in [('vpn',[('mullvad-city-osa','city',{'country':'jp','city':'osa'}),('mullvad-country-nl','country','nl'),('mullvad-country-jp','country','jp'),('mullvad-country-sg','country','sg')]),('tailnet',[('tailscale-toggle','toggle',None),('tailscale-ping-laptop','ping','laptop'),('tailscale-copy-laptop','copy','laptop')]),('input',[('input-method-pinyin','select','pinyin')]),('battery',[('battery-profile-performance','profile','performance'),('battery-profile-power-saver','profile','power-saver')])]:
                    ipc({'op':'detailTest','module':module})
                    wait_for(lambda:ipc({'op':'detailState'})['panel']['opened'])
                    if module=='vpn':
                        def city_visible():
                            return ipc({'op':'controlTest','name':'mullvad-city-osa','inspectOnly':True})['visible']
                        def settled():
                            panel=ipc({'op':'detailState'})['panel']
                            return abs(panel['card']['height']-panel['height'])<.01
                        def toggle_cities(expanded):
                            ipc({'op':'flagMotion','start':True})
                            assert ipc({'op':'controlTest','name':'mullvad-toggle-cities'})['enabled']
                            wait_for(lambda:city_visible()==expanded and settled())
                            frames=ipc({'op':'flagMotion'})['frames']
                            assert len(frames)>2,frames
                            assert max(f['height'] for f in frames)-min(f['height'] for f in frames)>10,frames
                            for axis in ('x','y','sectionBottom'):
                                drift=max(f[axis] for f in frames)-min(f[axis] for f in frames)
                                assert drift<.5,('Mullvad footer or section edge jumped during city animation',expanded,axis,drift,frames)
                        wait_for(settled)
                        collapsed_height=ipc({'op':'detailState'})['panel']['height']
                        before=len(ipc({'op':'testActions'})['actions'])
                        assert not city_visible()
                        toggle_cities(True)
                        assert ipc({'op':'detailState'})['panel']['height']>collapsed_height
                        assert len(ipc({'op':'testActions'})['actions'])==before
                        toggle_cities(False)
                        assert ipc({'op':'detailState'})['panel']['height']==collapsed_height
                        ipc({'op':'controlTest','name':'mullvad-toggle-cities'})
                        wait_for(city_visible)
                        assert not ipc({'op':'controlTest','name':'mullvad-city-tyo'})['enabled']
                        assert len(ipc({'op':'testActions'})['actions'])==before
                        if len(sys.argv)>1:
                            image=Path(sys.argv[1]).resolve()/'mullvad-cities.png';image.parent.mkdir(parents=True,exist_ok=True)
                            ipc({'op':'captureDetail','path':str(image)})
                            wait_for(lambda:image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
                    for control,action,selection in controls:
                        before=len(ipc({'op':'testActions'})['actions'])
                        assert ipc({'op':'controlTest','name':control})['enabled']
                        actions=ipc({'op':'testActions'})['actions'];assert len(actions)==before+1,(control,actions)
                        last=actions[-1]
                        assert last['module']==module and last['action']==action and last.get('selection')==selection,last
                        assert last['monitor'] and last['barKey'] and last['item'] and 'anchor' in last,last
                        if action=='city':wait_for(lambda:not city_visible())
                    if module=='battery':
                        before=len(ipc({'op':'testActions'})['actions'])
                        ipc({'op':'controlTest','name':'battery-profile-balanced'})
                        assert len(ipc({'op':'testActions'})['actions'])==before
                        for profile in ('performance','balanced','power-saver'):
                            cfg['modules']['battery']['demoProfile']=profile
                            path.write_text(json.dumps(cfg))
                            def selected_profiles():
                                return [key for key in ('performance','balanced','power-saver')
                                        if ipc({'op':'controlTest','name':'battery-profile-'+key,'inspectOnly':True})['selected']]
                            wait_for(lambda:selected_profiles()==[profile])
                        if len(sys.argv)>1:
                            destination=Path(sys.argv[1]);destination.mkdir(parents=True,exist_ok=True)
                            for external in (False,True):
                                cfg['modules']['battery']['demoExternal']=external
                                path.write_text(json.dumps(cfg))
                                wait_for(lambda:next(m for m in ipc({'op':'modules'})['modules'] if m['provider']=='battery')['items'][0]['battery']['external']==external)
                                time.sleep(.4)
                                image=destination/('battery-charging.png' if external else 'battery-remaining.png')
                                ipc({'op':'captureDetail','path':str(image)})
                                wait_for(lambda:image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
                    if module=='tailnet':
                        before=len(ipc({'op':'testActions'})['actions'])
                        assert not ipc({'op':'controlTest','name':'tailscale-ping-phone'})['enabled']
                        assert len(ipc({'op':'testActions'})['actions'])==before
                    if module=='vpn':
                        ipc({'op':'controlTest','name':'mullvad-toggle-cities'})
                        wait_for(city_visible)
                    ipc({'op':'detailTest','module':module,'leave':True})
                    wait_for(lambda:not ipc({'op':'detailState'})['panel']['opened'])
                    if module=='vpn':
                        ipc({'op':'detailTest','module':module})
                        wait_for(lambda:ipc({'op':'detailState'})['panel']['opened'])
                        assert not city_visible()
                        ipc({'op':'detailTest','module':module,'leave':True})
                        wait_for(lambda:not ipc({'op':'detailState'})['panel']['opened'])
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in (temp/'log').read_text().splitlines() if any(x in line for x in ('WARN scene','ERROR','Traceback','Binding loop'))]
        assert not errors,errors[:10]
    print('Mullvad city expand/collapse with stationary flags and synchronized section edges across animation frames, exact city selection, current-city disabling, reopen reset, country shortcuts, Tailscale, input selection and exclusive battery power modes: real Qt clicks and originating context passed')


if __name__=='__main__':main()
