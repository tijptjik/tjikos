"""Real QML Enter handling with a delayed synthetic authority; no private data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-save-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text(
    '[tasktjik]\nsocket=' + json.dumps(str(root / 'authority.sock')) + '\n')
(root / 'server.toml').write_text(
    '[server]\nsocket=' + json.dumps(str(root / 'authority.sock'))
    + '\ndirectory=' + json.dumps(str(root / 'tasks'))
    + '\norigin="https://synthetic.ts.net"\nallowed_users=["synthetic@example.test"]\n'
    + '[server.local_users]\n' + json.dumps(str(os.getuid())) + '="synthetic@example.test"\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'tasktjik-api', home / 'native-api')
env = {**os.environ, 'TASKTJIK_HOME': str(home), 'QT_QPA_PLATFORM': 'offscreen',
       'QT_QUICK_BACKEND': 'software', 'QT_QUICK_CONTROLS_STYLE': 'Basic',
       'XDG_RUNTIME_DIR': str(root / 'runtime'), 'XDG_CONFIG_HOME': str(root / 'config'),
       'XDG_DATA_HOME': str(root / 'data'), 'XDG_STATE_HOME': str(root / 'state'),
       'XDG_CACHE_HOME': str(root / 'cache')}
# The gate holds a response until the test releases it, so assertions cannot
# accidentally pass because a fast local save completed before the second Enter.
wrapper = home / 'tasktjik-api'
wrapper.write_text('''#!/usr/bin/python3
import json, pathlib, subprocess, sys, time
home=pathlib.Path(__file__).parent
value=json.load(sys.stdin)
with (home/'requests.jsonl').open('a') as log:
    log.write(json.dumps(value)+'\\n')
deadline=time.monotonic()+20
while ((home/'hold').exists() or (value.get('action')=='undo' and (home/'hold-undo').exists())) and time.monotonic()<deadline:
    time.sleep(.02)
if (home/'reject-purge').exists() and value.get('action')=='purge':
    (home/'reject-purge').unlink()
    sys.stderr.write('Synthetic deletion rejected')
    sys.exit(1)
if (home/'reject-undo').exists() and value.get('action')=='undo':
    (home/'reject-undo').unlink()
    sys.stderr.write('Synthetic undo rejected')
    sys.exit(1)
result=subprocess.run([str(home/'native-api'),*sys.argv[1:]],input=json.dumps(value),text=True,capture_output=True)
if (home/'lose-reply').exists() and value.get('action')=='create' and result.returncode==0:
    (home/'lose-reply').unlink()
    sys.stderr.write('Synthetic connection lost after saving')
    sys.exit(1)
sys.stdout.write(result.stdout)
sys.stderr.write(result.stderr)
sys.exit(result.returncode)
''')
wrapper.chmod(0o700)
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    property alias testUndoButton: toastUndo
    function testDraft(title, due) {
        selectedId='';
        openEditor({title:title,kind:'todo',group:'Finances',tags:['synthetic'],due:due,repeat:{},reference:'Synthetic note'});
    }
    function draftFields() {return {title:titleInput.text,due:dueInput.text,group:groupInput.editText,tags:tagsInput.text,reference:planInput.text};}
    function testEdit(title) {selectedId=stateData.tasks[0].id;openEditor();titleInput.text=title;}
    function testDue(value) {dueInput.text=value;titleInput.forceActiveFocus();}
'''))
shell = home / 'qml/shell.qml'
shell.write_text(shell.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n').replace('    Component.onCompleted:', '''    TestCase {id:input;name:"SaveInput";when:false}
    IpcHandler {
        target:"save-test"
        function state():string {return JSON.stringify({ready:!!view.stateData.today,busy:view.requestBusy,tasks:view.stateData.tasks,rows:view.listState.tasks,editor:view.editor,pending:view.pendingSave,deleting:view.pendingPurge,queue:view.queuedPurges,history:view.purgeHistory,toast:view.undoToastTask,selected:view.selectedId,error:view.errorMessage,fields:view.draftFields()});}
        function draft(title:string,due:string):void {view.testDraft(title,due);}
        function enter():void {input.keyClick(Qt.Key_Return);}
        function edit(title:string):void {view.testEdit(title);}
        function due(value:string):void {view.testDue(value);}
        function repeatSave():void {input.keyClick(Qt.Key_Return);view.saveEditor();view.saveEditor();}
        function refresh():void {root.request({action:'list'});}
        function select(id:string):void {view.selectedId=id;view.forceActiveFocus();}
        function purge():void {input.keyClick(Qt.Key_P);}
        function undo():void {input.keyClick(Qt.Key_U);}
        function undoClick():void {input.mouseClick(view.testUndoButton,view.testUndoButton.width/2,view.testUndoButton.height/2);}
        function resetHistory():void {view.purgeHistory=[];view.undoToastTask='';}
        function toggle():void {input.keyClick(Qt.Key_D);}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
    Component.onCompleted:'''))


def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
        'call', 'save-test', method, *map(str, args)], text=True, env=env, stderr=subprocess.DEVNULL)


def current():
    return json.loads(ipc('state'))


def wait(fn, timeout=12):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except (subprocess.CalledProcessError, ValueError, KeyError):
            pass
        time.sleep(.04)
    raise AssertionError('Timed out; see ' + str(root / 'qml.log'))


def requests():
    return [json.loads(line) for line in (home / 'requests.jsonl').read_text().splitlines()]


def stored():
    return json.loads(subprocess.check_output([str(home / 'native-api'), 'request'],
        input='{"action":"list"}', text=True, env=env))


def settled():
    state = current()
    return not state['busy'] and not state['deleting'] and not state['queue']


def mutations_since(offset):
    return [(r['action'], r.get('id')) for r in requests()[offset:] if r['action'] in ('purge', 'undo')]


with (root / 'qml.log').open('w') as log, (root / 'authority.log').open('w') as authority_log:
    authority = subprocess.Popen([str(home / 'native-api'), '--config', str(root / 'server.toml')], env=env, stdout=authority_log, stderr=authority_log)
    process = None
    try:
        wait(lambda: (root / 'authority.sock').is_socket())
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
        wait(lambda: current()['ready'] and not current()['busy'])
        (home / 'hold').touch()
        ipc('draft', 'Synthetic delayed save', '')
        ipc('enter')
        wait(lambda: current()['pending'])
        state = current()
        assert state['editor'] == '' and len(state['rows']) == 1, state
        assert state['rows'][0]['saving'] and not stored(), state
        ipc('repeatSave')
        wait(lambda: len([r for r in requests() if r['action'] == 'create']) == 1)
        ipc('capture', root / 'saving.png')
        wait(lambda: (root / 'saving.png').is_file())
        (home / 'hold').unlink()
        wait(lambda: not current()['pending'] and len(current()['tasks']) == 1)
        assert len(stored()) == 1
        assert len([r for r in requests() if r['action'] == 'create']) == 1

        (home / 'hold').touch()
        ipc('edit', 'Synthetic edited save')
        ipc('enter')
        state = current()
        assert state['editor'] == '' and state['rows'][0]['title'] == 'Synthetic edited save', state
        assert len(state['rows']) == 1 and stored()[0]['title'] == 'Synthetic delayed save'
        ipc('repeatSave')
        (home / 'hold').unlink()
        wait(lambda: not current()['pending'] and current()['tasks'][0]['title'] == 'Synthetic edited save')
        assert len([r for r in requests() if r['action'] == 'edit']) == 1

        # A save queued behind an in-flight refresh still accepts Enter immediately.
        (home / 'hold').touch()
        before = len(requests())
        ipc('refresh')
        wait(lambda: len(requests()) > before)
        ipc('draft', 'Synthetic queued save', '')
        ipc('enter')
        state = current()
        assert state['editor'] == '' and state['pending'] and not state['pending']['sent'], state
        ipc('repeatSave')
        (home / 'hold').unlink()
        wait(lambda: not current()['pending'] and len(current()['tasks']) == 2)
        assert len(stored()) == 2

        # Validation errors restore every draft field, and remove the optimistic row.
        ipc('draft', 'Synthetic invalid date', 'not-a-date')
        ipc('enter')
        wait(lambda: current()['error'] and current()['editor'] == 'edit')
        state = current()
        assert not state['pending'] and len(state['rows']) == 2, state
        assert state['fields'] == {'title': 'Synthetic invalid date', 'due': 'not-a-date',
            'group': 'Finances', 'tags': 'synthetic', 'reference': 'Synthetic note'}, state
        invalid_id = requests()[-1]['request_id']
        ipc('due', '2026-11-03')
        ipc('enter')
        wait(lambda: not current()['pending'] and current()['editor'] == '' and len(current()['tasks']) == 3)
        assert requests()[-1]['request_id'] != invalid_id

        # The server can commit before a reply is lost. Retrying unchanged must
        # reuse the UUID and return the original task, rather than creating a copy.
        ipc('draft', 'Synthetic lost reply', '')
        (home / 'lose-reply').touch()
        ipc('enter')
        wait(lambda: current()['error'] and current()['editor'] == 'edit')
        assert len(stored()) == 4
        first = requests()[-1]['request_id']
        ipc('enter')
        wait(lambda: not current()['pending'] and current()['editor'] == '' and len(current()['tasks']) == 4)
        assert requests()[-1]['request_id'] == first
        assert len(stored()) == 4 and not current()['error']

        # Completion and deletion patches must preserve every unrelated row.
        target = current()['tasks'][0]
        ipc('select', target['id'])
        ipc('toggle')
        wait(lambda: any(t['id'] == target['id'] and t['done'] for t in current()['tasks']))
        assert len(current()['tasks']) == 4
        ipc('select', target['id'])
        (home / 'hold').touch()
        ipc('purge')
        state = current()
        assert state['deleting'] and len(state['rows']) == 3 and len(stored()) == 4, state
        assert state['selected'] != target['id'], state
        assert state['toast'] == target['id'], 'Undo should appear before deletion is confirmed'
        ipc('select', target['id'])
        ipc('purge')
        (home / 'hold').unlink()
        wait(lambda: not current()['deleting'] and len(current()['tasks']) == 3)
        assert current()['toast'] == target['id'] and len(stored()) == 3
        assert len([r for r in requests() if r['action'] == 'purge']) == 1

        # A rejected deletion restores the row and its selection.
        target = current()['tasks'][0]
        ipc('select', target['id'])
        (home / 'reject-purge').touch()
        ipc('purge')
        wait(lambda: current()['error'] and not current()['deleting'])
        assert len(current()['rows']) == 3 and current()['selected'] == target['id']
        assert len(stored()) == 3 and not current()['toast']

        # P is accepted visually even while a previous refresh is in flight.
        (home / 'hold').touch()
        before = len(requests())
        ipc('refresh')
        wait(lambda: len(requests()) > before)
        ipc('purge')
        state = current()
        assert state['deleting'] and not state['deleting']['sent'] and len(state['rows']) == 2, state
        (home / 'hold').unlink()
        wait(lambda: not current()['deleting'] and len(current()['tasks']) == 2)
        assert len(stored()) == 2

        # Cancelling a deletion behind a slow refresh requires no server mutation.
        wait(settled)
        ipc('resetHistory')
        target = current()['tasks'][0]
        ipc('select', target['id'])
        (home / 'hold').touch()
        offset = len(requests())
        ipc('refresh')
        wait(lambda: len(requests()) > offset)
        ipc('purge')
        assert current()['toast'] == target['id'] and not current()['deleting']['sent']
        ipc('undoClick')
        assert not current()['deleting'] and len(current()['rows']) == 2
        (home / 'hold').unlink()
        wait(settled)
        assert mutations_since(offset) == [] and len(stored()) == 2

        # Undo during an in-flight delete restores the row before either reply.
        ipc('select', target['id'])
        (home / 'hold').touch()
        (home / 'hold-undo').touch()
        offset = len(requests())
        ipc('purge')
        wait(lambda: mutations_since(offset) == [('purge', target['id'])])
        assert current()['toast'] == target['id']
        ipc('undoClick')
        assert len(current()['rows']) == 2 and current()['deleting']['undoRequested']
        ipc('undo')  # A repeated Undo must not send the same restore twice.
        (home / 'hold').unlink()
        wait(lambda: mutations_since(offset) == [('purge', target['id']), ('undo', target['id'])])
        assert len(stored()) == 1 and len(current()['rows']) == 2
        (home / 'hold-undo').unlink()
        wait(settled)
        assert len(stored()) == 2 and not current()['error']

        # If deletion itself is rejected, queued Undo must never undo some
        # earlier server action; the original row is already the desired state.
        ipc('select', target['id'])
        (home / 'hold').touch()
        (home / 'reject-purge').touch()
        offset = len(requests())
        ipc('purge')
        wait(lambda: mutations_since(offset) == [('purge', target['id'])])
        ipc('undoClick')
        (home / 'hold').unlink()
        wait(lambda: current()['error'] and settled())
        assert len(stored()) == 2 and len(current()['rows']) == 2
        assert mutations_since(offset) == [('purge', target['id'])]

        # Rapid deletes queue; Undo cancels the newest queued one first.
        ipc('resetHistory')
        first, second = current()['tasks']
        (home / 'hold').touch()
        offset = len(requests())
        ipc('select', first['id'])
        ipc('purge')
        wait(lambda: mutations_since(offset) == [('purge', first['id'])])
        assert current()['selected'] == second['id']
        ipc('purge')
        assert len(current()['rows']) == 0 and len(current()['queue']) == 1
        assert current()['toast'] == second['id']
        ipc('undo')
        assert len(current()['rows']) == 1 and not current()['queue']
        ipc('undo')
        assert len(current()['rows']) == 2
        (home / 'hold').unlink()
        wait(settled)
        assert mutations_since(offset) == [('purge', first['id']), ('undo', first['id'])]
        assert len(stored()) == 2

        # Confirmed deletes also undo immediately, then restore in stack order.
        ipc('resetHistory')
        for task in (first, second):
            ipc('select', task['id'])
            ipc('purge')
            wait(settled)
        assert not stored()
        (home / 'hold-undo').touch()
        offset = len(requests())
        ipc('undo')
        wait(lambda: mutations_since(offset) == [('undo', second['id'])])
        ipc('undo')
        assert len(current()['rows']) == 2 and len(current()['queue']) == 1
        (home / 'hold-undo').unlink()
        wait(settled)
        assert mutations_since(offset) == [('undo', second['id']), ('undo', first['id'])]
        assert len(stored()) == 2

        # Failure of a restore rolls back dependent queued restores. Retrying
        # uses the same operation IDs and the correct server undo-stack order.
        ipc('resetHistory')
        for task in (first, second):
            ipc('select', task['id'])
            ipc('purge')
            wait(settled)
        (home / 'hold-undo').touch()
        (home / 'reject-undo').touch()
        offset = len(requests())
        ipc('undo')
        wait(lambda: mutations_since(offset) == [('undo', second['id'])])
        undo_id = requests()[-1]['request_id']
        ipc('undo')
        assert len(current()['rows']) == 2
        (home / 'hold-undo').unlink()
        wait(lambda: current()['error'] and settled())
        assert not stored() and not current()['rows']
        assert mutations_since(offset) == [('undo', second['id'])]
        ipc('undo')
        wait(settled)
        assert requests()[-1]['request_id'] == undo_id
        ipc('undo')
        wait(settled)
        assert len(stored()) == 2 and not current()['error']

        ipc('capture', root / 'saved.png')
        wait(lambda: (root / 'saved.png').is_file())
        assert 'TypeError' not in (root / 'qml.log').read_text()
        print('PASS instant delete/undo, queued cancellation, in-flight undo, ordered restores, failure recovery, saves and incremental reconciliation:', root)
    finally:
        if process:
            process.terminate()
            process.wait(timeout=5)
        authority.terminate()
        authority.wait(timeout=5)
