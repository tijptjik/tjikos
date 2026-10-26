"""Synthetic shared-height cards and pointer travel corridor on a private bus."""
import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from render import wait_for
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
    destination=Path(sys.argv[1]).resolve();destination.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-panels-') as td:
        temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
        for name in ('PreviewHover.qml','Part.qml','CryptoPanel.qml','WorkspacePreview.qml','DetailPanel.qml','MediaDetails.qml'):
            p=temp/'qml'/name;p.write_text(p.read_text().replace('HoverHandler {','HoverHandler {enabled:false;'))
        p=temp/'qml/WorkspacePreview.qml';p.write_text(p.read_text().replace('readonly property var sourceList: !opened','property bool testEmpty: false\n    readonly property var sourceList: testEmpty || !opened'))
        p=temp/'qml/shell.qml';s=p.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}","if(msg.kind==='pointer'){}")
        s=s.replace("case 'status':break;",'''case 'panelPointer':root.pointer=req.pointer;root.pointerUpdated=Date.now();return {ok:true};
        case 'panelTest': {
            const p=panels.instances.find(p=>p.bar.moduleConfigs[req.module]);
            const m=p.bar.moduleConfigs[req.module],row=items(p.bar,req.module)[req.index || 0];
            const region={x:p.geometry.x,y:p.geometry.y,width:p.geometry.width,height:p.geometry.height};
            const c={barKey:p.barKey,module:req.module,item:row.id,monitor:p.bar.screen,part:'item',
                anchor:{x:region.x+region.width/2,y:region.y,width:0,height:0},hoverRegion:region};
            if(m.provider==='crypto'){if(req.leave)cryptoLeave(c);else cryptoEnter(c,p,row,m.panel);}
            else if(m.provider==='workspaces'){if(req.leave)workspaceLeave(c);else workspaceEnter(c,p,row,m.preview);}
            else {if(req.leave)detailLeave(c);else detailEnter(c,p,row,m.panel);}
            return {ok:true,region:region,screen:p.bar.screen};
        }
        case 'panelLoading':cryptoPanel.chart={loading:true};return {ok:true};
        case 'panelEmpty':workspacePreview.item.testEmpty=true;return {ok:true};
        case 'chartCacheTest': {
            const p=panels.instances.find(p=>p.bar.moduleConfigs.prices),r=items(p.bar,'prices')[0];
            const c={barKey:p.barKey,module:'prices',item:r.id,monitor:p.bar.screen,event:'hoverEnter',anchor:{x:200,y:p.geometry.y,width:0,height:0}};
            cryptoPanel.closePanel();const fetch=cryptoPanel.open(c,p,r);
            return {ok:true,fetch:fetch,points:cryptoPanel.points.length};
        }
        case 'status':break;''');p.write_text(s)
        cfg=json.loads((ROOT/'examples/comparison.json').read_text())
        cfg['bars']=cfg['bars'][:1];cfg['bars'][0].update(monitors=[1],autoHide=False)
        path=temp/'config.json';path.write_text(json.dumps(cfg));os.environ.update(BARTJIK_QML=str(temp/'qml'),BARTJIK_CONFIG=str(path))
        with (destination/'panels.log').open('w') as log:
            process=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({'op':'status'}).get('ready'))
                heights=[]
                for module,state_op,capture in [('prices','cryptoState','captureCrypto'),('spaces','workspacePreviewState','captureWorkspace'),('music','detailState','captureDetail')]:
                    def state():
                        s=ipc({'op':state_op});return s.get('preview',s.get('panel',s))
                    source=ipc({'op':'panelTest','module':module});region=source['region']
                    wait_for(lambda:state()['opened'])
                    assert state()['height']==360,state();heights.append(state()['height'])
                    if module=='spaces':
                        assert abs(state()['desktopHeight']-340)<1,state()
                        assert abs(state()['width']-state()['desktopWidth']-20)<1,state()
                    # Drop Qt hover, as happens between Wayland surfaces, while the
                    # compositor still sees the cursor in the 8px travel corridor.
                    x=region['x']+region['width']/2
                    ipc({'op':'panelPointer','pointer':{'valid':True,'monitor':source['screen'],'x':x,'y':region['y']-4}})
                    ipc({'op':'panelTest','module':module,'leave':True})
                    time.sleep(.5);assert state()['opened'],(module,'corridor',state())
                    ipc({'op':'panelPointer','pointer':{'valid':True,'monitor':source['screen'],'x':x,'y':region['y']-100}})
                    time.sleep(.35);assert state()['opened'],(module,'card',state())
                    if module=='music':wait_for(lambda:state()['artworkReady'])
                    shot=destination/(module+'.png');ipc({'op':capture,'path':str(shot)})
                    wait_for(lambda:shot.exists() and shot.read_bytes().endswith(b'IEND\xaeB`\x82'))
                    ipc({'op':'panelPointer','pointer':{'valid':True,'monitor':source['screen'],'x':0,'y':0}})
                    wait_for(lambda:not state()['opened'])
                assert len(set(heights))==1,heights
                # Reopening must retain the graph immediately and skip a second lookup.
                cached=ipc({'op':'chartCacheTest'})
                assert not cached['fetch'] and cached['points']>0,cached
                ipc({'op':'panelLoading'})
                time.sleep(.2)
                ipc({'op':'captureCrypto','path':str(destination/'loading.png')})
                ipc({'op':'panelTest','module':'spaces'})
                wait_for(lambda:ipc({'op':'workspacePreviewState'})['preview']['opened'])
                ipc({'op':'panelEmpty'})
                ipc({'op':'captureWorkspace','path':str(destination/'empty.png')})
                wait_for(lambda:(destination/'empty.png').exists())
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in (destination/'panels.log').read_text().splitlines() if any(k in line for k in ('WARN scene','ERROR','Traceback','Binding loop'))]
        assert not errors,errors
    print('All preview heights equal; source/card travel survives lost hover and closes on departure')


if __name__=='__main__':main()
