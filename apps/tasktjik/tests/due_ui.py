"""Natural-language due dates through the real QML editor and a synthetic local store."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-due-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text(
    '[tasktjik]\nauthority=""\ndirectory=' + json.dumps(str(root / 'tasks')) + '\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'tasktjik-api', home / 'tasktjik-api')
env = {**os.environ, 'TASKTJIK_HOME': str(home), 'TASKTJIK_DEMO': '0',
       'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software',
       'QT_QUICK_CONTROLS_STYLE': 'Basic', 'XDG_RUNTIME_DIR': str(root / 'runtime'),
       'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
       'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace('    id: app', '''    id: app
    function testDraft() {
        draftId='';selectedId='';openEditor({title:'Synthetic natural date',kind:'todo',group:'General',tags:[],due:'',repeat:{},reference:''});
        dueToday=new Date(2026,10,1,12);reveal('due');
    }
    function testReading() {
        background.color=theme.background;collection='reading';draftId='synthetic-capture';selectedId='';
        openEditor({title:'Synthetic article — a useful read',kind:'reading',group:'All',tags:[],due:'',repeat:{},reference:'https://example.test/article'});
    }
    function testLayout() {
        return {width:editorForm.width,height:editorForm.height,buttonsHeight:detailButtons.height,
            titleY:titleInput.mapToItem(editorForm,0,0).y,titleHeight:titleInput.height,
            linkY:planInput.mapToItem(editorForm,0,0).y,linkHeight:planInput.height,
            buttonsY:detailButtons.y,actionsY:editorActions.y,dueVisible:!!sections.due};
    }
    function testDue(value) {dueInput.text=value;dueInput.forceActiveFocus();}
    function testEdit() {selectedId=stateData.tasks[0].id;openEditor();dueToday=new Date(2026,10,1,12);}
    function testFields() {return {due:dueInput.text,preview:duePreview.text,parsed:parsedDue,focused:dueInput.activeFocus,suggestions:dueSuggestions,suggesting:showDueSuggestions,suggestionIndex:dueSuggestionIndex};}
'''))
shell = home / 'qml/shell.qml'
shell.write_text('''// A small authority-backed host keeps these editor tests independent of shell queues.
import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
ShellRoot {
    id:root
    function request(value) {backend.payload=JSON.stringify(value);backend.output='';backend.failure='';view.errorMessage='';backend.running=true;}
    Process {
        id:backend
        property string payload
        property string output
        property string failure
        command:[Quickshell.env('TASKTJIK_HOME')+'/tasktjik-api','state']
        stdinEnabled:true
        onStarted:{write(payload);stdinEnabled=false;}
        stdout:StdioCollector {onStreamFinished:backend.output=text}
        stderr:StdioCollector {onStreamFinished:backend.failure=text.trim()}
        onExited:(exitCode,exitStatus)=>{
            stdinEnabled=true;
            if(exitCode!==0){view.errorMessage=failure;return;}
            view.stateData=JSON.parse(output);
            if(['create','edit'].includes(JSON.parse(payload).action))view.accepted();
        }
    }
    FloatingWindow {
        implicitWidth:860;implicitHeight:680
        color:'#26233a'
        TodoApp {
            id:view;anchors.fill:parent;anchors.margins:20;focus:true
            requestBusy:backend.running
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
            onRequest:value=>root.request(value)
        }
    }
    Component.onCompleted:root.request({action:'list'})
}
''')
shell.write_text(shell.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n').replace('    FloatingWindow {', '    FloatingWindow {\n        id:testWindow').replace('    Component.onCompleted:', '''    TestCase {id:input;name:"DueInput";when:false}
    IpcHandler {
        target:"due-test"
        function state():string {return JSON.stringify({ready:!!view.stateData.today,busy:view.requestBusy,tasks:view.stateData.tasks,editor:view.editor,pending:backend.running,error:view.errorMessage,fields:view.testFields()});}
        function draft():void {view.testDraft();}
        function reading():void {view.testReading();}
        function layout():string {return JSON.stringify(view.testLayout());}
        function resize(width:int):void {view.anchors.fill=null;view.width=width-40;view.height=560;}
        function focusSection(name:string):void {input.findChild(view,'add-'+name).forceActiveFocus();}
        function tab():void {input.keyClick(Qt.Key_Tab);}
        function focused():string {return ['group','tags','due','repeat','reference','reminder'].map(name=>input.findChild(view,'add-'+name)).filter(button=>button.activeFocus).map(button=>button.objectName).join(',');}
        function down():void {input.keyClick(Qt.Key_Down);}
        function pressEscape():void {input.keyClick(Qt.Key_Escape);}
        function choose(index:int):void {const item=input.findChild(view,'due-suggestion-'+index);input.mouseClick(item,item.width/2,item.height/2);}
        function keypadEnter():void {input.keyClick(Qt.Key_Enter);}
        function due(value:string):void {view.testDue(value);}
        function edit():void {view.testEdit();}
        function enter():void {input.keyClick(Qt.Key_Return);}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
    Component.onCompleted:'''))


def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
        'call', 'due-test', method, *map(str, args)], text=True, env=env, stderr=subprocess.DEVNULL)


def current():
    return json.loads(ipc('state'))


def wait(fn):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        try:
            if fn():
                return
        except (subprocess.CalledProcessError, ValueError, KeyError):
            pass
        time.sleep(.04)
    raise AssertionError('Timed out; see ' + str(root / 'qml.log'))


def stored():
    return json.loads(subprocess.check_output([str(home / 'tasktjik-api'), 'request'],
        input='{"action":"list"}', text=True, env=env))


with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
    try:
        wait(lambda: current()['ready'] and not current()['busy'])
        ipc('resize', 720)
        wait(lambda: json.loads(ipc('layout'))['width'] < 720)
        ipc('reading')
        ipc('capture', root / 'reading.png')
        wait(lambda: (root / 'reading.png').exists())
        layout = json.loads(ipc('layout'))
        assert layout['linkY'] - layout['titleY'] - layout['titleHeight'] < 45, layout
        assert layout['buttonsY'] - layout['linkY'] - layout['linkHeight'] <= 20, layout
        assert layout['buttonsHeight'] <= 40 and layout['height'] < 300, layout
        # Reach the due-date chip through the actual Tab chain, then press Return.
        for _ in range(10):
            ipc('tab')
            if ipc('focused').strip() == 'add-due':
                break
        assert ipc('focused').strip() == 'add-due'
        ipc('enter')
        wait(lambda: current()['fields']['focused'])
        assert json.loads(ipc('layout'))['dueVisible']
        assert current()['editor'] == 'edit' and not stored()
        ipc('due', 'tomorrow')
        ipc('capture', root / 'reading-due.png')
        wait(lambda: (root / 'reading-due.png').exists())
        ipc('resize', 420)
        wait(lambda: json.loads(ipc('layout'))['width'] < 420)
        ipc('capture', root / 'reading-narrow.png')
        wait(lambda: (root / 'reading-narrow.png').exists())
        layout = json.loads(ipc('layout'))
        assert layout['buttonsHeight'] <= 80 and layout['height'] < 500, layout
        ipc('reading')
        ipc('focusSection', 'due')
        ipc('keypadEnter')
        wait(lambda: current()['fields']['focused'])
        assert json.loads(ipc('layout'))['dueVisible']
        ipc('resize', 720)
        ipc('draft')
        ipc('due', 'tom')
        assert current()['fields']['suggesting']
        ipc('capture', root / 'autocomplete.png')
        wait(lambda: (root / 'autocomplete.png').exists())
        ipc('enter')
        assert current()['editor'] == 'edit' and not stored()
        assert current()['fields']['due'] == '2026-11-02'
        ipc('due', 'next f')
        ipc('tab')
        assert current()['fields']['due'] == '2026-11-06'
        assert current()['fields']['focused'] and current()['editor'] == 'edit'
        ipc('due', '')
        ipc('down')
        ipc('down')
        ipc('enter')
        assert current()['fields']['due'] == '2026-11-02'
        ipc('due', 'in 3 w')
        ipc('pressEscape')
        assert not current()['fields']['suggesting'] and current()['editor'] == 'edit'
        ipc('due', 'tom')
        ipc('choose', 0)
        assert current()['fields']['due'] == '2026-11-02'
        assert current()['editor'] == 'edit' and not stored()
        ipc('due', 'in two weeks')
        state = current()
        assert state['fields']['parsed'] == {'value': '2026-11-15', 'error': ''}, state
        assert 'Sunday, 15 November 2026' in state['fields']['preview'], state
        ipc('capture', root / 'preview.png')
        wait(lambda: (root / 'preview.png').exists())
        ipc('enter')
        wait(lambda: not current()['pending'] and current()['editor'] == '')
        assert stored()[0]['due'] == '2026-11-15'

        ipc('edit')
        assert current()['fields']['due'] == '2026-11-15'
        ipc('due', 'February 30')
        ipc('enter')
        state = current()
        assert state['editor'] == 'edit' and not state['pending'], state
        assert state['error'] and state['fields']['focused'], state
        assert state['fields']['due'] == 'February 30'
        assert stored()[0]['due'] == '2026-11-15'
        ipc('capture', root / 'invalid.png')
        wait(lambda: (root / 'invalid.png').exists())

        ipc('due', 'next Friday')
        ipc('enter')
        wait(lambda: not current()['pending'] and current()['editor'] == '')
        assert stored()[0]['due'] == '2026-11-06'
        assert not current()['error']
        ipc('edit')
        ipc('due', '')
        ipc('enter')
        wait(lambda: not current()['pending'] and current()['editor'] == '')
        assert stored()[0]['due'] == ''
        assert 'ERROR' not in (root / 'qml.log').read_text()
        print('PASS Tab/Return/keypad Enter, reading layout at two widths, due-date preview, save, edit, rejection and clearing:', root)
    finally:
        process.terminate()
        process.wait(timeout=5)
