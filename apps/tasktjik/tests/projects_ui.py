"""Exercise projects, template copies, flow toggles and layout using synthetic data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-project-ui-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text('[tasktjik]\nauthority=""\ndirectory="' + str(root / 'tasks') + '"\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'target/release/tasktjik-api', home / 'tasktjik-api')
env = {**os.environ, 'TASKTJIK_HOME': str(home), 'QT_QPA_PLATFORM': 'offscreen',
       'QT_QUICK_BACKEND': 'software', 'QT_QUICK_CONTROLS_STYLE': 'Basic',
       'XDG_RUNTIME_DIR': str(root / 'runtime'), 'XDG_CONFIG_HOME': str(root / 'config'),
       'XDG_DATA_HOME': str(root / 'data'), 'XDG_STATE_HOME': str(root / 'state'),
       'XDG_CACHE_HOME': str(root / 'cache')}

def request(value):
    return json.loads(subprocess.check_output([str(home / 'tasktjik-api'), 'request'], input=json.dumps(value), text=True, env=env))

def step(id, kind='task', **extra):
    return dict(id=id, title=id.capitalize(), kind=kind, due='', done=False, items=[], **extra)

project = request({'action': 'create', 'fields': {'title': 'Synthetic celebration', 'kind': 'project', 'group': 'Socials'}})
flow = step('response', 'flow')
flow['due'] = '2099-11-20'
flow['items'] = [step('agree'), step('decline', 'exit'), step('remind', 'reminder', offset='one week before')]
flow['items'][0]['items'] = [step('branch')]
template = request({'action': 'create', 'fields': {'title': 'Synthetic contribution', 'kind': 'template', 'subtasks': [step('sent'), flow, step('received')]}})
member = request({'action': 'create', 'template_id': template['id'], 'fields': {'title': 'Synthetic guest', 'project': project['id'], 'group': 'Socials'}})
request({'action': 'create', 'fields': {'title': 'Outside task', 'group': 'Work'}})
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    function testSettings() {return taskSettings;}
    function testFill(title) {titleInput.text=title;}
    function testNewTemplate() {newItem('template');titleInput.text='Created template';workflowEditor.add('flow','');workflowEditor.flush();}
    function testTemplate() {templatePicker.currentIndex=1;templatePicker.activated(1);}
    function testEdit() {return {kind:editing?editing.kind:'',nodes:workflowEditor.nodes,title:titleInput.text};}
'''))
shell = home / 'qml/shell.qml'
s = shell.read_text().replace('implicitWidth: 860', 'implicitWidth: 1200').replace('implicitHeight: 680', 'implicitHeight: 1100').replace('import QtQuick\n', 'import QtQuick\nimport QtQuick.Window\nimport QtTest\n')
s = s.replace('    Component.onCompleted:', '''    TestCase {id:pointer;name:"ProjectInput";when:false}
    IpcHandler {
        target:"project-test"
        function state():string {return JSON.stringify({view:view.view,drawer:pointer.findChild(view,'drag-group-drawer').opened,emptyGroups:view.dropGroups.filter(g=>g.visible && g.rows.count===0).length,project:view.projectId,count:view.visibleCount(),tasks:view.stateData.tasks,error:view.errorMessage,groups:view.stateData.groups,settings:view.testSettings().visible,renaming:view.groupRename?view.groupRename.source:'',editor:view.editor,edit:view.testEdit(),busy:view.requestBusy,rows:view.visualRows()});}
        function clickTask(id:string, twice:bool):void {const row=pointer.findChild(view,'task-'+id);if(twice)pointer.mouseDoubleClickSequence(row,80,12);else pointer.mouseClick(row,80,12);}
        function complete(id:string):void {const row=pointer.findChild(view,'task-'+id);pointer.mouseMove(row,10,10);pointer.wait(180);const button=pointer.findChild(view,'complete-'+id);pointer.mouseClick(button,button.width/2,button.height/2);}
        function heading(group:string):void {const label=pointer.findChild(view,'group-heading-'+group);pointer.mouseDoubleClickSequence(label,label.width/2,label.height/2);}
        function nameGroup(name:string):void {const settings=view.testSettings();if(view.groupRename){const input=pointer.findChild(view,'group-heading-edit-'+view.groupRename.source);input.text=name;input.forceActiveFocus();view.Window.window.requestActivate();pointer.keyClick(Qt.Key_Return);}else {const input=pointer.findChild(settings,'group-name-input');input.text=name;const dialog=pointer.findChild(settings,'group-name-dialog');dialog.accept();}}
        function groupDialog():bool {return pointer.findChild(view.testSettings(),'group-confirm-dialog').visible;}
        function confirmGroup(accept:bool):void {const dialog=pointer.findChild(view.testSettings(),'group-confirm-dialog');if(accept)dialog.accept();else dialog.reject();}
        function settingsClose():void {view.testSettings().close();}
        function settingsTab(tab:string):void {view.testSettings().tab=tab;}
        function settingsButton(name:string):void {const button=pointer.findChild(view.testSettings(),name);pointer.mouseClick(button,button.width/2,button.height/2);}
        function settingsDrag(source:string,target:string):void {const settings=view.testSettings(),src=settings.dragRows.find(r=>r.modelData===source),dest=settings.dragRows.find(r=>r.modelData===target),handle=pointer.findChild(src,'group-handle-'+source);const scroll=pointer.findChild(settings,'settings-scroll');scroll.contentItem.contentY=Math.max(0,src.y+src.height-scroll.height);pointer.wait(100);const surface=settings.panelItem,start=handle.mapToItem(surface,12,18),end=dest.mapToItem(surface,dest.width/2,dest.height/2);pointer.mousePress(surface,start.x,start.y);pointer.mouseMove(surface,start.x+12,start.y+12,100);pointer.mouseMove(surface,end.x,end.y,250);pointer.mouseRelease(surface,end.x,end.y);}
        function refresh():void {root.request({action:'list'});}
        function select(id:string):void {view.selectedId=id;view.focusList();}
        function button(name:string):void {const button=pointer.findChild(view,name);pointer.mouseClick(button,button.width/2,button.height/2);}
        function dragStart(id:string):void {const row=pointer.findChild(view,'task-'+id);const start=row.mapToItem(view,80,10);pointer.mousePress(view,start.x,start.y);pointer.mouseMove(view,start.x+20,start.y+15,100);}
        function cancelDrag():void {pointer.keyClick(Qt.Key_Escape);pointer.mouseRelease(view,0,0);}
        function leave():void {view.leaveProject();}
        function key(code:int):void {view.Window.window.requestActivate();pointer.wait(30);pointer.keyClick(code);}
        function expand(id:string):void {view.toggleExpanded(id);}
        function toggle(id:string):void {const box=pointer.findChild(view,'subtask-complete-'+id);pointer.mouseClick(box,box.width/2,box.height/2);}
        function newTemplate():void {view.testNewTemplate();}
        function visibility(value:bool):void {const p=view.stateData.tasks.find(t=>t.kind==='project');root.request({action:'edit',id:p.id,revision:p.revision,fields:{onlyInProject:value}});}
        function create():void {view.newItem('todo');view.testFill('Created from template');view.testTemplate();}
        function save():void {view.saveEditor();}
        function add(id:string):void {const task=view.stateData.tasks.find(t=>t.id===id);view.addSubtask(task);}
        function capture(path:string):void {(view.testSettings().visible?view.testSettings().panelItem:view).grabToImage(image=>image.saveToFile(path));}
        function back():void {view.projectId='';view.selectVisibleTask();}
        function move(id:string,group:string):void {
            const row=pointer.findChild(view,'task-'+id);const start=row.mapToItem(view,80,10);
            pointer.mousePress(view,start.x,start.y);pointer.mouseMove(view,start.x+20,start.y+15,100);pointer.wait(250);
            const target=view.dropGroups.find(g=>g.modelData===group);const end=target.mapToItem(view,60,target.height-8);
            pointer.mouseMove(view,end.x,end.y,100);pointer.wait(100);pointer.mouseRelease(view,end.x,end.y);
        }
    }
    Component.onCompleted:''')
shell.write_text(s)

def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'project-test', method, *map(str, args)], text=True, env=env, stderr=subprocess.DEVNULL)

def state():
    return json.loads(ipc('state'))

def wait(fn, timeout=12):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except (subprocess.CalledProcessError, ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see ' + str(root / 'qml.log'))

def current():
    return next(t for t in state()['tasks'] if t['id'] == member['id'])

def row_status(id):
    return next((r['status'] for r in current()['workflowRows'] if r['id'] == id), 'hidden')

with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
    try:
        wait(lambda: len(state()['tasks']) == 4)
        assert state()['count'] == 2  # One project card plus the outside task.
        assert state()['emptyGroups'] == 0
        ipc('button', 'view-projects');wait(lambda: state()['view'] == 'projects')
        assert state()['count'] == 1
        ipc('button', 'new-item');wait(lambda: state()['editor'] == 'edit')
        assert state()['edit']['kind'] == 'project'
        ipc('key', 0x01000000)
        ipc('select', project['id']);ipc('key', ord('E'))
        wait(lambda: state()['editor'] == 'edit')
        assert state()['edit']['kind'] == 'project'
        ipc('key', 0x01000000);ipc('button', 'edit-task')
        wait(lambda: state()['editor'] == 'edit');ipc('key', 0x01000000)
        ipc('select', project['id']);ipc('key', 0x01000004)
        wait(lambda: state()['project'] == project['id'])
        ipc('leave');wait(lambda: not state()['project'] and state()['view'] == 'projects')
        ipc('button', 'view-all')
        ipc('dragStart', project['id']);wait(lambda: state()['drawer']);assert state()['emptyGroups']==0
        ipc('cancelDrag');wait(lambda: not state()['drawer'])
        ipc('select', project['id']);ipc('key', 0x01000004)
        wait(lambda: state()['project'] == project['id'])
        assert state()['count'] == 1
        ipc('expand', member['id']);time.sleep(.3)
        choices = member['subtasks'][1]['items']
        agree, decline, remind = [c['id'] for c in choices]
        branch = choices[0]['items'][0]['id']
        received = member['subtasks'][2]['id']
        assert row_status(remind) == 'snoozed'
        ipc('toggle', decline)
        wait(lambda: row_status(received) == 'cancelled')
        ipc('toggle', decline)
        wait(lambda: row_status(received) == 'ready')
        ipc('toggle', agree)
        wait(lambda: row_status(branch) == 'ready')
        assert row_status(remind) == 'cancelled'
        ipc('capture', root / 'project.png');wait(lambda: (root / 'project.png').exists())
        ipc('create');wait(lambda: state()['editor'] == 'edit')
        assert len(state()['edit']['nodes']) == 3
        ipc('capture', root / 'template-editor.png');wait(lambda: (root / 'template-editor.png').exists())
        ipc('save');wait(lambda: not state()['editor'] and len(state()['tasks']) == 5)
        copied = next(t for t in state()['tasks'] if t['title'] == 'Created from template')
        assert copied['project'] == project['id']
        assert copied['subtasks'][1]['items'][0]['done'] is False
        assert copied['subtasks'][0]['id'] != template['subtasks'][0]['id']
        ipc('add', member['id']);wait(lambda: state()['editor'] == 'edit')
        assert len(state()['edit']['nodes']) == 4
        ipc('key', 0x01000000);ipc('back');time.sleep(.3)
        ipc('move', project['id'], 'Work')
        wait(lambda: next(t for t in state()['tasks'] if t['id'] == project['id'])['group'] == 'Work')
        ipc('visibility', 'false');wait(lambda: state()['count'] == 4)
        ipc('visibility', 'true');wait(lambda: state()['count'] == 2)
        ipc('newTemplate');wait(lambda: state()['editor'] == 'edit')
        assert state()['edit']['nodes'][0]['kind'] == 'flow'
        ipc('save');wait(lambda: any(t['title'] == 'Created template' for t in state()['tasks']))
        created_template = next(t for t in state()['tasks'] if t['title'] == 'Created template')
        assert created_template['kind'] == 'template' and created_template['subtasks'][0]['kind'] == 'flow'
        # Direct manipulation: double click edits, C cancels, and hover check completes.
        ipc('settingsClose');ipc('back');ipc('select', project['id']);ipc('clickTask', project['id'], 'false')
        wait(lambda: state()['project'] == project['id'])
        time.sleep(.3)
        positions = state()['rows']
        assert positions and min(r['x'] for r in positions) < 100, positions
        ipc('clickTask', member['id'], 'true');wait(lambda: state()['editor'] == 'edit')
        assert current()['done'] is False
        ipc('key', 0x01000000);ipc('select', member['id']);ipc('key', ord('C'))
        wait(lambda: current()['status'] == 'cancelled' and not state()['busy'])
        restored = current()
        request({'action':'status','id':restored['id'],'revision':restored['revision'],'status':'ready','actor':'fixture'})
        ipc('refresh');wait(lambda: current()['status'] == 'ready')
        ipc('select',member['id'])
        ipc('key', ord('E'));wait(lambda: state()['editor'] == 'edit')
        ipc('select', member['id']);ipc('key', ord('C'))
        wait(lambda: current()['status'] == 'cancelled' and not state()['editor'])
        restored = current()
        request({'action':'status','id':restored['id'],'revision':restored['revision'],'status':'ready','actor':'fixture'})
        ipc('refresh');wait(lambda: current()['status'] == 'ready')
        ipc('complete', member['id']);wait(lambda: current()['done'])
        done = current();request({'action':'toggle','id':done['id'],'revision':done['revision']})
        ipc('refresh');wait(lambda: not current()['done'])
        # The heading rename only changes this project, including with hidden members elsewhere.
        hidden = request({'action':'create','fields':{'title':'Hidden Socials fixture','group':'Socials'}})
        ipc('refresh');wait(lambda: any(t['id']==hidden['id'] for t in state()['tasks']))
        ipc('heading','Socials');assert not state()['settings'] and state()['renaming']=='Socials'
        ipc('key',0x01000000);assert not state()['renaming'] and current()['group']=='Socials'
        ipc('heading','Socials');ipc('capture',root/'inline-rename.png');ipc('nameGroup','Guests')
        wait(lambda: current()['group']=='Guests')
        assert next(t for t in state()['tasks'] if t['id']==hidden['id'])['group']=='Socials'
        ipc('button','settings-button');wait(lambda: state()['settings']);ipc('settingsTab','groups')
        ipc('settingsButton','settings-new');ipc('nameGroup','Empty fixture')
        wait(lambda: 'Empty fixture' in state()['groups'])
        ipc('capture',root / 'before-drag.png');ipc('settingsDrag','Guests','Work');wait(lambda: ipc('groupDialog').strip()=='true')
        assert current()['group']=='Guests'
        ipc('confirmGroup','false');assert current()['group']=='Guests'
        ipc('capture',root / 'before-drag.png');ipc('settingsDrag','Guests','Work');wait(lambda: ipc('groupDialog').strip()=='true')
        ipc('confirmGroup','true');wait(lambda: current()['group']=='Work' and 'Guests' not in state()['groups'])
        ipc('capture', root / 'settings.png');wait(lambda: (root / 'settings.png').exists())
        ipc('settingsTab','templates');ipc('settingsButton','settings-new')
        wait(lambda: state()['editor']=='edit' and not state()['settings'])
        assert state()['edit']['kind']=='template'
        ipc('key',0x01000000)
        assert not state()['error'], state()['error']
        logs = (root / 'qml.log').read_text()
        assert not any(word in logs for word in ['TypeError', 'ReferenceError', 'Binding loop', 'Cannot assign']), logs
        print('PASS projects, workflows, double-click edit, hover completion, list/editor C, scoped rename, confirmed group drag and settings templates:', root)
    finally:
        process.terminate();process.wait(timeout=5)
