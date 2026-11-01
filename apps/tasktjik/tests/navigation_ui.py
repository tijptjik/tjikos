"""Exercise keyboard navigation against rendered, synthetic multi-column tasks."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-navigation-'))
(root / 'runtime').mkdir(mode=0o700)
home = root / 'qml'
shutil.copytree(APP / 'qml', home)
tasks = []
for group, count in [('Work', 2), ('All', 5), ('Books', 3), ('Home', 7), ('Small', 1)]:
    for index in range(count):
        tasks.append({'id': f'{group}-{index}', 'title': f'Synthetic {group} {index}',
                      'group': group, 'kind': 'todo', 'tags': [], 'done': index == 1,
                      'status': 'done' if index == 1 else 'inbox', 'revision': 1,
                      'due': '', 'repeat': {}, 'reference': ''})
fixture = {'tasks': tasks, 'groups': ['Work', 'All', 'Books', 'Home', 'Small'], 'today': '2026-11-01'}
component = home / 'TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    function testInputState() {
        return {editor:editor,title:titleInput.text,titleFocus:titleInput.activeFocus,
            searching:searching,filter:filterInput.text,filterFocus:filterInput.activeFocus,view:view};
    }
    function testGeometry() {
        const result=[];
        for(const group of dropGroups)for(let i=0;i<group.rows.count;i++) {
            const row=group.rows.itemAt(i),p=row.mapToItem(app,0,0);
            result.push({id:row.modelData.id,x:p.x,y:p.y});
        }
        return result;
    }
'''))
(home / 'shell.qml').write_text('''import QtQuick
import QtTest
import Quickshell
import Quickshell.Io
ShellRoot {
    FloatingWindow {
        id:testWindow
        implicitWidth:1500;implicitHeight:680
        TodoApp {
            id:view;width:1100;height:parent.height;focus:true;view:'all';showCompleted:true
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
            stateData:FIXTURE
        }
    }
    TestCase {id:keyboard;name:'Navigation';when:false}
    IpcHandler {
        target:'navigation-test'
        function state():string {return JSON.stringify({selected:view.selectedId,rows:view.testGeometry()});}
        function inputState():string {return JSON.stringify(view.testInputState());}
        function clickRow():void {
            const first=view.visualRows()[0];
            view.focusRow(first);
            keyboard.wait(50);
            for(const group of view.dropGroups)for(let i=0;i<group.rows.count;i++) {
                const row=group.rows.itemAt(i);
                if(row.modelData.id===first.id) {
                    keyboard.mouseClick(row,90,row.height/2);
                    return;
                }
            }
        }
        function select(id:string):void {view.selectedId=id;view.forceActiveFocus();}
        function key(name:string):void {
            const keys={Down:Qt.Key_Down,Up:Qt.Key_Up,j:Qt.Key_J,k:Qt.Key_K,Tab:Qt.Key_Tab,Left:Qt.Key_Left,Right:Qt.Key_Right,n:Qt.Key_N,N:Qt.Key_N,Slash:Qt.Key_Slash,Escape:Qt.Key_Escape};
            keyboard.keyClick(keys[name],name==='N'?Qt.ShiftModifier:Qt.NoModifier);
        }
        function resize(value:int):void {view.width=value;}
        function filter(value:string):void {view.filter=value;}
        function completed(show:bool):void {view.showCompleted=show;}
        function sort():void {view.sortOrder=3;}
    }
}
'''.replace('FIXTURE', json.dumps(fixture)))
env = {**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software',
       'XDG_RUNTIME_DIR': str(root / 'runtime'), 'XDG_CONFIG_HOME': str(root / 'config'),
       'XDG_CACHE_HOME': str(root / 'cache')}


def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(home), 'call',
                                    'navigation-test', method, *map(str, args)],
                                   env=env, text=True, stderr=subprocess.DEVNULL)


def state():
    return json.loads(ipc('state'))


def settled():
    previous = None
    stable = 0
    for _ in range(100):
        try:
            current = state()['rows']
            stable = stable + 1 if current == previous and current else 0
            if stable >= 5:
                return sorted(current, key=lambda row: (round(row['x']), row['y']))
            previous = current
        except (subprocess.CalledProcessError, ValueError):
            pass
        time.sleep(.1)
    raise AssertionError('Layout did not settle; see ' + str(root / 'qml.log'))


def check_navigation(columns):
    rows = settled()
    assert len({round(row['x']) for row in rows}) == columns, rows
    ids = [row['id'] for row in rows]
    for key, step in [('Down', 1), ('Up', -1), ('j', 1), ('k', -1), ('Tab', 1)]:
        ipc('select', ids[0])
        for index in range(1, len(ids) + 1):
            ipc('key', key)
            assert state()['selected'] == ids[(index * step) % len(ids)], (key, index, state(), ids)
    return rows


with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(home)], env=env, stdout=log, stderr=log)
    try:
        rows = check_navigation(2)
        assert [row['id'] for row in rows] != [task['id'] for task in tasks]
        # Horizontal navigation still chooses the nearest row in the adjacent column.
        first = rows[0]
        adjacent = [row for row in rows if row['x'] > first['x'] + 1]
        expected = min(adjacent, key=lambda row: abs(row['y'] - first['y']))
        ipc('select', first['id'])
        ipc('key', 'Right')
        assert state()['selected'] == expected['id']
        ipc('resize', 1500)
        check_navigation(3)
        ipc('completed', 'false')
        assert all(not row['id'].endswith('-1') for row in check_navigation(3))
        ipc('sort')
        check_navigation(3)
        ipc('filter', 'Home')
        assert all(row['id'].startswith('Home-') for row in check_navigation(1))
        ipc('filter', 'no matching synthetic task')
        time.sleep(.3)
        ipc('key', 'Down')
        assert not state()['rows'] and state()['selected'] == ''
        ipc('filter', '')
        ipc('resize', 650)
        check_navigation(1)
        # N still types in the filter, but creates an item after clicking a task
        # while that filter remains visible (including with a nonempty query).
        def input_state():
            return json.loads(ipc('inputState'))

        ipc('key', 'n')
        assert input_state()['editor'] == 'edit' and input_state()['titleFocus']
        ipc('key', 'n')
        assert input_state()['title'] == 'n'
        ipc('key', 'Escape')
        for typed_filter in [False, True]:
            ipc('key', 'Slash')
            assert input_state()['filterFocus']
            if typed_filter:
                ipc('key', 'n')
                assert input_state()['filter'] == 'n' and not input_state()['editor']
            settled()
            ipc('clickRow')
            assert input_state()['searching'] and not input_state()['filterFocus'], input_state()
            ipc('key', 'n')
            assert input_state()['editor'] == 'edit' and input_state()['titleFocus']
            assert input_state()['title'] == ''
            ipc('key', 'Escape')
            ipc('key', 'Slash')
            ipc('key', 'Escape')
            assert not input_state()['searching'] and input_state()['filter'] == ''
        ipc('key', 'Slash')
        ipc('clickRow')
        ipc('key', 'N')
        assert input_state()['view'] == 'today' and not input_state()['editor']
        assert 'ERROR' not in (root / 'qml.log').read_text()
        print('PASS rendered column navigation and New shortcut with filter focus:', root)
    finally:
        process.terminate()
        process.wait(timeout=5)
