"""Project placement, member due dates, settings control, and saved preference."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-project-sort-'))
qml = root / 'qml'
shutil.copytree(APP / 'qml', qml)
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
env = {**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software',
       'QT_QUICK_CONTROLS_STYLE': 'Basic', 'XDG_RUNTIME_DIR': str(root / 'runtime'),
       'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
       'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

def task(id, **values):
    return dict(id=id, title=id, kind='todo', group='All', due='', period='once',
                done=False, status='ready', tags=[], subtasks=[], **values)

tasks = [task('middle'), task('project'), task('empty'), task('member'), task('done'), task('cancelled')]
tasks[0]['due'] = '2099-02-20'
for index in (1, 2):
    tasks[index].update(kind='project', onlyInProject=True)
tasks[3].update(project='project', due='2099-02-10', group='Hidden')
tasks[4].update(project='project', due='2099-01-01', done=True, status='done')
tasks[5].update(project='project', due='2099-01-02', status='cancelled')
state = dict(tasks=tasks, groups=['All', 'Hidden'], today='2099-02-01')
(qml / 'shell.qml').write_text('''import QtQuick
import QtQuick.Controls
import QtTest
import Quickshell
import Quickshell.Io
ShellRoot {
    TestCase {id:input;when:false}
    FloatingWindow {
        implicitWidth:900;implicitHeight:700;color:'#26233a'
        TodoApp {id:view;anchors.fill:parent;view:'all';stateData:''' + json.dumps(state) + '''
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
        }
    }
    IpcHandler {
        target:'sort-test'
        function state():string {return JSON.stringify({mode:view.projectSort,rows:view.tasks('All').map(t=>t.id),due:view.effectiveDue(view.stateData.tasks[1])});}
        function choose(index:int):void {const settings=input.findChild(view,'task-settings');settings.open();const choice=input.findChild(settings,'project-sort-choice');choice.currentIndex=index;choice.activated(index);settings.close();}
        function sort(index:int):void {view.sortOrder=index;}
        function progressFixture(value:string):void {view.stateData=JSON.parse(value);}
        function progressState():string {return JSON.stringify({sort:view.sortOrder,rows:view.tasks('All').map(t=>({id:t.id,progress:view.completionProgress(t)}))});}
        function progressChoice():void {const choice=input.findChild(view,'sortChoice');const index=choice.model.indexOf('Progress');choice.currentIndex=index;choice.activated(index);if(view.sortOrder!==5)throw new Error('Progress selection failed');}
        function due(value:string):void {const next=JSON.parse(JSON.stringify(view.stateData));next.tasks[3].due=value;view.stateData=next;}
        function capture(path:string):void {const settings=input.findChild(view,'task-settings');settings.open();input.wait(150);settings.panelItem.grabToImage(image=>image.saveToFile(path));}
        function quit():void {Qt.quit();}
    }
}
''')

def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(qml), 'call',
                                    'sort-test', method, *map(str, args)], env=env,
                                   text=True, stderr=subprocess.DEVNULL)

def state_now():
    return json.loads(ipc('state'))

def wait(fn):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except (subprocess.CalledProcessError, ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out: ' + str(root))

with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(qml)], env=env, stdout=log, stderr=log)
    try:
        wait(state_now)
        assert state_now() == {'mode': 'first-due', 'rows': ['project', 'middle', 'empty'], 'due': '2099-02-10'}, state_now()
        ipc('sort', 1)
        assert state_now()['rows'] == ['middle', 'project', 'empty']
        for mode, index in [('first', 0), ('last', 1)]:
            ipc('choose', index)
            for order in range(6):
                ipc('sort', order)
                rows = state_now()['rows']
                assert rows[-1 if mode == 'first' else 0] == 'middle', (mode, order, rows)
        ipc('choose', 2);ipc('sort', 0);ipc('due', '2099-03-01')
        assert state_now()['rows'] == ['middle', 'project', 'empty']
        ipc('due', '')
        assert state_now()['due'] == '' and state_now()['rows'][0] == 'middle'
        ipc('choose', 1);ipc('capture', root / 'settings.png')
        wait(lambda: (root / 'settings.png').exists())
        ipc('quit');process.wait(timeout=5)
        process = subprocess.Popen(['quickshell', '--path', str(qml)], env=env, stdout=log, stderr=log)
        wait(state_now)
        assert state_now()['mode'] == 'last' and state_now()['rows'][0] == 'middle', state_now()
        ipc('choose', 2)
        progress_tasks = [task('zero'), task('half'), task('quarter'), task('full'), task('also-half'), task('empty-progress')]
        for row, done, total in zip(progress_tasks[1:], [1, 2, 3, 2, 0], [2, 8, 3, 4, 0]):
            row['subtaskProgress'] = dict(done=done, total=total)
        fixture = dict(tasks=progress_tasks, groups=['All'], today='2099-02-01')
        ipc('progressFixture', json.dumps(fixture))
        ipc('progressChoice')
        assert state_now()['rows'] == ['full', 'also-half', 'half', 'quarter', 'empty-progress', 'zero'], ipc('progressState')
        progress_tasks[2]['subtaskProgress']['done'] = 6
        ipc('progressFixture', json.dumps(fixture))
        assert state_now()['rows'] == ['full', 'quarter', 'also-half', 'half', 'empty-progress', 'zero'], state_now()
        errors = (root / 'qml.log').read_text()
        assert not any(word in errors for word in ['TypeError', 'ReferenceError', 'Binding loop', 'Cannot assign']), errors
        print('PASS project placement across all sorts, earliest active hidden member, undated projects, reactive dates, settings, persistence and reactive percentage sorting:', root)
    finally:
        if process.poll() is None:
            process.terminate();process.wait(timeout=5)
