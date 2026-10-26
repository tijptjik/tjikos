"""Real pointer events for moving dropped cards without swallowing controls.

Run on a private bus. All audio, network and media providers are synthetic.
"""
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


GESTURES = '''    id:host
    TestEvent {id:moveInput}
    property Item testTarget:null
    property point testPoint:Qt.point(0,0)
    function moveTest(req){
        const all=[];
        function walk(item){if(!item.visible)return;all.push(item);item.children?.forEach(walk);}
        walk(panelBody);
        const handle=all.find(i=>i.popup===host && i.symbol!==undefined && i.text!==undefined);
        if(req.action==='press' || req.action==='click' || req.action==='wheel' || req.action==='quick'){
            if(req.target==='handle')testTarget=handle;
            else if(req.mediaAction)testTarget=all.find(i=>i instanceof Controls.AbstractButton && i.modelData?.action===req.mediaAction);
            else if(req.target==='lyrics' || req.target==='shuffle')testTarget=all.find(i=>i instanceof Controls.AbstractButton && i.contentItem?.text===req.target);
            else if(req.target==='mute')testTarget=all.find(i=>i instanceof Controls.AbstractButton && /^(Unmute|Mute) Output$/.test(i.text));
            else if(req.target==='slider')testTarget=all.find(i=>i instanceof Controls.Slider);
            else if(req.target==='scrollbar')testTarget=all.find(i=>i instanceof Controls.ScrollBar && i.size<1);
            else if(req.target==='password')testTarget=all.find(i=>i instanceof TextInput);
            else if(req.target==='chart')testTarget=all.find(i=>i instanceof Canvas);
            else if(req.target==='wifi')testTarget=all.find(i=>i.modelData?.ssid==='Guest Wi-Fi' && i.clicked!==undefined);
            else if(req.text)testTarget=all.find(i=>i.text===req.text);
            else testTarget=panelBody;
            if(!testTarget)throw Error('Missing target '+JSON.stringify(req));
            testPoint=Qt.point(req.x ?? testTarget.width/2,req.y ?? testTarget.height/2);
        }
        const p=Qt.point(testPoint.x+(req.dx || 0),testPoint.y+(req.dy || 0));
        if(req.action==='press' || req.action==='quick')moveInput.mousePress(testTarget,testPoint.x,testPoint.y,Qt.LeftButton,Qt.NoModifier,0);
        if(req.action==='move' || req.action==='quick')moveInput.mouseMove(testTarget,p.x,p.y,0,Qt.LeftButton,Qt.NoModifier);
        if(req.action==='release' || req.action==='quick')moveInput.mouseRelease(testTarget,p.x,p.y,Qt.LeftButton,Qt.NoModifier,0);
        if(req.action==='click')moveInput.mouseClick(testTarget,p.x,p.y,Qt.LeftButton,Qt.NoModifier,0);
        if(req.action==='middle')moveInput.mouseClick(panelBody,100,100,Qt.MiddleButton,Qt.NoModifier,0);
        if(req.action==='wheel')moveInput.mouseWheel(testTarget,p.x,p.y,Qt.NoButton,Qt.NoModifier,0,req.delta,0);
        if(req.action==='cancel')cancelDrag();
        return {ok:true,detached:detached,dragging:dragging,dropping:dropping,preview:preview.visible,
            previewReady:previewReady,opened:panelOpen,symbol:handle?.symbol || '',placement:placement(),
            origin:attachedPosition(),width:panelBody.width,height:panelBody.height,
            point:testTarget?testTarget.mapToItem(panelBody,testPoint):null,
            targetWidth:testTarget?.width || 0,targetHeight:testTarget?.height || 0};
    }
'''

