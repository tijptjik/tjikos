"""Optimistic queue rollback, stale refreshes, subtask arrow navigation and themed controls."""
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-optimistic-ui-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text('[tasktjik]\nauthority=""\ndirectory=' + json.dumps(str(root / 'tasks')) + '\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'target/release/tasktjik-api', home / 'tasktjik-api')
env = {**os.environ, 'TASKTJIK_HOME': str(home), 'TASKTJIK_VIEW': 'all',
       'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software', 'QT_QUICK_CONTROLS_STYLE': 'Basic',
       'XDG_RUNTIME_DIR': str(root / 'runtime'), 'XDG_CONFIG_HOME': str(root / 'config'),
       'XDG_DATA_HOME': str(root / 'data'), 'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

def request(value):
    return json.loads(subprocess.check_output([str(home / 'tasktjik-api'), 'request'], input=json.dumps(value), text=True, env=env))

def step(id, kind='task', **extra):
    return dict(id=id, title=id.capitalize(), kind=kind, due='', done=False, items=[], **extra)

today = datetime.date.today()
next_month = (today.replace(day=28) + datetime.timedelta(days=4)).replace(day=1)
month_end = next_month - datetime.timedelta(days=1)
project = request({'action':'create','fields':{'title':'Synthetic wedding','kind':'project','group':'Socials'}})
flow = step('agreement','flow');flow['due'] = str(month_end)
choice = step('agreed');choice['items'] = [step('branch')]
flow['items'] = [choice,step('declined','exit'),step('followup','reminder',offset='one week before')]
sent = step('instruction');sent['due'] = str(month_end)
template = request({'action':'create','fields':{'title':'Synthetic workflow','kind':'template','workflow':True,'subtasks':[sent,flow,step('received')]}})
member = request({'action':'create','template_id':template['id'],'fields':{'title':'Synthetic guest','project':project['id'],'group':'Guestbook','due':str(month_end)}})
ids = {r['title'].lower():r['id'] for r in member['subtasks']}
ids.update({r['title'].lower():r['id'] for r in member['subtasks'][1]['items']})
branch = member['subtasks'][1]['items'][0]['items'][0]['id']
request({'action':'create','fields':{'title':'Within month','due':str(month_end)}})
request({'action':'create','fields':{'title':'Next month','due':str(next_month)}})
request({'action':'create','fields':{'title':'Undated'}})
blocked = request({'action':'create','fields':{'title':'Blocked task'}})
request({'action':'status','id':blocked['id'],'revision':blocked['revision'],'status':'blocked','blockers':[{'reason':'Waiting'}],'actor':'fixture'})
plain = request({'action':'create','fields':{'title':'Zeta fixture','group':'Guestbook','project':project['id']}})
dependent=request({'action':'create','template_id':template['id'],'fields':{'title':'Another fixture','group':'Guestbook','project':project['id']}})
dep_sent=dependent['subtasks'][0]['id'];dep_agree=dependent['subtasks'][1]['items'][0]['id']
# A private transport shim delays only completion requests; fixtures never touch live data.
real = home / 'tasktjik-real'
(home / 'tasktjik-api').rename(real)
(home / 'tasktjik-api').write_text("""#!/usr/bin/env python3
import json, pathlib, subprocess, sys, time
root=pathlib.Path(__file__).parent
payload=sys.stdin.read()
action=json.loads(payload).get('action','list')
if action in ['toggle','subtask-toggle','status']:
    deadline=time.monotonic()+10
    while (root/'hold').exists() and time.monotonic()<deadline: time.sleep(.02)
    if (root/'fail').exists():
        (root/'fail').unlink();print('Synthetic completion rejected',file=sys.stderr);sys.exit(1)
p=subprocess.run([str(root/'tasktjik-real'),*sys.argv[1:]],input=payload,text=True)
sys.exit(p.returncode)
""")
(home / 'tasktjik-api').chmod(0o755)
shell = home / 'qml/shell.qml'
shell.write_text(shell.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n').replace('    Component.onCompleted:', '''    property var staleState:null
    TestCase {id:pointer;name:"OptimisticInput";when:false}
    IpcHandler {
        target:"optimistic-test"
        function state():string {return JSON.stringify({project:view.projectId,selected:view.selectedId,subtask:view.selectedSubtaskId,tasks:view.stateData.tasks,projected:view.listState.tasks,queue:view.completionQueue,error:view.errorMessage,message:view.completionMessage,rows:view.visualRows(),busy:view.requestBusy});}
        function keymod(code:int,modifiers:int):void {pointer.keyClick(code,modifiers);}
        function subfocus(id:string):string {const row=pointer.findChild(view,'subtask-'+id),chevron=pointer.findChild(view,'subtask-chevron-'+id);return JSON.stringify({selected:row.selected,chevron:chevron.visible,queue:view.completionQueue.length});}
        function select(id:string):void {view.selectedId=id;view.focusList();}
        function key(code:int):void {pointer.keyClick(code);}
        function open(id:string):void {view.enterProject(view.stateData.tasks.find(t=>t.id===id));}
        function expand(id:string):void {view.toggleExpanded(id);}
        function toggle(id:string,subtask:string):void {view.toggleCompletion(id,subtask);}
        function button(name:string):void {const button=pointer.findChild(view,name);pointer.mouseClick(button,button.width/2,button.height/2);}
        function complete(id:string):void {const row=pointer.findChild(view,'task-'+id);pointer.mouseMove(row,10,10);pointer.wait(160);const button=pointer.findChild(view,'complete-'+id);pointer.mouseClick(button,button.width/2,button.height/2);}
        function snapshot():void {root.staleState=JSON.parse(JSON.stringify(view.stateData));}
        function stale():void {root.applyState(root.staleState);}
        function geometry(id:string):string {const box=pointer.findChild(view,'completion-marker-'+id),date=pointer.findChild(view,'task-date-'+id),title=pointer.findChild(view,'task-title-'+id);const p=box.mapToItem(view,box.width/2,box.height/2),d=date.mapToItem(view,date.width/2,date.height/2),column=box.parent.mapToItem(view,box.parent.width/2,0),t=title.mapToItem(view,0,title.firstLineHeight/2);return JSON.stringify({width:box.width-4,centerX:p.x,columnCenter:column.x,dateCenter:d.x,dateCentered:date.horizontalAlignment===Text.AlignHCenter,center:p.y,titleCenter:t.y});}
        function hoverAdd(id:string):void {view.focusRow(view.visualRows().find(r=>r.id===id));pointer.wait(200);const card=pointer.findChild(view,'task-'+id),add=pointer.findChild(card,'subtask-add');pointer.mouseMove(add,add.width/2,add.height/2);}
        function tooltip(path:string):string {const card=pointer.findChild(view,'task-'+view.selectedId),add=pointer.findChild(card,'subtask-add'),tip=pointer.findChild(add,'task-tooltip');if(tip.visible)tip.contentItem.parent.grabToImage(image=>image.saveToFile(path));return JSON.stringify({visible:tip.visible,background:tip.background.color.toString(),text:tip.contentItem.color.toString(),width:tip.width});}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
        function refresh():void {root.request({action:'list'});}
    }
    Component.onCompleted:'''))

def ipc(method, *args):
    return subprocess.check_output(['quickshell','ipc','--path',str(home/'qml'),'call','optimistic-test',method,*map(str,args)],text=True,env=env,stderr=subprocess.DEVNULL)

def state(): return json.loads(ipc('state'))
def wait(fn):
    end=time.monotonic()+12
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (subprocess.CalledProcessError,ValueError):pass
        time.sleep(.05)
    raise AssertionError('Timed out; see '+str(root/'qml.log'))
def task(id=member['id'], projected=False): return next(t for t in state()['projected' if projected else 'tasks'] if t['id']==id)
def status(id, projected=False): return next((r['status'] for r in task(projected=projected)['workflowRows'] if r['id']==id),'hidden')

with (root/'qml.log').open('w') as log:
    process=subprocess.Popen(['quickshell','--path',str(home/'qml')],env=env,stdout=log,stderr=log)
    try:
        wait(lambda:len(state()['tasks'])==9 and not state()['busy'])
        ipc('open',project['id']);ipc('expand',member['id']);ipc('select',member['id'])
        ipc('key',0x01000015);wait(lambda:state()['subtask']==ids['instruction'])
        ipc('key',0x01000013);assert state()['subtask']==''
        time.sleep(.2);ipc('button','subtask-'+ids['instruction'])
        assert state()['subtask']==ids['instruction'] and not state()['queue'], (state()['subtask'],state()['queue'])
        focus=json.loads(ipc('subfocus',ids['instruction']))
        assert focus['selected'] and focus['chevron'], focus
        ordered=[r['id'] for r in state()['rows']]
        next_top=ordered[(ordered.index(member['id'])+1)%len(ordered)]
        ipc('keymod',0x01000015,0x04000000)
        assert state()['selected']==next_top and not state()['subtask']
        ipc('keymod',0x01000013,0x04000000)
        assert state()['selected']==member['id'] and not state()['subtask']
        time.sleep(.2);ipc('button','subtask-'+ids['instruction'])
        ipc('keymod',0x01000015,0x02000000)
        wait(lambda:not state()['busy'])
        assert not state()['subtask'] and not state()['queue']
        ipc('select',member['id'])

        geometry=json.loads(ipc('geometry',member['id']))
        assert geometry['width']==14 and abs(geometry['centerX']-geometry['columnCenter'])<1 and abs(geometry['centerX']-geometry['dateCenter'])<1 and geometry['dateCentered'] and abs(geometry['center']-geometry['titleCenter'])<1,geometry
        ipc('snapshot');(home/'hold').touch()
        ipc('key',0x01000015);ipc('key',ord('D'))
        assert status(ids['instruction'],True)=='done' and status(ids['instruction'])=='ready'
        assert status(ids['agreed'],True)=='ready' and status(ids['agreed'])=='blocked'
        ipc('stale');assert status(ids['instruction'],True)=='done'
        ipc('key',0x01000015);assert state()['subtask']==ids['agreed']  # Skip flow headers.
        ipc('key',0x01000004);assert status(branch,True)=='ready'
        assert len(state()['queue'])==2
        (home/'hold').unlink();wait(lambda:not state()['queue'])
        assert status(branch)=='ready' and state()['subtask']==ids['agreed']
        ipc('key',0x01000015);assert state()['subtask']==branch
        ordered=[r['id'] for r in state()['rows']];next_task=ordered[(ordered.index(member['id'])+1)%len(ordered)]
        ipc('key',0x01000015);assert state()['selected']==next_task and not state()['subtask']
        ipc('key',0x01000013);assert state()['subtask']==branch
        ipc('key',0x01000004);wait(lambda:not state()['queue'])
        ipc('key',0x01000015);assert state()['subtask']==ids['received']
        # Failure restores the removed card; an independent queued completion still succeeds.
        (home/'hold').touch();(home/'fail').touch()
        ipc('complete',plain['id']);assert not any(r['id']==plain['id'] for r in state()['rows'])
        assert task(plain['id'],True)['done'] and not task(plain['id'])['done']
        ipc('toggle',member['id'],ids['received']);assert status(ids['received'],True)=='done'
        (home/'hold').unlink();wait(lambda:not state()['queue'] and not state()['busy'])
        assert not task(plain['id'])['done'] and any(r['id']==plain['id'] for r in state()['rows'])
        assert status(ids['received'])=='done' and 'rejected' in state()['message']
        (home/'hold').touch();(home/'fail').touch()
        ipc('toggle',dependent['id'],dep_sent);ipc('toggle',dependent['id'],dep_agree)
        assert len(state()['queue'])==2
        assert task(dependent['id'],True)['subtasks'][1]['items'][0]['done']
        (home/'hold').unlink();wait(lambda:not state()['queue'] and not state()['busy'])
        restored=task(dependent['id'],True)
        assert not restored['subtasks'][0]['done'] and not restored['subtasks'][1]['items'][0]['done']
        assert restored['revision']==dependent['revision']
        # C hides the focused task before persistence, survives stale refresh,
        # and restores it on failure. A later successful cancellation persists.
        ipc('select',plain['id']);ipc('snapshot');(home/'hold').touch();(home/'fail').touch()
        ipc('key',ord('C'))
        assert task(plain['id'],True)['status']=='cancelled' and task(plain['id'])['status']!='cancelled'
        assert not any(r['id']==plain['id'] for r in state()['rows'])
        ipc('stale');assert task(plain['id'],True)['status']=='cancelled'
        (home/'hold').unlink();wait(lambda:not state()['queue'] and not state()['busy'])
        assert task(plain['id'],True)['status']!='cancelled' and any(r['id']==plain['id'] for r in state()['rows'])
        ipc('select',plain['id']);ipc('key',ord('C'))
        wait(lambda:not state()['queue'] and not state()['busy'])
        assert task(plain['id'])['status']=='cancelled'
        ipc('hoverAdd',member['id']);time.sleep(.7)
        tip=json.loads(ipc('tooltip',root/'tooltip.png'))
        assert tip['visible'] and tip['background']=='#39354b' and tip['text']=='#f5f5f7' and tip['width']<=320,tip
        ipc('capture',root/'controls.png');wait(lambda:(root/'controls.png').exists())
        logs=(root/'qml.log').read_text()
        assert not any(word in logs for word in ['TypeError','ReferenceError','Binding loop','Cannot assign']),logs
        print('PASS immediate completion, dependency projection, ordered queue, stale refresh, failure rollback, arrow navigation and themed geometry:',root)
    finally:
        if (home/'hold').exists():(home/'hold').unlink()
        process.terminate();process.wait(timeout=5)
