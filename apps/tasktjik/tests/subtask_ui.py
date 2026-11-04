"""Synthetic workflow dependencies, compact subtask controls, month and empty layout."""
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-subtask-ui-'))
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
receipt = step('path-received', 'group')
receipt['title'] = 'Contribution received'
receipt['items'] = [step('book'), step('video')]
receipt['items'][0]['title'] = 'Books of Memories'
receipt['items'][1]['title'] = 'To a Lifetime Together'
pathway_request = {'action': 'create', 'fields': {'title': 'Synthetic contributor', 'project': project['id'], 'group': 'Contributions', 'workflow': True, 'subtasks': [
    receipt, step('book-review', requires=['book']), step('video-review', requires=['video']),
    step('book-compiled', requires=['book-review']), step('video-compiled', requires=['video-review']),
    step('thanks', requires=['book-compiled', 'video-compiled'], acceptCancelled=True)]}}
for stage in pathway_request['fields']['subtasks']:
    if stage['id'].startswith(('book-', 'video-')):
        prefix = 'Books of Memories' if stage['id'].startswith('book-') else 'To a Lifetime Together'
        stage['title'] = prefix + ' - Contribution ' + ('Review' if stage['id'].endswith('review') else 'Compiled')
    elif stage['id'] == 'thanks':
        stage['title'] = 'Thank you sent'
