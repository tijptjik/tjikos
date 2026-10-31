"""Real QML pointer gestures with synthetic tasks and an offscreen window."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import tempfile

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-drag-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text(
    '[tasktjik]\nauthority=""\ndirectory="' + str(root / 'tasks') + '"\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'target/release/tasktjik-api', home / 'tasktjik-api')
env = {**os.environ, 'TASKTJIK_HOME': str(home), 'QT_QPA_PLATFORM': 'offscreen',
       'QT_QUICK_BACKEND': 'software', 'XDG_RUNTIME_DIR': str(root / 'runtime'),
       'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
       'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

def request(value):
    return json.loads(subprocess.check_output([str(home / 'tasktjik-api'), 'request'],
        input=json.dumps(value), text=True, env=env))

moving = request({'action': 'create', 'fields': {'title': 'Move this synthetic task', 'group': 'Work'}})
anchor = request({'action': 'create', 'fields': {'title': 'Target synthetic task', 'group': 'Tech'}})
request({'action': 'create', 'fields': {'title': 'Keep this task', 'group': 'Work'}})
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    function testRow(id) {
        for(const group of dropGroups)for(let i=0;i<group.rows.count;i++) {
            const row=group.rows.itemAt(i);
            if(row.modelData.id===id){const p=row.mapToItem(app,60,row.gapHeight+10);return {x:p.x,y:p.y,opacity:row.opacity,gap:row.gapHeight};}
        }
        return {};
    }
'''))
shell = home / 'qml/shell.qml'
s = shell.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n')
s = s.replace('view.stateData = JSON.parse(text);', "const state=JSON.parse(text);state.groups=['Work','Tech','Empty'];view.stateData=state;")
s = s.replace('    Component.onCompleted:', '''    TestCase {id:pointer;name:"DragInput";when:false}
    IpcHandler {
        target:"drag-test"
        function state():string {return JSON.stringify({tasks:view.stateData.tasks,drag:!!view.dragTask,pending:view.pendingMove,group:view.dropGroup,before:view.dropBefore,error:view.errorMessage});}
        function manual():void {view.sortOrder=4;}
        function automatic():void {view.sortOrder=0;}
        function row(id:string):string {return JSON.stringify(view.testRow(id));}
        function press(x:real,y:real):void {pointer.mousePress(view,x,y,Qt.LeftButton);}
        function move(x:real,y:real):void {pointer.mouseMove(view,x,y,20);}
        function release(x:real,y:real):void {pointer.mouseRelease(view,x,y,Qt.LeftButton);}
        function cancel():void {pointer.keyClick(Qt.Key_Escape);}
        function refresh():void {root.request({action:'list'});}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
    Component.onCompleted:''')
shell.write_text(s)

def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
        'call', 'drag-test', method, *map(str,args)], text=True, env=env, stderr=subprocess.DEVNULL)

def wait(fn, timeout=12):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (subprocess.CalledProcessError, ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see '+str(root / 'qml.log'))

with (root / 'qml.log').open('w') as log:
    process=subprocess.Popen(['quickshell','--path',str(home / 'qml')],env=env,stdout=log,stderr=log)
    try:
        wait(lambda:len(json.loads(ipc('state'))['tasks'])==3)
        ipc('manual')
        a=wait(lambda:json.loads(ipc('row',moving['id'])))
        b=wait(lambda:json.loads(ipc('row',anchor['id'])))
        ipc('press',a['x'],a['y']);ipc('move',a['x']+15,a['y'])
        wait(lambda:json.loads(ipc('state'))['drag'])
        assert json.loads(ipc('row',moving['id']))['opacity']==.5
        time.sleep(.2)
        b=wait(lambda:json.loads(ipc('row',anchor['id'])))
        ipc('move',b['x'],b['y']-5)
        wait(lambda:json.loads(ipc('state'))['group']=='Tech')
        wait(lambda:json.loads(ipc('row',anchor['id']))['gap']>0)
        ipc('capture',root / 'drag-preview.png')
        wait(lambda:(root / 'drag-preview.png').is_file())
        # Background refresh must not destroy the gesture or dimmed source.
        ipc('refresh');time.sleep(.25)
        assert json.loads(ipc('state'))['drag']
        ipc('release',b['x'],b['y']-5)
        wait(lambda:not json.loads(ipc('state'))['drag'])
        tasks=request({'action':'list'})
        moved=next(t for t in tasks if t['id']==moving['id'])
        target=next(t for t in tasks if t['id']==anchor['id'])
        assert moved['group']=='Tech'
        assert moved['manualOrder'] < target['id'].replace('-','')+'8'
        # Escape cancels a gesture without modifying any task.
        a=wait(lambda:json.loads(ipc('row',moving['id'])))
        ipc('press',a['x'],a['y']);ipc('move',a['x']+15,a['y'])
        wait(lambda:json.loads(ipc('state'))['drag'])
        ipc('cancel');wait(lambda:not json.loads(ipc('state'))['drag'])
        ipc('release',a['x']+15,a['y'])
        assert request({'action':'list'})==tasks
        # Automatic sorting never starts a drag or sends a move.
        ipc('automatic');a=wait(lambda:json.loads(ipc('row',moving['id'])))
        ipc('press',a['x'],a['y']);ipc('move',a['x']+30,a['y']+15)
        assert not json.loads(ipc('state'))['drag']
        ipc('release',a['x']+30,a['y']+15)
        assert request({'action':'list'})==tasks
        assert not json.loads(ipc('state'))['error']
        assert 'TypeError' not in (root / 'qml.log').read_text()
        print('PASS manual pointer drag, opacity, preview gap, refresh stability, persisted move, automatic-mode guard:',root)
    finally:
        process.terminate();process.wait(timeout=5)
