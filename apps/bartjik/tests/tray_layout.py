"""Synthetic rendering: equal number-to-dot spacing, normal fonts, flat flag and dots."""

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

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from bartjik import ipc


def main():
    if not os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/'):
        raise RuntimeError('Private bus required')
    destination=Path(sys.argv[1]).resolve();destination.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-tray-layout-') as td:
        temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
        shell=temp/'qml/shell.qml';s=shell.read_text().replace("case 'status':break;",'''case 'layoutProbe': {
            const panel=panels.instances[0], out=[], dots=[];
            function visit(q) {
                if(q.objectName==='workspace-separator' && q.visible) {
                    const xy=q.mapToItem(panel.contentItem,0,0);dots.push({x:xy.x,width:q.width});
                }
                if(q.kind && q.moduleConfig && q.row) {
                    const xy=q.mapToItem(panel.contentItem,0,0);
                    function label(item) {if(item.objectName==='part-label')return item;for(const c of item.children || []){const l=label(c);if(l)return l;}return null;}
                    const text=label(q), center=text?text.mapToItem(panel.contentItem,text.width/2,text.height/2).x:null;
                    out.push({provider:q.moduleConfig.provider,part:q.kind,id:q.row.id,visible:q.visible,
                        labelCenter:center,labelWidth:text?.width || 0,x:xy.x,y:xy.y,width:q.width,height:q.height,font:q.style.font,fontSize:q.style.fontSize,
                        icon:q.row.icon?.value,unread:q.row.state?.urgent});
                }
                for(const child of q.children || [])visit(child);
            }
            visit(panel.contentItem);return {ok:true,parts:out,dots:dots};
        }
        case 'status':break;''');shell.write_text(s)
        cfg={'theme':'rose-punk','modules':{
            'spaces':{'provider':'workspaces','label':'name','appearance':'separators','style':{'itemGap':0}},
            'tray':{'provider':'icontray','style':{'iconSize':21},'iconOverrides':{'ChatGPT':'builtin:chatgpt-outline','TelegramDesktop':'builtin:telegram-outline','ZapZap':'builtin:whatsapp-outline'}},
            'vpn':{'provider':'mullvad','regionFlag':True},
            'time':{'provider':'datetime'},'quota':{'provider':'codex'},'credit':{'provider':'openrouter'}},
            'bars':[{'id':'layout','monitors':[1],'side':'t','position':'c','islands':[
                {'id':'spaces','modules':['spaces']},{'id':'tray','modules':['tray']},
                {'id':'vpn','modules':['vpn','time']},{'id':'ai','modules':['quota','credit']}]}]}
        (temp/'config.json').write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp/'qml'),BARTJIK_CONFIG=str(temp/'config.json'))
        with (destination/'layout.log').open('w') as log:
            p=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({'op':'status'}).get('ready'))
                probe=wait_for(lambda:ipc({'op':'layoutProbe'}));parts=probe['parts']
                spaces=[p for p in parts if p['provider']=='workspaces' and p['part']=='text' and p['visible']]
                assert [p['id'] for p in spaces]==['1','2','3','6','10','14'],spaces
                centers=[p['labelCenter'] for p in spaces]
                assert all(abs(p['labelCenter']-(p['x']+p['width']/2))<.01 for p in spaces),spaces
                steps=[round(b-a,4) for a,b in zip(centers,centers[1:])]
                dots=probe['dots'];assert len(dots)==len(spaces)-1,dots
                for left,dot,right in zip(spaces,dots,spaces[1:]):
                    before=dot['x']-(left['labelCenter']+left['labelWidth']/2)
                    after=right['labelCenter']-right['labelWidth']/2-dot['x']-dot['width']
                    assert abs(before-6)<.01 and abs(after-6)<.01,(left['id'],right['id'],before,after)
                    assert dot['width']==4,dot
                assert spaces[4]['width']>spaces[3]['width'],spaces
                clock=next(p for p in parts if p['provider']=='datetime' and p['part']=='text')
                assert all((p['font'],p['fontSize'])==(clock['font'],clock['fontSize']) for p in spaces)
                flag=next(p for p in parts if p['provider']=='mullvad' and p['part']=='icon')
                assert flag['icon'].endswith('/flags/jp.svg'),flag
                time.sleep(.5)
                bar=ipc({'op':'bars'})['bars'][0]
                shot=destination/'layout.png';shot.unlink(missing_ok=True);ipc({'op':'capture','bar':bar['key'],'path':str(shot)})
                wait_for(lambda:shot.exists() and shot.read_bytes().endswith(b'IEND\xaeB`\x82'))
                (destination/'geometry.json').write_text(json.dumps({'centers':centers,'steps':steps,'parts':parts},indent=2))
            finally:
                p.terminate();p.wait(timeout=5)
        errors=[l for l in (destination/'layout.log').read_text().splitlines() if any(e in l for e in ('WARN scene','ERROR','Traceback'))]
        assert not errors,errors
    print('Equal number-to-dot spacing including 10/14, inherited typography and flat flag rendering passed:',destination/'layout.png')


if __name__=='__main__':main()
