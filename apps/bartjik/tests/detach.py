"""Real Qt pointer gestures for detachable cards; synthetic data on a private bus."""
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
    with tempfile.TemporaryDirectory(prefix='bartjik-detach-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        for name in ('Part.qml', 'PreviewHover.qml', 'CryptoPanel.qml', 'DetailPanel.qml'):
            p = temp / 'qml' / name
            p.write_text(p.read_text().replace('HoverHandler {', 'HoverHandler {enabled:false;'))
        p = temp / 'qml/DetachablePopup.qml'
        s = p.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        s = s.replace('    id:host', '''    id:host
    TestEvent {id:testInput}
    property var testHandle:null
    function gesture(req){
        if(!testHandle || req.action==='state' || req.action==='press' || req.action==='click' || req.action==='quickDrop'){
            const all=[];function walk(item){if(!item.visible)return;all.push(item);item.children?.forEach(walk);}walk(panelBody);
            testHandle=all.find(i=>i.popup===host && i.symbol!==undefined && i.text!==undefined);
            if(!testHandle)testHandle=panelBody;
            if(!testHandle)throw Error('handle missing');
        }
        const modifiers=req.super?Qt.MetaModifier:Qt.NoModifier;
        const h=req.body?panelBody:testHandle,originX=req.x ?? (h===panelBody?8:h.width/2),originY=req.y ?? (h===panelBody?30:h.height/2);
        const x=originX+(req.dx || 0),y=originY+(req.dy || 0);
        if(req.action==='quickDrop'){
            testInput.mousePress(h,originX,originY,Qt.LeftButton,modifiers,0);
            testInput.mouseMove(h,x,y,0,Qt.LeftButton,modifiers);
            testInput.mouseRelease(h,x,y,Qt.LeftButton,modifiers,0);
        }
        if(req.action==='press')testInput.mousePress(h,x,y,Qt.LeftButton,modifiers,0);
        if(req.action==='move')testInput.mouseMove(h,x,y,0,Qt.LeftButton,modifiers);
        if(req.action==='release')testInput.mouseRelease(h,x,y,Qt.LeftButton,modifiers,0);
        if(req.action==='click')testInput.mouseClick(h,x,y,Qt.LeftButton,modifiers,0);
        if(req.action==='cancel')cancelDrag();
        return {ok:true,address:nativeDetach?nativeWindow.address:"",placed:nativeDetach?nativeWindow.placed:false,symbol:testHandle.symbol || "",detached:detached,dragging:dragging,opened:panelOpen,
            preview:preview.visible,previewReady:previewReady,bodyOpacity:bodyOpacity,dropFrames:dropFrames,sourceFrames:sourceFrames,dropping:dropping,placement:placement(),origin:attachedPosition(),
            width:panelBody.width,height:panelBody.height,screen:panelScreen?.name};
    }''')
        p.write_text(s)
        p = temp / 'qml/shell.qml'
        s = p.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        s = s.replace('cryptoPanel.capture(req.path)', '(testedCrypto || cryptoPanel).capture(req.path)')
        s = s.replace('    id: root', '    id: root\n    property var testedCrypto:null')
        s = s.replace("case 'status':break;", '''case 'gesture':return (req.panel==='crypto'?(testedCrypto || cryptoPanel):req.panel==='workspace'?workspacePreview.item:detailPanel).gesture(req);
        case 'selectCrypto':testedCrypto=cryptoPanel;return {ok:true};
        case 'cryptoPool':return {ok:true,cards:cryptoPanels.map(p=>({pair:p.pair,opened:p.opened,detached:p.detached,token:p.token,points:p.points.length}))};
        case 'closePanels':cryptoPanels.forEach(p=>p.closePanel());testedCrypto=null;detailPanel.close();workspacePreview.item?.close();return {ok:true};
        case 'testScreens':return {ok:true,screens:Quickshell.screens.map(s=>({name:s.name,x:s.x,y:s.y,width:s.width,height:s.height}))};
        case 'held':return {ok:true,held:!!menuOpen(bars[0].key)};
        case 'status':break;''')
        p.write_text(s)
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        cfg['bars'] = [dict(cfg['bars'][0], monitors=[1], autoHide=False, perMonitor={}, position='c', alignment='c',
                            islands=[{'id':'cards','modules':['prices','sound','net','time']}])]
        # All providers are synthetic; no source hover is allowed to interfere.
        for module in ('prices', 'sound', 'net', 'time'):
            cfg['modules'][module]['panel'] = {'hover':True,'delay':30,'closeDelay':80}
        path = temp / 'config.json'
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_CONFIG=str(path), BARTJIK_QML=str(temp / 'qml'))
        log_path = destination / 'detach.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc({'op':'status'}).get('ready'))
                screens = ipc({'op':'testScreens'})['screens']
                def gesture(panel, action='state', **kw):
                    return ipc(dict(op='gesture',panel=panel,action=action,**kw))
                def show(panel, module):
                    ipc({'op':'closePanels'})
                    if panel == 'crypto':
                        ipc({'op':'cryptoHoverTest','module':'prices','item':'SOL/USD','x':600})
                        wait_for(lambda: ipc({'op':'cryptoState'})['opened'])
                        ipc({'op':'selectCrypto'})
                    elif panel == 'workspace':
                        ipc({'op':'workspacePreviewTest','module':'spaces','item':'2','x':600})
                        wait_for(lambda: ipc({'op':'workspacePreviewState'})['preview']['opened'])
                    else:
                        ipc({'op':'detailTest','module':module,'x':600})
                        wait_for(lambda: ipc({'op':'detailState'})['panel']['opened'])
                    wait_for(lambda: gesture(panel)['width'] > 100)
                def capture(panel, name):
                    image = destination / (name + '.png')
                    ipc({'op':'captureCrypto' if panel=='crypto' else 'captureDetail','path':str(image)})
                    wait_for(lambda: image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
                results=[]
                for side in ('b','t','l','r'):
                    cfg['bars'][0]['side'] = side
                    path.write_text(json.dumps(cfg));ipc({'op':'reload'});time.sleep(.3)
                    for panel,module in (('crypto','prices'),('detail','sound'),('detail','net'),('detail','time')):
                        show(panel,module)
                        initial=gesture(panel,'click')
                        assert initial['symbol']=='' and initial['opened'] and not initial['detached'],initial
                        gesture(panel,'press');gesture(panel,'move',dx=3,dy=2)
                        small=gesture(panel,'release',dx=3,dy=2)
                        assert not small['detached'] and small['symbol']=='',small
                        if side=='b':capture(panel,module+'-handle')
                        gesture(panel,'press')
                        active=gesture(panel,'move',dx=-100,dy=-60)
                        wait_for(lambda:gesture(panel)['previewReady'])
                        assert active['dragging'] and not active['detached'] and active['symbol']=='',active
                        # Leaving the source while dragging must not close or switch cards.
                        if panel=='crypto':
                            ipc({'op':'cryptoHoverTest','module':'prices','item':'SOL/USD','leave':True})
                        else:ipc({'op':'detailTest','module':module,'leave':True})
                        time.sleep(.2);assert gesture(panel)['opened']
                        gesture(panel,'release',dx=-100,dy=-60)
                        wait_for(lambda:gesture(panel)['detached'])
                        dropped=gesture(panel)
                        assert dropped['detached'] and dropped['opened'] and dropped['symbol']=='close',dropped
                        screen=next(s for s in screens if s['name']==initial['screen'])
                        for axis,delta,extent,size in (('x',-100,screen['width'],initial['width']),('y',-60,screen['height'],initial['height'])):
                            expected=max(12,min(initial['origin'][axis]+delta,extent-size-12))
                            assert abs(dropped['placement'][axis]-expected)<2,(initial,dropped,expected)
                        wait_for(lambda:not gesture(panel)['preview'])
                        settled=gesture(panel)
                        assert settled['dropFrames']>=2 and settled['sourceFrames']>=2,settled
                        assert not ipc({'op':'held'})['held']
                        client=next(c for c in json.loads(subprocess.check_output(['hyprctl','-j','clients'],text=True)) if c['address']==gesture(panel)['address'])
                        assert client['floating'] and not client['pinned'],client
                        expected=[screen['x']+dropped['placement']['x'],screen['y']+dropped['placement']['y']]
                        assert all(abs(a-b)<2 for a,b in zip(client['at'],expected)),(client,expected)
                        location=gesture(panel)['placement']
                        if panel=='crypto':ipc({'op':'cryptoHoverTest','module':'prices','item':'BTC/USD','x':900})
                        else:ipc({'op':'detailTest','module':'time','x':900})
                        time.sleep(.2)
                        assert gesture(panel)['placement']==location
                        if panel=='crypto':
                            fresh=ipc({'op':'cryptoState'})
                            assert fresh['opened'] and not fresh['detached'] and fresh['pair']=='BTC/USD',fresh
                            cards=ipc({'op':'cryptoPool'})['cards']
                            old=next(c for c in cards if c['opened'] and c['detached'])
                            assert old['pair']=='SOL/USD' and old['token']!=next(c['token'] for c in cards if c['opened'] and not c['detached']),cards
                        ipc({'op':'hide','bar':'all'});time.sleep(.1)
                        assert gesture(panel)['opened']
                        ipc({'op':'show','bar':'all'})
                        if side=='b':capture(panel,module+'-dropped')
                        if panel=='crypto':
                            ipc({'op':'cryptoPreview','module':'prices','item':'BTC/USD'})
                            wait_for(lambda:ipc({'op':'cryptoState'})['opened'])
                        closed=gesture(panel,'click')
                        assert not closed['opened'] and not closed['detached'],closed
                        if panel=='crypto':
                            assert ipc({'op':'cryptoState'})['opened']
                            assert len(ipc({'op':'cryptoPool'})['cards'])==2
                        results.append((side,module,'drag/drop/close'))
                # Move a card to every available output using the same original mouse grab.
                for screen in screens[1:]:
                    for panel,module in (('crypto','prices'),('detail','time')):
                        show(panel,module)
                        state=gesture(panel)
                        origin=state['origin'];source=next(s for s in screens if s['name']==state['screen'])
                        dx=screen['x']+screen['width']/2-(source['x']+origin['x']+8)
                        dy=screen['y']+screen['height']/2-(source['y']+origin['y']+30)
                        gesture(panel,'press');gesture(panel,'move',dx=dx,dy=dy)
                        wait_for(lambda:gesture(panel)['previewReady'])
                        gesture(panel,'release',dx=dx,dy=dy)
                        wait_for(lambda:gesture(panel)['detached'])
                        state=gesture(panel)
                        wait_for(lambda:not gesture(panel)['preview'])
                        assert state['screen']==screen['name'],state
                        assert gesture(panel,'click')['opened'] is False
                        results.append((screen['name'],panel,'cross-monitor'))
                # Release before the asynchronous snapshot/preview is ready.
                for panel,module in (('crypto','prices'),('detail','time')):
                    show(panel,module);quick=gesture(panel,'quickDrop',dx=50)
                    assert not quick['previewReady'] and quick['bodyOpacity']==1,quick
                    wait_for(lambda:gesture(panel)['detached'] and not gesture(panel)['preview'])
                    assert gesture(panel,'click')['opened'] is False
                # Explicit cancellation and closing/reopening both restore a drag handle.
                show('crypto','prices');gesture('crypto','press');gesture('crypto','move',dx=50)
                gesture('crypto','cancel');gesture('crypto','release',dx=50)
                assert not gesture('crypto')['detached']
                show('crypto','prices');assert gesture('crypto')['symbol']==''
                # The attached card can be picked up at the header, chart or
                # border without using its top-right handle.
                for x,y in ((60,30),(180,190),(5,100)):
                    show('crypto','prices')
                    gesture('crypto','quickDrop',body=True,x=x,y=y,dx=70,dy=-50)
                    wait_for(lambda:gesture('crypto')['placed'] and not gesture('crypto')['preview'])
                    assert gesture('crypto')['detached']
                    assert gesture('crypto','click')['opened'] is False
                results.append(('crypto','body drag','header/chart/border'))
                show('crypto','prices')
                # Native geometry/workspace ownership and compositor close, with
                # two independent crypto windows alive at once.
                gesture('crypto','quickDrop',dx=80,dy=-80)
                wait_for(lambda:gesture('crypto')['placed'] and not gesture('crypto')['preview'])
                first=gesture('crypto')['address']
                def client(address):
                    return next((c for c in json.loads(subprocess.check_output(['hyprctl','-j','clients'],text=True)) if c['address']==address),None)
                def dispatch(code):
                    assert subprocess.check_output(['hyprctl','eval',code],text=True).strip()=='ok'
                workspace='bartjik-acceptance-'+str(os.getpid())
                dispatch('hl.dispatch(hl.dsp.window.move({workspace='+json.dumps('name:'+workspace)+',follow=false,window="address:'+first+'"}))')
                wait_for(lambda:client(first)['workspace']['name']==workspace)
                ipc({'op':'cryptoPreview','module':'prices','item':'BTC/USD'})
                ipc({'op':'selectCrypto'})
                gesture('crypto','quickDrop',dx=-80,dy=-100)
                wait_for(lambda:gesture('crypto')['placed'] and not gesture('crypto')['preview'])
                second=gesture('crypto')['address']
                assert first!=second and client(first)['workspace']['name']==workspace
                dispatch('hl.dispatch(hl.dsp.window.move({x=150,y=160,relative=false,window="address:'+second+'"}))')
                wait_for(lambda:client(second)['at']==[150,160])
                ipc({'op':'cryptoPreview','module':'prices','item':'SOL/USD'})
                assert client(second)['at']==[150,160] and client(first)['workspace']['name']==workspace
                dispatch('hl.dispatch(hl.dsp.window.close({window="address:'+first+'"}))')
                wait_for(lambda:client(first) is None)
                assert client(second) and ipc({'op':'cryptoState'})['opened']
                assert sum(c['opened'] for c in ipc({'op':'cryptoPool'})['cards'])==2
                results.append(('crypto','native windows','placement/workspace/move/multiple/close'))
                for panel,module in (('detail','sound'),('detail','net'),('detail','time'),('workspace','spaces')):
                    if panel=='workspace':
                        cfg['bars'][0]['islands'][0]['modules'].append('spaces')
                        path.write_text(json.dumps(cfg));ipc({'op':'reload'});time.sleep(.3)
                    show(panel,module)
                    gesture(panel,'quickDrop',dx=80,dy=-80,**({'body':True,'x':60,'y':30} if panel=='workspace' else {}))
                    wait_for(lambda:gesture(panel)['placed'] and not gesture(panel)['preview'])
                    address=gesture(panel)['address']
                    assert client(address)['floating'] and not client(address)['pinned']
                    dispatch('hl.dispatch(hl.dsp.window.move({workspace='+json.dumps('name:'+workspace)+',follow=false,window="address:'+address+'"}))')
                    wait_for(lambda:client(address)['workspace']['name']==workspace)
                    ipc({'op':'detailTest','module':'time','x':900})
                    time.sleep(.2)
                    assert client(address)['workspace']['name']==workspace
                    dispatch('hl.dispatch(hl.dsp.window.close({window="address:'+address+'"}))')
                    wait_for(lambda:not gesture(panel)['opened'])
                    assert client(address) is None
                    results.append((module,'native window','workspace/persistence/close'))

                cfg['bars'][0]['islands'][0]['modules'].append('music')
                path.write_text(json.dumps(cfg));ipc({'op':'reload'});time.sleep(.3)
                for module in ('sound','net','time','music'):
                    show('detail',module)
                    gesture('detail','quickDrop',body=True,x=100,y=90,dx=80,dy=-80,**{'super':True})
                    wait_for(lambda:gesture('detail')['placed'] and not gesture('detail')['preview'])
                    assert gesture('detail')['detached'],module
                    results.append((module,'Super body drag','detached'))

                (destination/'results.json').write_text(json.dumps(results,indent=2)+'\n')
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in log_path.read_text().splitlines() if any(k in line for k in ('WARN scene','ERROR','Traceback','Binding loop','window placement','error:'))]
        assert not errors,errors[:20]
    print('Drag threshold, four bar sides, crypto/audio/network/clock, pinned hover, auto-hide, cross-monitor drop, close and reset passed')


if __name__=='__main__':
    main()
