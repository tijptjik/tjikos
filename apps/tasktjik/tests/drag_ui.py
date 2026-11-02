"""Real QML pointer gestures with synthetic tasks and an offscreen window."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import tempfile
import sys

if '--wayland' in sys.argv:
    os.environ['AQ_BACKEND'] = 'headless'
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'runtjik/tests'))
    from private_compositor import private_compositor, wait_for
    with private_compositor(isolate_pointer=True):
        subprocess.run(['hyprctl', 'output', 'create', 'headless'], check=True, stdout=subprocess.DEVNULL)
        wait_for(lambda: json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True)))
        environment = {**os.environ, 'TASKTJIK_DRAG_WAYLAND': '1'}
        environment['WAYLAND_DISPLAY'] = str(Path(os.environ['XDG_RUNTIME_DIR']) / os.environ['WAYLAND_DISPLAY'])
        result = subprocess.run([sys.executable, '-B', __file__], env=environment)
    raise SystemExit(result.returncode)

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-drag-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text(
    '[tasktjik]\nauthority=""\ndirectory="' + str(root / 'tasks') + '"\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'target/release/tasktjik-api', home / 'native-api')
wrapper = home / 'tasktjik-api'
wrapper.write_text('''#!/usr/bin/python3
import json, pathlib, subprocess, sys, time
home = pathlib.Path(__file__).parent
value = json.load(sys.stdin)
with (home / 'requests.jsonl').open('a') as log:
    log.write(json.dumps(value) + '\\n')
gate = home / ('hold-move' if value.get('action') == 'move' else 'hold-read')
deadline = time.monotonic() + 20
while gate.exists() and time.monotonic() < deadline:
    time.sleep(.02)
if value.get('action') == 'move' and (home / 'reject-move').exists():
    (home / 'reject-move').unlink()
    sys.stderr.write('Synthetic move rejected')
    sys.exit(1)
result = subprocess.run([str(home / 'native-api'), *sys.argv[1:]], input=json.dumps(value), text=True, capture_output=True)
sys.stdout.write(result.stdout)
sys.stderr.write(result.stderr)
sys.exit(result.returncode)
''')
wrapper.chmod(0o700)
env = {**os.environ, 'TASKTJIK_HOME': str(home),
       'QT_QPA_PLATFORM': 'wayland' if os.environ.get('TASKTJIK_DRAG_WAYLAND') else 'offscreen',
       'QT_QUICK_BACKEND': 'software', 'XDG_RUNTIME_DIR': str(root / 'runtime'),
       'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
       'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

def request(value):
    return json.loads(subprocess.check_output([str(home / 'native-api'), 'request'],
        input=json.dumps(value), text=True, env=env))

moving = request({'action': 'create', 'fields': {'title': 'Move this synthetic task', 'group': 'Work', 'due': '2026-11-10'}})
anchor = request({'action': 'create', 'fields': {'title': 'Target synthetic task', 'group': 'Tech', 'due': '2026-11-02'}})
request({'action': 'create', 'fields': {'title': 'Keep this task', 'group': 'Work'}})
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace('color:"transparent"', 'color:theme.background', 1).replace("Choice {model:['Due: soonest'", "Choice {objectName:'sortChoice';model:['Due: soonest'").replace('    id: app', '''    id: app
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
s = s.replace('root.applyState(JSON.parse(output));', "const state=JSON.parse(output);state.groups=['Work','Tech','Empty'];root.applyState(state);")
s = s.replace('    Component.onCompleted:', '''    TestCase {id:pointer;name:"DragInput";when:false}
    IpcHandler {
        target:"drag-test"
        function state():string {return JSON.stringify({tasks:view.stateData.tasks,rows:view.listState.tasks,order:view.dropGroups.reduce((order,g)=>{order[g.modelData]=view.tasks(g.modelData).map(t=>t.id);return order;},{}),sort:view.sortOrder,drag:!!view.dragTask,pending:view.pendingMove,sent:view.moveSent,busy:view.requestBusy,group:view.dropGroup,before:view.dropBefore,error:view.errorMessage});}
        function manual():void {const c=pointer.findChild(view,'sortChoice');pointer.mouseClick(c,c.width/2,c.height/2,Qt.LeftButton);pointer.wait(250);pointer.keyClick(Qt.Key_End);pointer.keyClick(Qt.Key_Return);pointer.wait(250);if(view.sortOrder!==4)throw new Error('Manual selection failed: '+view.sortOrder);}
        function sort(mode:int):void {view.sortOrder=mode;}
        function groupPoint(name:string):string {const g=view.dropGroups.find(g=>g.modelData===name);pointer.wait(200);const target=g.visible?g:pointer.findChild(view,'drag-group-'+name);const p=target.mapToItem(view,g.visible?60:target.width/2,g.visible?g.height-8:target.height/2);return JSON.stringify({x:p.x,y:p.y});}
        function labels():string {return JSON.stringify({sort:pointer.findChild(view,'sortChoice').model,enable:!!pointer.findChild(view,'enableDragging')});}
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

def current():
    return json.loads(ipc('state'))


def stored():
    return request({'action': 'list'})


def drag(destination, before=None):
    # Settle the previous drop gap and group balancing before pressing a row.
    time.sleep(.35)
    point = json.loads(ipc('row', moving['id']))
    ipc('press', point['x'], point['y'])
    ipc('move', point['x'] + 15, point['y'])
    wait(lambda: current()['drag'])
    assert json.loads(ipc('row', moving['id']))['opacity'] == .5
    if before:
        point = json.loads(ipc('row', before))
        point['y'] -= 5
    else:
        point = json.loads(ipc('groupPoint', destination))
    ipc('move', point['x'], point['y'])
    wait(lambda: current()['group'] == destination)
    return point


def drop(point):
    ipc('release', point['x'], point['y'])
    try:
        wait(lambda: not current()['drag'])
    except AssertionError:
        raise AssertionError('Drop did not finish: ' + json.dumps(current()))


def settle():
    wait(lambda: not current()['pending'] and not current()['busy'])
    assert not current()['error'], current()


def assert_projected(group, mode, expected_order=None):
    state = current()
    assert state['pending'] and not state['drag'], state
    assert state['sort'] == mode, state
    assert next(t for t in state['rows'] if t['id'] == moving['id'])['group'] == group
    assert len(state['rows']) == 3 and len({t['id'] for t in state['rows']}) == 3
    assert json.loads(ipc('row', moving['id']))['opacity'] == 1
    if expected_order:
        assert state['order'][group] == expected_order, state


with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
    try:
        wait(lambda: len(current()['tasks']) == 3 and not current()['busy'])
        assert current()['sort'] == 0
        assert json.loads(ipc('labels')) == {'sort': ['Due: soonest', 'Due: latest', 'Title: A–Z', 'Title: Z–A', 'Manual'], 'enable': False}
        (home / 'hold-move').touch()
        point = drag('Tech', anchor['id'])
        wait(lambda: json.loads(ipc('row', anchor['id']))['gap'] > 0)
        ipc('capture', root / 'drag-preview.png')
        wait(lambda: (root / 'drag-preview.png').is_file())
        ipc('refresh')
        wait(lambda: not current()['busy'])
        assert current()['drag'], 'A refresh must not destroy an active gesture'
        drop(point)
        assert_projected('Tech', 0, [anchor['id'], moving['id']])
        assert next(t for t in stored() if t['id'] == moving['id'])['group'] == 'Work'
        time.sleep(.35)
        ipc('capture', root / 'optimistic-drop.png')
        wait(lambda: (root / 'optimistic-drop.png').is_file())
        (home / 'hold-move').unlink()
        settle()
        assert current()['order']['Tech'] == [anchor['id'], moving['id']]
        assert next(t for t in stored() if t['id'] == moving['id'])['group'] == 'Tech'

        # Even a same-group drop immediately returns to the selected automatic sort.
        (home / 'hold-move').touch()
        drop(drag('Tech', anchor['id']))
        assert_projected('Tech', 0, [anchor['id'], moving['id']])
        (home / 'hold-move').unlink()
        settle()

        # Every automatic sort permits dragging and sorts before the write completes.
        for mode in [1, 2, 3]:
            ipc('sort', mode)
            drop(drag('Work'))
            settle()
            (home / 'hold-move').touch()
            drop(drag('Tech', anchor['id']))
            expected = [anchor['id'], moving['id']] if mode == 3 else [moving['id'], anchor['id']]
            assert_projected('Tech', mode, expected)
            assert next(t for t in stored() if t['id'] == moving['id'])['group'] == 'Work'
            (home / 'hold-move').unlink()
            settle()
            assert current()['order']['Tech'] == expected

        # Manual retains the exact dropped position during and after persistence.
        ipc('manual')
        (home / 'hold-move').touch()
        drop(drag('Tech', anchor['id']))
        assert_projected('Tech', 4, [moving['id'], anchor['id']])
        (home / 'hold-move').unlink()
        settle()
        assert current()['order']['Tech'] == [moving['id'], anchor['id']]

        # Empty groups accept drops; a rejected write restores the original rows/order.
        before = stored()
        (home / 'hold-move').touch()
        (home / 'reject-move').touch()
        drop(drag('Empty'))
        assert_projected('Empty', 4, [moving['id']])
        assert stored() == before
        (home / 'hold-move').unlink()
        wait(lambda: not current()['pending'] and not current()['busy'] and current()['error'])
        assert stored() == before
        assert current()['order']['Tech'] == [moving['id'], anchor['id']]
        assert current()['order']['Empty'] == []

        # A drop queued behind a refresh is optimistic before it can even be sent.
        (home / 'hold-read').touch()
        (home / 'hold-move').touch()
        ipc('refresh')
        wait(lambda: current()['busy'])
        drop(drag('Work'))
        assert_projected('Work', 4)
        assert not current()['sent'] and stored() == before
        (home / 'hold-read').unlink()
        wait(lambda: current()['sent'])
        assert_projected('Work', 4)
        assert stored() == before
        (home / 'hold-move').unlink()
        settle()
        assert next(t for t in stored() if t['id'] == moving['id'])['group'] == 'Work'

        # Escape cancels only the gesture and never writes a move.
        before = stored()
        point = drag('Tech')
        ipc('cancel')
        wait(lambda: not current()['drag'])
        ipc('release', point['x'], point['y'])
        assert stored() == before and not current()['pending']
        assert 'TypeError' not in (root / 'qml.log').read_text()
        print('PASS all sort modes, immediate drop and autosort, delayed writes, manual placement, rejection rollback, queued refresh and Escape:', root)
    finally:
        process.terminate()
        process.wait(timeout=5)
