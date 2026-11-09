"""Synthetic todo app smoke test in a private compositor."""
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
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('function testState(): string {', '''function todoTest(action: string): void {
        if(action==='activate'){root.activate(false);}
        else if(action==='new'){todoApp.selectedId='';todoApp.openEditor();}
        else if(action==='snooze')todoApp.editor='snooze';
        else root.send({op:'app-action',provider:'Todo',action:'create',fields:{title:'Synthetic task',group:'Work',due:'2026-10-30'}});
    }
    function todoState(): string {return JSON.stringify({mode:root.appMode,state:root.appState});}
    function testState(): string {'''))
    config = temp / 'runtjik.conf'
    config.write_text('[theme]\nicon_theme="rose-punk"\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config), XDG_STATE_HOME=str(temp/'state'))
    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home/'qml'), 'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)
    def state(method="todoState"):
        try:
            return json.loads(ipc(method))
        except subprocess.CalledProcessError:
            return {}
    with (temp/'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell','--path',str(home/'qml'),'--no-duplicate'],stdout=log,stderr=log)
        try:
            wait_for(lambda: state('testState').get('rows'), timeout=20)
            ipc('test','todo')
            wait_for(lambda: any(row.get('id') == '/todo' for row in state('testState').get('rows', [])))
            ipc('todoTest','activate')
            wait_for(lambda: state().get('mode') == 'Todo')
            ipc('todoTest','create')
            wait_for(lambda: len(state().get('state',{}).get('tasks',[]))==1)
            ipc('capture',str(temp/'todo.png'))
            time.sleep(.4)
            ipc('todoTest','new')
            ipc('capture',str(temp/'editor.png'))
            time.sleep(.4)
            ipc('todoTest','snooze')
            ipc('capture',str(temp/'snooze.png'))
            time.sleep(.4)
            assert 'TypeError' not in (temp/'launcher.log').read_text()
            assert 'ERROR' not in (temp/'launcher.log').read_text()
            print('PASS todo app, create, editor and snooze; evidence:',temp)
        finally:
            process.terminate()
            process.wait(timeout=5)
    # Notjik todo cards are covered by its native GTK gallery.
