"""Synthetic browser filing workflow, rendered in private Hyprland."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    home.mkdir()
    shutil.copytree(app / 'qml', home / 'qml')
    for path in app.glob('*.py'):
        shutil.copy2(path, home / path.name)
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('property string appMode:', 'property string testActionMode: ""\n    property string appMode:').replace('function send(data) {', "function send(data) {if(data.op==='activate')root.testActionMode=data.mode;").replace('function testState(): string {', '''function readingAction(action: string): void {
        if(action==='file'){root.selected=0;root.activate(true);}
        else if(action==='save')todoApp.saveEditor();
        else if(action==='cancel')todoApp.cancelEditor();
        else if(action==='back'){root.windowEntry='';root.search();}
        else if(action==='launch'){root.selected=0;root.activate(true);}
        else if(action==='switch'){root.selected=0;root.activate(false);}
    }
    function readingState(): string {return JSON.stringify({mode:root.appMode,editor:todoApp.editor,collection:todoApp.collection,draft:todoApp.editing,state:root.appState,picker:root.windowEntry,actionMode:root.testActionMode});}
    function testState(): string {'''))
    config = temp / 'runtjik.conf'
    config.write_text('[plugins]\nenabled=["Browser","Todo"]\n[index]\nrefresh_seconds=1\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config), XDG_STATE_HOME=str(temp/'state'))
    def ipc(*args):
        return subprocess.check_output(['quickshell','ipc','--path',str(home/'qml'),'call','runtjik',*args],text=True,stderr=subprocess.DEVNULL)
    def state(method='readingState'):
        try:
            return json.loads(ipc(method))
        except subprocess.CalledProcessError:
            return {}
    def query(value):
        ipc('test',value)
        wait_for(lambda:state('testState').get('resultQuery')==value)
    with (temp/'launcher.log').open('w') as log:
        process=subprocess.Popen(['quickshell','--path',str(home/'qml'),'--no-duplicate'],stdout=log,stderr=log)
        try:
            wait_for(lambda:state('testState').get('rows'),timeout=20)
            query('/tabs Firefox')
            ipc('readingAction','file')
            wait_for(lambda:state().get('editor')=='edit')
            assert state()['draft']['title']=='Firefox extension documentation'
            assert state()['draft']['reference']=='https://example.org/firefox'
            time.sleep(1.5)
            assert state()['mode']=='Todo', 'Periodic refresh dismissed filing panel'
            ipc('capture',str(temp/'filing.png'))
            time.sleep(.3)
            ipc('readingAction','save')
            wait_for(lambda:state().get('mode')=='')
            query('/reading')
            wait_for(lambda:len(state().get('state',{}).get('tasks',[]))==1)
            assert state()['state']['tasks'][0]['kind']=='reading'
            ipc('capture',str(temp/'reading.png'))
            time.sleep(.3)
            query('Firefox')
            ipc('readingAction','switch')
            wait_for(lambda:bool(state().get('picker')))
            ipc('readingAction','back')
            wait_for(lambda:any(row.get('provider')=='apps' for row in state('testState').get('rows',[])))
            ipc('readingAction','launch')
            wait_for(lambda:state().get('actionMode')=='launch')
            assert 'TypeError' not in (temp/'launcher.log').read_text(), (temp/'launcher.log').read_text()
            print('PASS: filing form, refresh stability, save, Reading list, Enter switches; evidence:',temp)
        finally:
            process.terminate()
            process.wait(timeout=5)