request({'action':'create','fields':{'title':'Within month','due':str(month_end)}})
request({'action':'create','fields':{'title':'Next month','due':str(next_month)}})
request({'action':'create','fields':{'title':'Undated'}})
blocked = request({'action':'create','fields':{'title':'Blocked task'}})
request({'action':'status','id':blocked['id'],'revision':blocked['revision'],'status':'blocked','blockers':[{'reason':'Waiting'}],'actor':'fixture'})
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    function testEmpty() {const p=emptyLabel.mapToItem(taskScroll,0,emptyLabel.height/2);return {y:p.y,height:taskScroll.height};}
''').replace("Label {text:searching && filter?'No matching tasks'", "Label {id:emptyLabel;text:searching && filter?'No matching tasks'"))
shell = home / 'qml/shell.qml'
shell.write_text(shell.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n').replace('    Component.onCompleted:', '''    TestCase {id:pointer;name:"SubtaskInput";when:false}
    IpcHandler {
        target:"subtask-test"
        function state():string {return JSON.stringify({view:view.view,project:view.projectId,count:view.visibleCount(),expanded:view.expandedTasks,blocked:view.showBlocked,editor:view.editor,tasks:view.stateData.tasks,error:view.errorMessage,busy:view.requestBusy||view.completing,rows:view.visualRows(),empty:view.testEmpty()});}
        function button(name:string):void {const button=pointer.findChild(view,name);pointer.mouseClick(button,button.width/2,button.height/2);}
        function task(id:string):void {const row=pointer.findChild(view,'task-'+id);pointer.mouseClick(row,80,12);}
        function select(id:string):void {view.selectedId=id;view.focusList();}
        function key(code:int):void {pointer.keyClick(code);}
        function has(id:string):bool {const row=pointer.findChild(view,'subtask-'+id);return !!row && row.visible;}
        function layout(id:string):string {const row=pointer.findChild(view,'subtask-'+id),date=pointer.findChild(view,'subtask-date-'+id);const card=pointer.findChild(view,'task-'+view.selectedId);return JSON.stringify({date:date.text,dateX:date.mapToItem(view,0,0).x,cardX:card.mapToItem(view,0,0).x,dot:row.dotColor.toString(),title:row.modelData.title,bold:pointer.findChild(row,'subtask-title-'+id).font.bold,strikeout:pointer.findChild(row,'subtask-title-'+id).font.strikeout});}
        function focus(id:string):void {pointer.findChild(view,'subtask-'+id).forceActiveFocus();}
        function editTemplate(id:string):void {view.openEditor(view.stateData.tasks.find(t=>t.id===id));}
        function save():void {view.saveEditor();}
        function leave():void {view.leaveProject();}
        function filter(text:string):void {view.filter=text;}
        function refresh():void {root.request({action:'list'});}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
    Component.onCompleted:'''))

def ipc(method, *args):
    return subprocess.check_output(['quickshell','ipc','--path',str(home/'qml'),'call','subtask-test',method,*map(str,args)],text=True,env=env,stderr=subprocess.DEVNULL)

def state():
    return json.loads(ipc('state'))

def wait(fn):
    end=time.monotonic()+12
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (subprocess.CalledProcessError,ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see '+str(root/'qml.log'))

def current():
    return next(t for t in state()['tasks'] if t['id']==member['id'])

def status(id):
    return next((r['status'] for r in current()['workflowRows'] if r['id']==id),'hidden')

with (root/'qml.log').open('w') as log:
    process=subprocess.Popen(['quickshell','--path',str(home/'qml')],env=env,stdout=log,stderr=log)
    try:
        wait(lambda:len(state()['tasks'])==7)
        assert state()['count']==4  # Project, two dated tasks, and undated; blocked hidden.
        ipc('button','view-month');wait(lambda:state()['view']=='month')
        names={t['title'] for t in state()['tasks'] if t['id'] in {r['id'] for r in state()['rows']}}
        assert 'Within month' in names and 'Undated' in names and 'Next month' not in names,names
        ipc('button','blocked-toggle');wait(lambda:state()['blocked'])
        assert any(r['id']==blocked['id'] for r in state()['rows'])
        ipc('button','blocked-toggle');ipc('task',project['id']);wait(lambda:state()['project']==project['id'])
        ipc('task',member['id']);wait(lambda:state()['expanded'].get(member['id']))
        assert ipc('has',ids['instruction']).strip()=='true'
        assert ipc('has',ids['agreement']).strip()=='false'
        assert current()['subtaskProgress']=={'done':0,'total':3}
        layout=json.loads(ipc('layout',ids['instruction']))
        assert abs(layout['dateX']-layout['cardX']-4)<1,layout
        assert len(layout['date'])<=3 and '-' not in layout['date'],layout
        ipc('capture',root/'only-ready.png');wait(lambda:(root/'only-ready.png').exists())
        ipc('select',member['id']);ipc('key',0x01000004);wait(lambda:not state()['expanded'].get(member['id']))
        ipc('key',0x01000004);wait(lambda:state()['expanded'].get(member['id']))
        ipc('focus',ids['instruction']);ipc('key',0x01000004);wait(lambda:status(ids['instruction'])=='done')
        assert ipc('has',ids['agreed']).strip()=='true'
        assert ipc('has',ids['received']).strip()=='false'
        assert json.loads(ipc('layout',ids['declined']))['dot']=='#eb6f92'
        assert json.loads(ipc('layout',ids['followup']))['dot']=='#f6c177'
        ipc('capture',root/'agreement.png');wait(lambda:(root/'agreement.png').exists())
        ipc('button','subtask-complete-'+ids['agreed']);wait(lambda:status(branch)=='ready')
        assert status(ids['received'])=='blocked'
        chosen=json.loads(ipc('layout',ids['agreed']))
        assert chosen['bold'] and not chosen['strikeout'],chosen
        assert json.loads(ipc('layout',ids['agreement']))['strikeout']
        assert json.loads(ipc('layout',ids['declined']))['strikeout']
        assert json.loads(ipc('layout',ids['followup']))['strikeout']
        ipc('button','blocked-toggle');wait(lambda:ipc('has',ids['received']).strip()=='true')
        ipc('button','subtask-complete-'+ids['received']);assert status(ids['received'])=='blocked'
        ipc('button','subtask-complete-'+branch);wait(lambda:status(ids['received'])=='ready')
        ipc('button','subtask-complete-'+ids['instruction']);wait(lambda:status(ids['instruction'])=='ready')
        ipc('button','blocked-toggle');wait(lambda:not state()['blocked'])
        assert all(ipc('has',ids[name]).strip()=='false' for name in ['agreement','agreed','declined','followup','received'])
        pathways=request(pathway_request);ipc('refresh');wait(lambda:len(state()['tasks'])==8)
        ipc('task',pathways['id']);wait(lambda:state()['expanded'].get(pathways['id']))
        def path_status(id):
            task = next(t for t in state()['tasks'] if t['id'] == pathways['id'])
            return next(r['status'] for r in task['workflowRows'] if r['id'] == id)
        ipc('focus','video');ipc('key',ord('C'));wait(lambda:path_status('video')=='cancelled')
        assert path_status('video-review') == 'cancelled'
        assert path_status('thanks') == 'blocked'
        for id in ['book','book-review','book-compiled']:
            wait(lambda:not state()['busy']);ipc('focus',id);ipc('key',0x01000004);wait(lambda:path_status(id)=='done')
        wait(lambda:path_status('thanks')=='ready')
        book_style=json.loads(ipc('layout','book'))
        assert book_style['bold'] and not book_style['strikeout'],book_style
        assert json.loads(ipc('layout','video'))['strikeout']
        assert json.loads(ipc('layout','path-received'))['strikeout']
        ipc('focus','thanks');time.sleep(.2)
        ipc('capture',root/'pathways-ready.png');wait(lambda:(root/'pathways-ready.png').exists())
        ipc('focus','video');ipc('key',ord('C'));wait(lambda:path_status('video')=='ready')
        assert path_status('thanks') == 'blocked'
        assert not json.loads(ipc('layout','path-received'))['strikeout']
        assert path_status('book-compiled') == 'done'
        ipc('editTemplate',template['id']);wait(lambda:state()['editor']=='edit')
        ipc('button','workflow-toggle');ipc('save');wait(lambda:next(t for t in state()['tasks'] if t['id']==template['id'])['workflow'] is False)
        ipc('key',0x01000000);ipc('leave');ipc('filter','no-such-fixture')
        wait(lambda:state()['count']==0);time.sleep(.2)
        empty=state()['empty'];assert abs(empty['y']-empty['height']/2)<40,empty
        ipc('capture',root/'empty.png');wait(lambda:(root/'empty.png').exists())
        assert not state()['error'],state()['error']
        logs=(root/'qml.log').read_text()
        assert not any(word in logs for word in ['TypeError','ReferenceError','Binding loop','Cannot assign']),logs
        print('PASS workflow gating, branch completion, pips, Enter/click controls, date alignment, month and centered empty state:',root)
    finally:
        process.terminate();process.wait(timeout=5)