NETWORK = '''    id: root
    function moveFixture(req){
        if(req.count!==undefined){
            const networks=state.networks.filter(n=>n.active);
            for(let i=0;i<req.count;++i)networks.push({ssid:i?'Demo network '+(i+1):'Guest Wi-Fi',
                bssid:'02:00:00:00:01:'+String(i).padStart(2,'0'),interface:'wlan-demo',signal:78-i,security:'WPA2',active:false});
            state=Object.assign({},state,{networks:networks});
        }
        return {contentY:wifiScroll.contentItem.contentY,scrollbar:wifiBar.visible && wifiBar.size<1,
            point:wifiScroll.mapToItem(popup.panelBody,wifiScroll.width/2,wifiScroll.height/2),
            passwordFocus:password.activeFocus,selected:!!selected};
    }
'''


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-reposition-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        for name in ('Part.qml', 'PreviewHover.qml', 'CryptoPanel.qml', 'DetailPanel.qml', 'WorkspacePreview.qml'):
            p = temp / 'qml' / name
            p.write_text(p.read_text().replace('HoverHandler {', 'HoverHandler {enabled:false;'))
        p = temp / 'qml/DetachablePopup.qml'
        p.write_text(p.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1).replace('    id:host', GESTURES, 1))
        p = temp / 'qml/NetworkDetails.qml'
        p.write_text(p.read_text().replace('    id: root', NETWORK, 1))
        p = temp / 'qml/DetailPanel.qml'
        p.write_text(p.read_text().replace('    id:popup', '''    id:popup
    function moveControls(req){return {ok:true,wifi:networkDetails.moveFixture(req),
        muted:audioDetails.demoNodes[0].audio.muted,volume:audioDetails.demoNodes[0].audio.volume};}
    function moveMedia(req){return mediaDetails.moveMedia(req);}
''', 1))
        p = temp / 'qml/Lyrics.qml'
        p.write_text(p.read_text().replace('    id:lyrics', '    id:lyrics\n    function moveScroll(){return scroll.contentY;}', 1))
        p = temp / 'qml/MediaDetails.qml'
        p.write_text(p.read_text().replace('    id:details', '''    id:details
    function moveMedia(req){
        if(req.longLyrics)popup.controller.lyricsReceived(lyrics.token,{status:'ready',text:Array.from({length:50},(_,i)=>'Synthetic lyric line '+i).join('\\n')});
        return {ok:true,shown:lyricsShown,status:lyrics.status,scroll:lyrics.moveScroll()};
    }
''', 1))
        p = temp / 'qml/shell.qml'
        source = p.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        source = source.replace('    id: root', '    id: root\n    property var moveActions:[]', 1)
        source = source.replace('if(demo)return;', 'if(demo){moveActions=moveActions.concat([action]);return;}')
        source = source.replace("case 'status':break;", '''case 'moveTest':return (req.panel==='crypto'?cryptoPanel:req.panel==='workspace'?workspacePreview.item:detailPanel).moveTest(req);
        case 'moveMedia':return Object.assign({actions:moveActions},detailPanel.moveMedia(req));
        case 'moveHeld':return {ok:true,held:menuOpen(bars[0].key)};
        case 'moveControls':return detailPanel.moveControls(req);
        case 'closePanels':cryptoPanel.closePanel();detailPanel.close();workspacePreview.item?.close();return {ok:true};
        case 'testScreens':return {ok:true,screens:Quickshell.screens.map(s=>({name:s.name,x:s.x,y:s.y,width:s.width,height:s.height}))};
        case 'status':break;''')
        p.write_text(source)
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        modules = ['prices', 'sound', 'net', 'time', 'music', 'spaces']
        cfg['bars'] = [dict(cfg['bars'][0], monitors=[1], autoHide=False, perMonitor={}, side='t', position='c', alignment='c',
                            islands=[{'id':'cards','modules':modules}])]
        for module in modules:
            cfg['modules'][module]['panel'] = {'hover':True,'delay':30,'closeDelay':80}
        path = temp / 'config.json'
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_CONFIG=str(path), BARTJIK_QML=str(temp / 'qml'))
        log_path = output / 'reposition.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc({'op':'status'}).get('ready'))
                screens = ipc({'op':'testScreens'})['screens']

                def gesture(panel, action='state', **kw):
                    result = ipc(dict(op='moveTest',panel=panel,action=action,**kw))
                    assert result.get('ok'), result
                    return result

                def controls(**kw):
                    return ipc(dict(op='moveControls',**kw))

                def show(panel, module):
                    ipc({'op':'closePanels'})
                    if panel == 'crypto':
                        ipc({'op':'cryptoHoverTest','module':'prices','item':'SOL/USD','x':600})
                    elif panel == 'workspace':
                        ipc({'op':'workspacePreviewTest','module':'spaces','item':'1','x':600})
                    else:
                        ipc({'op':'detailTest','module':module,'x':600})
                    wait_for(lambda: gesture(panel)['opened'] and gesture(panel)['width'] > 100)

                def settle(panel):
                    wait_for(lambda: gesture(panel)['detached'] and not gesture(panel)['preview'] and not gesture(panel)['dropping'])
                    return gesture(panel)

                def drag(panel, dx=60, dy=50, interference=None, **target):
                    before = gesture(panel)
                    gesture(panel, 'press', **target)
                    gesture(panel, 'move', dx=dx, dy=dy)
                    wait_for(lambda: gesture(panel)['previewReady'])
                    if interference:
                        interference()
                        time.sleep(.35)
                        assert gesture(panel)['dragging'] and gesture(panel)['opened']
                    gesture(panel, 'release', dx=dx, dy=dy)
                    after = settle(panel)
                    screen = next(s for s in screens if s['name'] == before['placement']['monitor'])
                    for axis, delta, extent, size in (('x', dx, screen['width'], before['width']), ('y', dy, screen['height'], before['height'])):
                        start = before['placement'][axis] if before['detached'] else before['origin'][axis]
                        expected = max(12, min(start + delta, extent - size - 12))
                        assert abs(after['placement'][axis] - expected) < 2, (target, before, after, expected)
                    assert after['symbol'] == 'close' and after['opened'], after
                    return after

                def cross_screens(panel):
                    for screen in screens:
                        before = gesture(panel)
                        source = next(s for s in screens if s['name'] == before['placement']['monitor'])
                        press = gesture(panel, 'press', x=100, y=100)
                        dx = screen['x'] + screen['width']/2 - source['x'] - before['placement']['x'] - press['point']['x']
                        dy = screen['y'] + screen['height']/2 - source['y'] - before['placement']['y'] - press['point']['y']
                        gesture(panel, 'move', dx=dx, dy=dy)
                        wait_for(lambda: gesture(panel)['previewReady'])
                        gesture(panel, 'release', dx=dx, dy=dy)
                        assert settle(panel)['placement']['monitor'] == screen['name']

                results = []
                for panel, module in (('crypto','prices'), ('detail','sound'), ('detail','net'), ('detail','time')):
                    show(panel, module)
                    # Attached cards detach directly from non-control content.
                    assert gesture(panel)['symbol'] == ''
                    drag(panel, x=8, y=30)
                    drag(panel, x=100, y=30, dx=40, dy=30)
                    if panel == 'crypto':
                        drag(panel, target='chart', dx=-50, dy=20)
                        drag(panel, text='1 hour', dx=25, dy=25)
                    if module == 'sound':
                        drag(panel, text='USB Audio Speakers', dx=25, dy=25)
                    placed = gesture(panel)['placement']
                    gesture(panel, 'click', x=8, y=30)
                    gesture(panel, 'press', x=8, y=30)
                    gesture(panel, 'move', dx=3, dy=2)
                    gesture(panel, 'release', dx=3, dy=2)
                    assert gesture(panel)['placement'] == placed
                    gesture(panel, 'press', x=8, y=30)
                    gesture(panel, 'move', dx=80, dy=40)
                    wait_for(lambda: gesture(panel)['previewReady'])
                    gesture(panel, 'cancel')
                    gesture(panel, 'release', dx=80, dy=40)
                    assert gesture(panel)['detached'] and gesture(panel)['placement'] == placed
                    gesture(panel, 'quick', x=8, y=30, dx=-20, dy=-20)
                    settle(panel)
                    assert not gesture(panel, 'click', target='handle')['opened']
                    results.append([module, 'repeated movement, threshold, cancellation, quick release, close'])

                # Reposition a previously dropped card across every connected output.
                for panel, module in (('crypto','prices'), ('detail','time')):
                    show(panel, module)
                    drag(panel, target='handle')
                    cross_screens(panel)
                    results.append([panel, 'detached cross-monitor movement'])

                show('detail', 'sound')
                drag('detail', target='handle')
                placed = gesture('detail')['placement']
                gesture('detail', 'click', text='Mute Output')
                assert controls()['muted']
                gesture('detail', 'press', target='mute')
                gesture('detail', 'move', dx=-80, dy=40)
                gesture('detail', 'release', dx=-80, dy=40)
                assert gesture('detail')['placement'] == placed and not gesture('detail')['dragging']
                slider = gesture('detail', 'press', target='slider')
                gesture('detail', 'move', dx=slider['targetWidth']*.25)
                gesture('detail', 'release', dx=slider['targetWidth']*.25)
                assert .65 < controls()['volume'] < .85, controls()
                assert gesture('detail')['placement'] == placed
                results.append(['audio', 'button click, button drag, slider drag'])

                show('detail', 'net')
                controls(count=9)
                wait_for(lambda: controls()['wifi']['scrollbar'])
                drag('detail', target='handle')
                placed = gesture('detail')['placement']
                before = controls()['wifi']['contentY']
                gesture('detail', 'wheel', target='wifi', delta=-360)
                wait_for(lambda: controls()['wifi']['contentY'] > before)
                wheel_position = controls()['wifi']['contentY']
                bar = gesture('detail', 'press', target='scrollbar', x=3, y=10)
                gesture('detail', 'move', dy=bar['targetHeight']*.7)
                gesture('detail', 'release', dy=bar['targetHeight']*.7)
                assert not gesture('detail')['dragging'] and gesture('detail')['placement'] == placed
                assert controls()['wifi']['contentY'] > wheel_position, controls()
                # Scroll to top, then interact with a network row and password field.
                point = controls()['wifi']['point']
                gesture('detail', 'wheel', **point, delta=3000)
                wait_for(lambda: abs(controls()['wifi']['contentY']) < 1)
                gesture('detail', 'click', target='wifi')
                wait_for(lambda: controls()['wifi']['selected'])
                gesture('detail', 'click', target='password')
                wait_for(lambda: controls()['wifi']['passwordFocus'])
                gesture('detail', 'press', target='password')
                gesture('detail', 'move', dx=70)
                gesture('detail', 'release', dx=70)
                assert not gesture('detail')['dragging'] and gesture('detail')['placement'] == placed
                results.append(['network', 'wheel, scrollbar, network button, password drag'])

                def media(**kw):
                    return ipc(dict(op='moveMedia', **kw))

                for view in ('artwork', 'lyrics'):
                    show('detail', 'music')
                    assert gesture('detail')['symbol'] == ''
                    for action in ('player.previous', 'player.toggle', 'player.next'):
                        gesture('detail', 'click', mediaAction=action)
                        assert media()['actions'][-1] == action
                    gesture('detail', 'click', target='shuffle')
                    assert media()['actions'][-1] == 'player.shuffle'
                    gesture('detail', 'middle')
                    assert media()['actions'][-1] == 'player.toggle'
                    gesture('detail', 'press', mediaAction='player.toggle')
                    gesture('detail', 'move', dx=60, dy=-50)
                    gesture('detail', 'release', dx=60, dy=-50)
                    assert not gesture('detail')['detached']
                    if view == 'lyrics':
                        gesture('detail', 'click', target='lyrics')
                        wait_for(lambda: media()['shown'] and media()['status'] == 'ready')
                        media(longLyrics=True)
                        gesture('detail', 'wheel', x=150, y=100, delta=-240)
                        wait_for(lambda: media()['scroll'] > 0)
                        gesture('detail', 'press', target='scrollbar', y=10)
                        gesture('detail', 'move', dy=80)
                        gesture('detail', 'release', dy=80)
                        assert not gesture('detail')['detached']
                    gesture('detail', 'press', x=100, y=100)
                    gesture('detail', 'move', dx=3, dy=2)
                    assert not gesture('detail', 'release', dx=3, dy=2)['detached']
                    drag('detail', x=100, y=100, interference=lambda: ipc({'op':'workspacePreviewTest','module':'spaces','item':'1','x':600}))
                    drag('detail', x=120, y=130, dx=-30, dy=25)
                    cross_screens('detail')
                    placed = gesture('detail')['placement']
                    gesture('detail', 'click', mediaAction='player.toggle')
                    assert media()['actions'][-1] == 'player.toggle'
                    if view == 'lyrics':
                        assert media()['shown'] and media()['status'] == 'ready'
                    ipc({'op':'detailTest','module':'music','leave':True})
                    ipc({'op':'hide','bar':'all'})
                    time.sleep(.3)
                    assert gesture('detail')['opened'] and gesture('detail')['placement'] == placed
                    assert not ipc({'op':'moveHeld'})['held']
                    ipc({'op':'show','bar':'all'})
                    assert not gesture('detail', 'click', target='handle')['opened']
                    results.append(['music '+view, 'background detach, controls, repeated/cross-screen movement, close'])

                show('workspace', 'spaces')
                assert gesture('workspace')['symbol'] == ''
                original = ipc({'op':'workspacePreviewState'})['preview']
                drag('workspace', x=100, y=100, interference=lambda: ipc({'op':'detailTest','module':'sound','x':600}))
                drag('workspace', x=140, y=120)
                cross_screens('workspace')
                ipc({'op':'workspacePreviewTest','module':'spaces','item':'1','x':600,'leave':True})
                ipc({'op':'workspacePreviewTest','module':'spaces','item':'2','x':800})
                ipc({'op':'hide','bar':'all'})
                time.sleep(.3)
                final = ipc({'op':'workspacePreviewState'})['preview']
                assert final['opened'] and final['key'] == original['key'] and final['windows'] == 2, final
                assert final['sourceMonitor'] == original['sourceMonitor']
                assert not ipc({'op':'moveHeld'})['held']
                ipc({'op':'show','bar':'all'})
                assert not gesture('workspace', 'click', target='handle')['opened']
                assert ipc({'op':'workspacePreviewState'})['preview']['windows'] == 0
                show('workspace', 'spaces')
                assert not gesture('workspace')['detached'] and gesture('workspace')['symbol'] == ''
                results.append(['workspace', 'background detach, source retained, cross-screen movement, pinned, close/reset'])
                (output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines() if any(k in line for k in ('WARN scene', 'ERROR', 'Traceback', 'Binding loop'))]
        assert not errors, errors[:20]
    print('Dropped card movement, controls and scrolling; artwork, lyrics and workspace background detachment passed')


if __name__ == '__main__':
    main()
