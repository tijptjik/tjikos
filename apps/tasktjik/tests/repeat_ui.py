"""Natural-language repeats, optional deadlines and date-view filtering in QML."""
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-repeat-ui-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text('[tasktjik]\nauthority=""\ndirectory=' + json.dumps(str(root / 'tasks')) + '\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'tasktjik-api', home / 'tasktjik-api')
env = {**os.environ, 'TASKTJIK_HOME': str(home), 'TASKTJIK_VIEW': 'today',
       'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software', 'QT_QUICK_CONTROLS_STYLE': 'Basic',
       'XDG_RUNTIME_DIR': str(root / 'runtime'), 'XDG_CONFIG_HOME': str(root / 'config'),
       'XDG_DATA_HOME': str(root / 'data'), 'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

def request(value):
    return json.loads(subprocess.check_output([str(home / 'tasktjik-api'), 'request'], input=json.dumps(value), text=True, env=env))

undated = request({'action': 'create', 'fields': {'title': 'Synthetic undated'}})
request({'action': 'create', 'fields': {'title': 'Synthetic future', 'due': '2099-11-20'}})
snoozed = request({'action': 'create', 'fields': {'title': 'Synthetic snoozed'}})
request({'action': 'snooze', 'id': snoozed['id'], 'until': '2099-11-20T09:00:00+08:00'})
today = request({'action': 'create', 'fields': {'title': 'Synthetic today', 'due': datetime.date.today().isoformat()}})
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    function testDraft() {newDefault();titleInput.text='Synthetic repeat';reveal('repeat');}
    function testRepeatState() {return {typing:repeatTyping,text:repeatInput.text,parsed:parsedRepeat};}
'''))
shell = home / 'qml/shell.qml'
shell.write_text(shell.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n').replace('    Component.onCompleted:', '''    TestCase {id:pointer;name:'RepeatInput';when:false}
    IpcHandler {
        target:'repeat-test'
        function state():string {return JSON.stringify({tasks:view.stateData.tasks,editor:view.editor,busy:view.requestBusy,error:view.errorMessage,repeat:view.testRepeatState(),rows:view.dropGroups.filter(g=>g.visible).reduce((rows,g)=>rows.concat(view.tasks(g.modelData).map(t=>t.id)),[]),emptyGroups:view.dropGroups.filter(g=>g.visible && !g.rows.count).length});}
        function mode(mode:string):void {view.view=mode;}
        function draft():void {view.testDraft();}
        function text(value:string):void {const field=pointer.findChild(view,'repeat-input');field.text=value;field.forceActiveFocus();}
        function key(code:int):void {pointer.keyClick(code);}
        function retype():void {const button=pointer.findChild(view,'repeat-edit');pointer.mouseClick(button,button.width/2,button.height/2);}
        function save():void {view.saveEditor();}
        function edit(id:string):void {view.selectedId=id;view.openEditor();}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
    Component.onCompleted:'''))

def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'repeat-test', method, *map(str, args)], text=True, env=env, stderr=subprocess.DEVNULL)

def state():
    return json.loads(ipc('state'))

def wait(fn):
    until = time.monotonic() + 12
    while time.monotonic() < until:
        try:
            value = fn()
            if value:
                return value
        except (ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see ' + str(root / 'qml.log'))

with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], stdout=log, stderr=log, env=env)
    try:
        wait(lambda: len(state()['tasks']) == 4)
        assert set(state()['rows']) == {undated['id'], today['id']}, state()
        assert state()['emptyGroups'] == 0
        ipc('mode', 'week');assert set(state()['rows']) == {undated['id'], today['id']}
        ipc('mode', 'all');assert len(state()['rows']) == 4
        ipc('mode', 'today');ipc('draft');wait(lambda: state()['repeat']['typing'])
        ipc('text', 'every two weeks');ipc('key', 0x01000004)
        wait(lambda: not state()['repeat']['typing'])
        assert state()['repeat']['parsed']['label'] == 'Every 2 weeks'
        ipc('capture', root / 'repeat-summary.png');wait(lambda: (root / 'repeat-summary.png').exists())
        ipc('retype');assert state()['repeat']['typing']
        ipc('text', 'every zero days');ipc('save');assert state()['editor'] == 'edit' and state()['error']
        ipc('text', 'every two weeks');ipc('save')
        wait(lambda: not state()['editor'] and len(state()['tasks']) == 5)
        task = next(t for t in state()['tasks'] if t['title'] == 'Synthetic repeat')
        assert task['due'] == '' and task['repeat']['unit'] == 'weeks' and task['repeat']['value'] == 2
        assert task['repeat']['start'] == datetime.date.today().isoformat()
        assert len(state()['rows']) == 3
        ipc('edit', task['id']);assert not state()['repeat']['typing']
        ipc('retype');ipc('text', 'on the 1st and 15th of every month');ipc('key', 0x01000004)
        assert state()['repeat']['parsed']['rule'] == {'unit': 'days-of-the-month', 'value': '1,15'}
        ipc('save');wait(lambda: not state()['editor'])
        saved = next(t for t in state()['tasks'] if t['id'] == task['id'])
        assert saved['due'] == '' and saved['repeat']['start'] == task['repeat']['start']
        assert not state()['error']
        logs = (root / 'qml.log').read_text()
        assert not any(word in logs for word in ['TypeError', 'ReferenceError', 'Binding loop']), logs
        print('PASS natural repeat entry, interpreted summary, pencil retyping, no deadline, stable anchor and Today/Week undated filtering:', root)
    finally:
        process.terminate();process.wait(timeout=5)
