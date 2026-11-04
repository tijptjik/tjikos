"""Exercise editor option chips and searchable pickers with synthetic QML data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
if '--wayland' in sys.argv:
    os.environ['AQ_BACKEND'] = 'headless'
    sys.path.insert(0, str(APP.parent / 'runtjik/tests'))
    from private_compositor import private_compositor, wait_for
    with private_compositor(isolate_pointer=True):
        subprocess.run(['hyprctl', 'output', 'create', 'headless'], check=True, stdout=subprocess.DEVNULL)
        wait_for(lambda: json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True)))
        subprocess.run(['hyprctl', 'eval', 'hl.monitor({output="",mode="1400x1100@60",position="0x0",scale=1}); hl.window_rule({name="options-fixture",float=true,no_anim=true,match={title="^Tasktjik options fixture$"}})'], check=True, stdout=subprocess.DEVNULL)
        env = {**os.environ, 'TASKTJIK_OPTIONS_WAYLAND': '1'}
        env['WAYLAND_DISPLAY'] = str(Path(os.environ['XDG_RUNTIME_DIR']) / os.environ['WAYLAND_DISPLAY'])
        result = subprocess.run([sys.executable, '-B', __file__], env=env)
    raise SystemExit(result.returncode)

root = Path(tempfile.mkdtemp(prefix='tasktjik-editor-options-'))
qml = root / 'qml'
shutil.copytree(APP / 'qml', qml)
(root / 'runtime').mkdir(mode=0o700)
fixture = {'today': '2026-11-03', 'groups': ['All', 'Work'], 'tasks': [
    {'id': key, 'title': title, 'kind': kind, 'group': 'All', 'tags': [], 'due': '',
     'done': False, 'status': 'ready', 'revision': 1, 'repeat': {}, 'reference': '',
     'subtasks': [{'id': 'source-step', 'title': 'Prepare invitation', 'kind': 'task',
                   'due': '', 'done': False, 'items': []}] if kind == 'template' else []}
    for key, title, kind in [('p1', 'Celebration', 'project'), ('p2', '第二個專案', 'project'),
                              ('t1', 'Invitation', 'template')]]}
component = qml / 'TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    function testState() {
        return {editor:editor,project:projectPicker.currentValue,projectText:projectPicker.text,
            workflow:workflowEditor.visible,nodes:workflowEditor.nodes,title:titleInput.text,
            templateVisible:templatePicker.visible,projectVisible:projectPicker.visible};
    }
    function testDraft(project) {newItem('todo');if(project)openEditor(Object.assign({},editing,{project:project}));titleInput.text='Synthetic draft';}
'''))
(qml / 'shell.qml').write_text('''import QtQuick
import QtQuick.Window
import QtTest
import Quickshell
import Quickshell.Io
ShellRoot {
    id:root;property var saved: ({})
    FloatingWindow {
        id:win;title:'Tasktjik options fixture';implicitWidth:650;implicitHeight:900;color:'#26233a'
        TodoApp {
            id:view;anchors.fill:parent;view:'all'
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
            stateData:FIXTURE
            onRequest: value=>root.saved=value
        }
    }
    TestCase {id:pointer;name:'EditorOptions';when:false}
    IpcHandler {
        target:'options'
        function draft(project:string):void {view.testDraft(project);}
        function state():string {return JSON.stringify(view.testState());}
        function click(name:string):void {const item=pointer.findChild(view,name);pointer.mouseClick(item,item.width/2,item.height/2);}
        function search(name:string,text:string):void {pointer.findChild(view,name+'-search').text=text;}
        function picker(name:string):string {
            const popup=pointer.findChild(view,name+'-popup'),input=pointer.findChild(view,name+'-search'),list=pointer.findChild(view,name+'-results');
            return JSON.stringify({open:popup.visible,focus:input.activeFocus,query:input.text,
                titles:list.model.map(r=>r.title),width:popup.width,x:popup.contentItem.mapToItem(view,-popup.leftPadding,0).x,viewWidth:view.width,chipWidth:pointer.findChild(view,name).width});
        }
        function choose(name:string,index:int):void {const list=pointer.findChild(view,name+'-results'),row=list.itemAtIndex(index);pointer.mouseClick(row,row.width/2,row.height/2);}
        function key(code:int):void {view.Window.window.requestActivate();pointer.keyClick(code);}
        function resize(width:int):void {view.Window.window.width=width;}
        function kind(kind:string):void {view.newItem(kind);}
        function save():string {view.saveEditor();return JSON.stringify(root.saved);}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
}
'''.replace('FIXTURE', json.dumps(fixture, ensure_ascii=False)))
env = {**os.environ, 'QT_QPA_PLATFORM': 'wayland' if os.environ.get('TASKTJIK_OPTIONS_WAYLAND') else 'offscreen',
       'QT_QUICK_BACKEND': 'software', 'QT_QUICK_CONTROLS_STYLE': 'Basic',
       **{f'XDG_{key}_HOME': str(root / key.lower()) for key in ['CONFIG', 'DATA', 'STATE', 'CACHE']},
       'XDG_RUNTIME_DIR': str(root / 'runtime')}


def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(qml), 'call', 'options',
                                    method, *map(str, args)], env=env, text=True, stderr=subprocess.DEVNULL)


def wait(fn):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except (subprocess.CalledProcessError, ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see ' + str(root / 'qml.log'))


def state():
    return json.loads(ipc('state'))


def picker(name):
    return json.loads(ipc('picker', name))


def capture(name):
    if os.environ.get('TASKTJIK_OPTIONS_WAYLAND'):
        path = root / (name + '.png')
        time.sleep(.1)
        subprocess.run(['grim', str(path)], env=env, check=True)


with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(qml)], env=env, stdout=log, stderr=log)
    try:
        wait(lambda: state())
        ipc('draft', '')
        wait(lambda: state()['editor'] == 'edit')
        time.sleep(.2)
        assert not state()['workflow']
        assert not picker('project-choice')['open'] and not picker('from-template')['open']
        assert picker('project-choice')['chipWidth'] < 200
        capture('chips')
        ipc('click', 'project-choice')
        wait(lambda: picker('project-choice')['focus'])
        ipc('search', 'project-choice', '第二')
        assert picker('project-choice')['titles'] == ['第二個專案']
        assert state()['project'] == ''  # Searching never changes the draft.
        capture('project-search')
        ipc('key', 0x01000015)  # Down
        ipc('key', 0x01000004)  # Return
        wait(lambda: state()['project'] == 'p2')
        assert not picker('project-choice')['open']
        assert '第二個專案' in state()['projectText']
        ipc('click', 'project-choice')
        wait(lambda: picker('project-choice')['focus'])
        assert picker('project-choice')['query'] == ''
        ipc('search', 'project-choice', 'missing')
        ipc('key', 0x01000000)  # Escape dismisses only the picker.
        assert state()['editor'] == 'edit' and state()['project'] == 'p2'
        ipc('click', 'project-choice')
        wait(lambda: picker('project-choice')['focus'])
        ipc('choose', 'project-choice', 0)
        assert state()['project'] == ''
        ipc('click', 'from-template')
        wait(lambda: picker('from-template')['focus'])
        ipc('search', 'from-template', 'missing')
        assert picker('from-template')['titles'] == []
        ipc('key', 0x01000004)
        assert picker('from-template')['open'] and not state()['nodes']
        ipc('search', 'from-template', 'INVIT')
        assert picker('from-template')['titles'] == ['Invitation']
        ipc('choose', 'from-template', 0)
        assert state()['workflow'] and state()['nodes'][0]['title'] == 'Prepare invitation'
        assert state()['nodes'][0]['id'] != 'source-step'
        capture('template-applied')
        ipc('draft', '')
        assert not state()['workflow']
        ipc('click', 'add-subtasks')
        assert state()['workflow'] and not state()['nodes']
        ipc('kind', 'template')
        assert state()['workflow'] and not state()['projectVisible'] and not state()['templateVisible']
        ipc('draft', 'legacy-project')
        assert state()['project'] == 'legacy-project' and '(legacy)' in state()['projectText']
        ipc('resize', 360)
        time.sleep(.2)
        capture('narrow-chips')
        ipc('click', 'project-choice')
        wait(lambda: picker('project-choice')['focus'])
        bounds = picker('project-choice')
        assert bounds['width'] <= 328
        assert bounds['x'] >= 0 and bounds['x'] + bounds['width'] <= bounds['viewWidth'], bounds
        capture('narrow-search')
        ipc('search', 'project-choice', 'Celebration')
        ipc('choose', 'project-choice', 0)
        saved = json.loads(ipc('save'))
        assert saved['fields']['project'] == 'p1' and saved['fields']['title'] == 'Synthetic draft'
        logs = (root / 'qml.log').read_text()
        assert not any(word in logs for word in ['ReferenceError', 'TypeError', 'Unable to assign', 'Binding loop']), logs
        print('PASS compact chips, search focus/filtering, keyboard/pointer selection, Escape, clearing, template copies, subtasks, legacy projects, narrow layout and save:', root)
    finally:
        process.terminate()
        process.wait(timeout=5)
