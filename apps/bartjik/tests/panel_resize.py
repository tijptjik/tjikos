"""Synthetic animation checks for panel viewports and bottom controls."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from render import wait_for

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    results = []
    with tempfile.TemporaryDirectory(prefix='bartjik-resize-') as td:
        temp = Path(td)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        for name in ('Part.qml', 'PreviewHover.qml', 'DetailPanel.qml', 'CryptoPanel.qml', 'WorkspacePreview.qml'):
            p = temp / 'qml' / name
            p.write_text(p.read_text().replace('HoverHandler {', 'HoverHandler {enabled:false;'))
        p = temp / 'qml/MediaDetails.qml'
        p.write_text(p.read_text().replace('id:footer;', "id:footer;objectName:'audit-media-footer';"))
        p = temp / 'qml/DetailPanel.qml'
        s = p.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1)
        s = s.replace('    id:popup', '''    id:popup
    TestEvent {id:auditInput}
    function auditFind(predicate,node) {
        if(predicate(node))return node;
        for(const child of node.children || []){const match=auditFind(predicate,child);if(match)return match;}
        return null;
    }
    function auditGeometry(){
        const origin=attachedPosition();
        const contents=audioPanel?audioDetails:networkPanel?networkDetails:codexPanel?codexDetails:openrouterPanel?openrouterDetails:statusDetails;
        let viewport=contents.parent;while(viewport && viewport.contentY===undefined)viewport=viewport.parent;
        const footer=mediaPanel?auditFind(n=>n.objectName==='audit-media-footer',body):audioPanel?auditFind(n=>n.text==='Headphones · switch to Screen',body):networkPanel?auditFind(n=>n.text==='Connect Ethernet',body):null;
        return {provider:provider,height:body.height,target:implicitHeight,liveHeight:liveContent.height,
            viewportBottom:mediaPanel?null:origin.y+viewport.mapToItem(body,0,viewport.height).y,
            footerBottom:footer?origin.y+footer.mapToItem(body,0,footer.height).y:null,
            scrollY:mediaPanel?0:viewport.contentY,scrollMaximum:mediaPanel?0:Math.max(0,viewport.contentHeight-viewport.height),
            edge:origin.y+body.height,crossfade:contentProgress};
    }
    function auditAction(req){
        if(req.action==='clients'){
            const nodes=audioDetails.demoNodes.filter(n=>!n.isStream);
            for(let i=0;i<req.count;i++)nodes.push({ready:true,name:'demo-client-'+i,description:'Client '+i,isStream:true,isSink:true,properties:{'application.name':'Client '+i},audio:{volume:.7,muted:false}});
            audioDetails.demoNodes=nodes;
        }else if(req.action==='scroll'){
            const contents=codexPanel?codexDetails:statusDetails;
            let viewport=contents.parent;while(viewport && viewport.contentY===undefined)viewport=viewport.parent;
            viewport.contentY=Math.max(0,viewport.contentHeight-viewport.height);
        }else if(req.action==='click'){
            const button=auditFind(n=>(n.text===req.name || n.modelData?.ssid===req.name) && n.clicked!==undefined,body);
            if(!button)throw Error('Missing button: '+req.name);
            auditInput.mouseClick(button,button.width/2,button.height/2,Qt.LeftButton,Qt.NoModifier,0);
        }
    }''', 1)
        p.write_text(s)
        p = temp / 'qml/CryptoPanel.qml'
        s = p.read_text().replace('    id: popup', '''    id: popup
    function auditGeometry(){
        const pos=attachedPosition(),plot=graph.parent;
        return {provider:'crypto',height:body.height,target:implicitHeight,edge:pos.y+body.height,
            plotBottom:pos.y+plot.mapToItem(body,0,plot.height).y};
    }''', 1)
        p.write_text(s)
        p = temp / 'qml/WorkspacePreview.qml'
        s = p.read_text().replace('    id:popup', '''    id:popup
    function auditGeometry(){return {provider:'workspace',height:body.height,target:implicitHeight,
        desktopTop:desktop.y,desktopBottom:desktop.y+desktop.height};}''', 1)
        p.write_text(s)
        p = temp / 'qml/shell.qml'
        s = p.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        s = s.replace('    id: root', '''    id: root
    property bool auditRunning:false
    property var auditSamples:[]
    property var auditBase:({})
    function auditSnapshot(){return Object.assign({progress:panelShell.contentProgress},panelShell.activePanel?.auditGeometry() || {});}
    FrameAnimation {running:root.auditRunning;onTriggered:root.auditSamples.push(root.auditSnapshot())}''', 1)
        s = s.replace("case 'status':break;", '''case 'auditState':return {ok:true,sample:auditSnapshot(),progress:panelShell.contentProgress};
        case 'auditSamples':
            auditRunning=req.start===true;
            if(auditRunning)auditSamples=[auditSnapshot()];
            return {ok:true,samples:auditSamples};
        case 'auditAction':detailPanel.auditAction(req);return {ok:true};
        case 'auditRows': {
            const source=detailPanel.bar.sources[detailPanel.selected.context.module];
            const providers=JSON.parse(JSON.stringify(state.providers));
            const item=providers[source].items[0];
            if(!auditBase[source])auditBase[source]=JSON.parse(JSON.stringify(item.panel));
            const base=auditBase[source],field=detailPanel.provider==='tailscale'?'peers':'rows';
            const rows=base[field];
            if(detailPanel.provider==='codex'){
                const accounts=rows.filter(r=>r.group==='account' || typeof r.progress==='number');
                item.panel.rows=Array.from({length:req.count},(_,i)=>Object.assign({},accounts[i%accounts.length],{id:'audit-'+i,label:'Account '+i})).concat(rows.filter(r=>r.group!=='account' && typeof r.progress!=='number'));
            }else item.panel[field]=Array.from({length:req.count},(_,i)=>Object.assign({},rows[i%rows.length],{id:'audit-'+i,label:'Entry '+i,name:'Device '+i}));
            state=Object.assign({},state,{providers:providers});
            return {ok:true};
        }
        case 'auditOpen': {
            const p=panels.instances[0],m=p.bar.moduleConfigs[req.module],r=items(p.bar,req.module)[0];
            const c={barKey:p.barKey,module:req.module,item:r.id,monitor:p.bar.screen,anchor:{x:700,y:p.geometry.y,width:0,height:0}};
            if(m.provider==='crypto')cryptoEnter(c,p,r,m.panel);
            else if(m.provider==='workspaces')workspaceEnter(c,p,r,m.preview);
            else detailEnter(c,p,r,m.panel);
            return {ok:true};
        }
        case 'status':break;''')
        p.write_text(s)
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        modules = ['sound', 'net', 'time', 'tailnet', 'input', 'quota', 'credit', 'vpn', 'music', 'prices', 'spaces']
        cfg['bars'] = [dict(cfg['bars'][0], monitors=[1], autoHide=False, perMonitor={}, islands=[{'id':'audit','modules':modules}])]
        for module in modules:
            cfg['modules'][module]['panel'] = {'hover':True,'delay':300,'closeDelay':160}
        path = temp / 'config.json'
        path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp / 'qml'), BARTJIK_CONFIG=str(path))
        with (destination / 'resize.log').open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc({'op':'status'}).get('ready'))
                def settled(provider=None):
                    s = ipc({'op':'auditState'})
                    f = s['sample']
                    return 'provider' in f and (provider is None or f['provider'] == provider) and abs(f['height'] - f['target']) < .1 and f.get('crossfade', 1) == 1 and s['progress'] == 1
                def open_panel(module, provider):
                    ipc({'op':'auditOpen','module':module})
                    wait_for(lambda: settled(provider))
                def exercise(name, request, provider, footer=False):
                    ipc({'op':'auditSamples','start':True})
                    ipc(request)
                    wait_for(lambda: settled(provider))
                    frames = [f for f in ipc({'op':'auditSamples'})['samples'] if f.get('provider') == provider and f.get('progress',1)>0]
                    failures=[]
                    if len(frames)<3 or max(f['height'] for f in frames)-min(f['height'] for f in frames)<5:
                        failures.append('Did not capture a resize')
                    if provider not in ('crypto','workspace'):
                        error=max(abs(f['liveHeight']-f['height']) for f in frames)
                        if error>.5:failures.append('Content height leads card by %.1fpx' % error)
                        if provider!='playerctl':
                            error=max(abs(f['viewportBottom']-(f['edge']-16)) for f in frames)
                            if error>.5:failures.append('Viewport edge mismatch %.1fpx' % error)
                            error=max(max(0,-f['scrollY'],f['scrollY']-f['scrollMaximum']) for f in frames)
                            if error>1:failures.append('Scroll position outside content by %.1fpx' % error)
                    if footer:
                        drift=max(f['footerBottom'] for f in frames)-min(f['footerBottom'] for f in frames)
                        if drift>1:failures.append('Bottom control moves %.1fpx' % drift)
                    if provider=='crypto':
                        drift=max(f['plotBottom'] for f in frames)-min(f['plotBottom'] for f in frames)
                        if drift>.5:failures.append('Chart bottom moves %.1fpx' % drift)
                    if provider=='workspace':
                        if any(f['desktopTop']<9 or f['desktopBottom']>f['height']-9 for f in frames):failures.append('Preview leaves card bounds')
                    result={'case':name,'failures':failures,'samples':frames};results.append(result)
                    (destination / 'samples.json').write_text(json.dumps(results,indent=2))
                    print(name+': '+('; '.join(failures) or 'passed'),flush=True)
                open_panel('sound','audio')
                for count in (3,1,0,2):exercise('Audio clients '+str(count),{'op':'auditAction','action':'clients','count':count},'audio',True)
                open_panel('net','network')
                for name in ('Guest Wi-Fi','Cancel connection'):
                    exercise('Networking '+name,{'op':'auditAction','action':'click','name':name},'network',True)
                for module,provider in [('time','datetime'),('tailnet','tailscale'),('input','locale'),('quota','codex'),('credit','openrouter')]:
                    open_panel(module,provider)
                    for count in (8,1):exercise(provider+' entries '+str(count),{'op':'auditRows','count':count},provider)
                open_panel('quota','codex')
                exercise('Codex long list',{'op':'auditRows','count':20},'codex')
                ipc({'op':'auditAction','action':'scroll'})
                exercise('Codex shrink after scrolling',{'op':'auditRows','count':1},'codex')
                open_panel('net','network')
                exercise('Tall card to media',{'op':'auditOpen','module':'music'},'playerctl',True)
                open_panel('net','network')
                exercise('Tall card to crypto',{'op':'auditOpen','module':'prices'},'crypto')
                open_panel('net','network')
                exercise('Tall card to workspace',{'op':'auditOpen','module':'spaces'},'workspace')
                for module,provider in [('music','playerctl'),('prices','crypto'),('spaces','workspace')]:
                    open_panel('input','locale')
                    exercise('Small card to '+provider,{'op':'auditOpen','module':module},provider,provider=='playerctl')
                (destination / 'samples.json').write_text(json.dumps(results,indent=2))
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in (destination/'resize.log').read_text().splitlines() if any(k in line for k in ('WARN scene','ERROR','Traceback','Binding loop'))]
        assert not errors,errors[:15]
    failures=[r['case'] for r in results if r['failures']]
    assert not failures,failures
    print('Panel resize and content geometry checks passed')


if __name__ == '__main__':
    main()
