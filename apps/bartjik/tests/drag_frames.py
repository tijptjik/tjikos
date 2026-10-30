"""Hyprland compositor acceptance: fully opaque pickup/drop on one surface.

Run on a private bus after installing the documented bartjik-panel no_anim rule.
Only synthetic content is displayed; layer metadata is filtered to this test PID.
"""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import json,os,shutil,subprocess,sys,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT),str(ROOT/'tests')]
from bartjik import ipc
from render import wait_for
output=Path(sys.argv[1]);output.mkdir(parents=True,exist_ok=True)
assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
with tempfile.TemporaryDirectory(prefix='bartjik-drag-frames-') as td:
 temp=Path(td);shutil.copytree(ROOT/'qml',temp/'qml')
 for name in ('Part.qml','PreviewHover.qml','CryptoPanel.qml','DetailPanel.qml'):
  p=temp/'qml'/name;p.write_text(p.read_text().replace('HoverHandler {','HoverHandler {enabled:false;'))
 p=temp/'qml/DetachablePopup.qml';s=p.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1)
 s=s.replace('    id:host','''    id:host
    TestEvent {id:input}
    property var handle:null
    property bool testMoving:false
    function gesture(action){
        if(!handle){const all=[];function walk(i){if(!i.visible)return;all.push(i);i.children?.forEach(walk);}walk(panelBody);handle=all.find(i=>i.popup===host && i.symbol!==undefined);}
        if(action==='pressMove'){testMoving=true;input.mousePress(panelBody,8,30,Qt.LeftButton,Qt.NoModifier,0);}
        const target=testMoving?panelBody:handle,x=testMoving?8:16,y=testMoving?30:14;
        if(action==='press')input.mousePress(handle,16,14,Qt.LeftButton,Qt.NoModifier,0);
        if(action==='move')input.mouseMove(target,x-100,y-60,0,Qt.LeftButton,Qt.NoModifier);
        if(action==='release')input.mouseRelease(target,x-100,y-60,Qt.LeftButton,Qt.NoModifier,0);
        return {ok:true,detached:detached,dragging:dragging,preview:preview.visible,bodyOpacity:bodyOpacity,dropping:dropping};
    }''');p.write_text(s)
 p=temp/'qml/shell.qml';s=p.read_text().replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}","if(msg.kind==='pointer'){}")
 s=s.replace("case 'status':break;","case 'gesture':return cryptoPanel.gesture(req.action);\ncase 'status':break;");p.write_text(s)
 cfg=json.loads((ROOT/'examples/comparison.json').read_text())
 cfg['bars']=[dict(cfg['bars'][0],monitors=[1],autoHide=False,perMonitor={},islands=[{'id':'prices','modules':['prices']}])]
 (temp/'config.json').write_text(json.dumps(cfg));os.environ.update(BARTJIK_CONFIG=str(temp/'config.json'),BARTJIK_QML=str(temp/'qml'))
 with (output/'render.log').open('w') as log:
  proc=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
  try:
   wait_for(lambda:ipc({'op':'status'}).get('ready'))
   ipc({'op':'cryptoPreview','module':'prices','item':'SOL/USD'})
   wait_for(lambda:ipc({'op':'cryptoState'})['opened']);time.sleep(.6)
   def sample(stage):
    start=time.monotonic();rows=[]
    while time.monotonic()-start<.7:
     layers=json.loads(subprocess.check_output(['hyprctl','-j','layers'],text=True))
     mine=[dict(layer,screen=screen) for screen,groups in layers.items() for level in groups['levels'].values() for layer in level if layer['pid']==proc.pid and layer['namespace']=='bartjik-panel']
     rows.append({'ms':round((time.monotonic()-start)*1000,1),'layers':mine})
     time.sleep(.008)
    return rows
   ipc({'op':'gesture','action':'press'});ipc({'op':'gesture','action':'move'})
   start=sample('start');ipc({'op':'gesture','action':'release'});drop=sample('drop')
   ipc({'op':'gesture','action':'pressMove'});ipc({'op':'gesture','action':'move'})
   restart=sample('restart');ipc({'op':'gesture','action':'release'});redrop=sample('redrop')
   (output/'frames.json').write_text(json.dumps({'start':start,'drop':drop,'restart':restart,'redrop':redrop},indent=2)+'\n')
   addresses=[]
   for stage,rows in [('start',start),('drop',drop)]:
    entries=[l for r in rows for l in r['layers']]
    assert entries,('no drag surface',stage)
    # Hyprland lists a newly registered surface at alpha 0 before its first
    # commit. The source card is retained during that preparation. Any partial
    # opacity, or returning to zero after becoming visible, is a regression.
    alphas=[l['alpha'] for l in entries]
    assert 1 in alphas and all(a in (0,1) for a in alphas),(stage,'compositor fade detected')
    assert all(a==1 for a in alphas[alphas.index(1):]),(stage,'surface disappeared after mapping')
    addresses.append({l['address'] for l in entries})
    print(stage,{'samples':len(rows),'minAlpha':min((l['alpha'] for l in entries),default=None),'surfaces':sorted({l['namespace']+':'+l['address'] for l in entries})})
   assert len(addresses[0])==1 and addresses[0]==addresses[1],('surface replaced on drop',addresses)
   moved_addresses=[]
   for stage,rows in [('restart',restart),('redrop',redrop)]:
    entries=[l for r in rows for l in r['layers']]
    present={l['address'] for l in entries}
    for address in present:
     alphas=[l['alpha'] for l in entries if l['address']==address]
     assert 1 in alphas and all(a in (0,1) for a in alphas),(stage,address,'compositor fade detected')
     assert all(a==1 for a in alphas[alphas.index(1):]),(stage,address,'surface blinked')
    moved_addresses.append(present-addresses[0])
    print(stage,{'samples':len(rows),'surfaces':sorted(present)})
   assert len(moved_addresses[0])==1 and moved_addresses[0]==moved_addresses[1],('reposition surface replaced on drop',moved_addresses)
   final=ipc({'op':'gesture','action':'state'})
   assert final['detached'] and not final['preview'] and not final['dropping'],final
  finally:proc.terminate();proc.wait(timeout=5)
