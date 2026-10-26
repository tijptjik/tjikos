"""Native network/audio placement, travel and controls using synthetic data only."""
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
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-native-panels-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        for name in ('Part.qml', 'PreviewHover.qml', 'DetailPanel.qml'):
            p = temp / 'qml' / name
            p.write_text(p.read_text().replace('HoverHandler {', 'HoverHandler {enabled:false;'))
        p = temp / 'qml/NetworkDetails.qml'
        source = p.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        source = source.replace('    id: root', '''    id: root
    TestEvent {id:wifiInput}
    function testWifi(req) {
        if(req.count!==undefined){
            const networks=state.networks.filter(n=>n.active);
            for(let i=0;i<req.count;++i)networks.push({ssid:i?'Demo network '+(i+1):'Guest Wi-Fi',
                bssid:'02:00:00:00:01:'+String(i).padStart(2,'0'),interface:'wlan-demo',signal:78-i,security:'WPA2',active:false});
            state=Object.assign({},state,{networks:networks});
        }
        if(req.wheel)wifiInput.mouseWheel(wifiScroll.contentItem,wifiScroll.width/2,wifiScroll.height/2,Qt.NoButton,Qt.NoModifier,0,req.wheel,0);
        const rows=[];
        for(let i=0;i<networkRows.count;++i){
            const row=networkRows.itemAt(i);
            rows.push({ssid:row.modelData.ssid,y:row.y,height:row.height});
        }
        return {height:wifiScroll.height,contentHeight:wifiScroll.contentHeight,
            contentY:wifiScroll.contentItem.contentY,scrollbar:wifiBar.visible && wifiBar.size<1,rows:rows};
    }''')
        p.write_text(source)
        p = temp / 'qml/DetailPanel.qml'
        source = p.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        source = source.replace('    id:popup', '''    id:popup
    TestEvent {id:controlsInput}
    function testWifi(req) {return networkDetails.testWifi(req);}
    function testHeader() {
        const icon=header.children.find(i=>i.source!==undefined && i.status!==undefined);
        return {source:header.imageSource,ready:!!icon && icon.visible && icon.status===Image.Ready};
    }
    function testControl(name) {
        const target=audioPanel?audioDetails:networkDetails;
        const all=[];function walk(item){all.push(item);if(item.children)item.children.forEach(walk);};walk(target);
        if(name==='volume'){
            const slider=all.find(i=>i.from===0 && i.to===1 && i.moved!==undefined);
            if(!slider)throw Error('missing volume slider');
            controlsInput.mouseClick(slider,slider.width*.75,slider.height/2,Qt.LeftButton,Qt.NoModifier,0);
        } else {
            const button=all.find(i=>i.text===name && i.clicked!==undefined)
                || all.find(i=>i.modelData?.ssid===name && i.clicked!==undefined);
            if(!button)throw Error('missing button '+name);
            controlsInput.mouseClick(button,button.width/2,button.height/2,Qt.LeftButton,Qt.NoModifier,0);
        }
        return {ok:true,muted:audioDetails.demoNodes[0].audio.muted,volume:audioDetails.demoNodes[0].audio.volume};
    }''')
        p.write_text(source)
        p = temp / 'qml/shell.qml'
        source = p.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        source = source.replace("case 'status':break;", '''case 'panelPointer':root.pointer=req.pointer;root.pointerUpdated=Date.now()+60000;return {ok:true};
        case 'panelClose':detailPanel.close();return {ok:true};
        case 'panelControl':return detailPanel.testControl(req.name);
        case 'panelWifi':return {ok:true,wifi:detailPanel.testWifi(req)};
        case 'panelHeader':return {ok:true,header:detailPanel.testHeader()};
        case 'panelTest': {
            const p=panels.instances.find(p=>p.bar.screen===req.monitor);
            const m=p.bar.moduleConfigs[req.module],r=items(p.bar,req.module)[0],g=p.geometry;
            const c={barKey:p.barKey,module:req.module,item:r.id,monitor:p.bar.screen,
                anchor:{x:req.x ?? g.x+g.width/2,y:req.y ?? g.y+g.height/2,width:0,height:0},hoverRegion:g};
            if(req.leave)detailLeave(c);else if(req.click)act(m.provider+'.panel',c,p,r);else detailEnter(c,p,r,m.panel);
            return {ok:true,region:g,screen:p.bar.screen};
        }
        case 'status':break;''')
        p.write_text(source)
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        cfg['bars'] = [dict(cfg['bars'][0], monitors='all', autoHide=False, perMonitor={}, position='c', alignment='c',
                            islands=[{'id':'controls','modules':['net','sound']}])]
        for module in ('net','sound'):
            cfg['modules'][module]['panel']={'hover':True,'delay':300,'closeDelay':160}
        path = temp / 'config.json'
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_CONFIG=str(path), BARTJIK_QML=str(temp / 'qml'))
        with (destination / 'native-panels.log').open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'),'--demo'],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({'op':'status'}).get('ready'))
                screens=ipc({'op':'monitors'})['monitors']
                def state():return ipc({'op':'detailState'})['panel']
                def pointer(screen,x,y):ipc({'op':'panelPointer','pointer':dict(valid=True,monitor=screen['name'],x=x,y=y)})
                def close():
                    ipc({'op':'panelClose'})
                    wait_for(lambda:not state()['opened'])
                def hover(screen,module,**kwargs):return ipc(dict(op='panelTest',module=module,monitor=screen['name'],**kwargs))
                def wifi(**kwargs):return ipc(dict(op='panelWifi',**kwargs))['wifi']
                wifi(count=7)
                for screen in screens:
                    for module,provider in [('net','network'),('sound','audio')]:
                        close()
                        # Brief entry cancels; continuous pointer movement preserves dwell.
                        hover(screen,module)
                        time.sleep(.05)
                        assert not state()['opened']
                        hover(screen,module,leave=True)
                        time.sleep(.35)
                        assert not state()['opened']
                        source=hover(screen,module);g=source['region']
                        wait_for(lambda:state()['opened'])
                        assert state()['provider']==provider and state()['monitor']==screen['name']
                        icon=ipc({'op':'panelHeader'})['header']
                        assert icon['ready'] and icon['source']=='icons/proicons/'+('wi-fi' if provider=='network' else 'volume')+'.svg',icon
                        if provider=='network':
                            listing=wifi();rows=listing['rows']
                            assert len(rows)==7 and listing['scrollbar'],listing
                            assert abs(listing['height']-rows[2]['y']-rows[2]['height'])<1,listing
                            assert rows[3]['y']>=listing['height'],listing
                        x=g['x']+g['width']/2
                        hover(screen,module,x=x+20)
                        wait_for(lambda:abs(state()['anchorX']+state()['width']/2-(x+20))<1)
                        # Cursor crossing the 8px gap must not dismiss the popup.
                        pointer(screen,x,g['y']-4)
                        hover(screen,module,leave=True)
                        time.sleep(.4);assert state()['opened'],state()
                        pointer(screen,x,g['y']-100)
                        time.sleep(.3);assert state()['opened'],state()
                        assert not state()['audio']['meters'] and not state()['network']['traffic']
                        image=destination/(provider+'-'+screen['name']+'.png')
                        ipc({'op':'captureDetail','path':str(image)})
                        wait_for(lambda:image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
                        pointer(screen,0,0)
                        wait_for(lambda:not state()['opened'])
                # A visible popup follows a new monitor without losing its pending target.
                if len(screens)>1:
                    hover(screens[0],'net');wait_for(lambda:state()['opened'])
                    hover(screens[1],'net');wait_for(lambda:state()['opened'] and state()['monitor']==screens[1]['name'])
                close()
                screen=screens[0]
                hover(screen,'net',click=True);wait_for(lambda:state()['opened'])
                wifi(wheel=-2400)
                wait_for(lambda:wifi()['contentY']>0)
                wait_for(lambda:abs(wifi()['contentY']+wifi()['height']-wifi()['contentHeight'])<1)
                scrolled=wifi();last=scrolled['rows'][-1]
                assert last['y']>=scrolled['contentY'] and last['y']+last['height']<=scrolled['contentY']+scrolled['height']+1,scrolled
                image=destination/'network-scrolled.png'
                ipc({'op':'captureDetail','path':str(image)})
                wait_for(lambda:image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
                ipc({'op':'panelControl','name':'Demo network 7'})
                wait_for(lambda:state()['network']['selected'])
                ipc({'op':'panelControl','name':'Cancel connection'})
                # Shrinking/refilling a scan must size to the actual rows and retain the cap.
                for count in (1,0,3,7):
                    wifi(count=count)
                    wait_for(lambda:len(wifi()['rows'])==count)
                    listing=wifi()
                    if count:
                        row=listing['rows'][min(count,3)-1]
                        wait_for(lambda:abs(wifi()['height']-row['y']-row['height'])<1)
                    else:
                        assert listing['height']>0,listing
                    wait_for(lambda:wifi()['scrollbar']==(count>3))
                wifi(wheel=2400)
                wait_for(lambda:wifi()['contentY']==0)
                close()
                hover(screen,'sound',click=True)
                wait_for(lambda:state()['opened'] and state()['pinned'])
                assert ipc({'op':'panelControl','name':'Mute Output'})['muted']
                assert ipc({'op':'panelControl','name':'volume'})['volume']>.5
                ipc({'op':'panelControl','name':'Headphones · switch to Screen'})
                assert state()['audio']['lastAction']=={'action':'toggle-output'}
                close()
                hover(screen,'net',click=True);wait_for(lambda:state()['opened'])
                ipc({'op':'panelControl','name':'Scan for networks'})
                assert state()['network']['lastAction']['action']=='scan'
                ipc({'op':'panelControl','name':'Guest Wi-Fi'})
                wait_for(lambda:state()['network']['selected'])
                ipc({'op':'panelControl','name':'Connect'})
                wait_for(lambda:state()['network']['lastAction']['action']=='connect')
                assert not state()['network']['selected']
                # Side-aware native positioning for all four edges, with no external IPC.
                for side in ('t','r','b','l'):
                    close()
                    cfg['bars'][0].update(side=side,offsetX=0,offsetY=0)
                    path.write_text(json.dumps(cfg));ipc({'op':'reload'})
                    wait_for(lambda:ipc({'op':'config'})['bars'][0]['side']==side)
                    for module in ('sound','net'):
                        close();g=hover(screen,module)['region'];wait_for(lambda:state()['opened'])
                        s=state()
                        if side in 'tb':
                            assert abs(s['anchorX']+s['width']/2-g['x']-g['width']/2)<1,s
                            assert abs(s['anchorY']-(g['y']-8 if side=='b' else g['y']+g['height']+8))<1,s
                        else:
                            assert abs(s['anchorY']+s['height']/2-g['y']-g['height']/2)<1,s
                            assert abs(s['anchorX']-(g['x']-8 if side=='r' else g['x']+g['width']+8))<1,s
                ipc({'op':'hide','bar':'all'});wait_for(lambda:not state()['opened'])
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in (destination/'native-panels.log').read_text().splitlines() if any(k in line for k in ('WARN scene','ERROR','Traceback','Binding loop'))]
        assert not errors,errors[:15]
    print('Native panels: title icons, three-row Wi-Fi cap, scrolling and scan resizing, monitor switching, all sides, dwell cancellation, pointer following, corridor retention, dismissal, pinning and real control clicks passed.')


if __name__=='__main__':main()
