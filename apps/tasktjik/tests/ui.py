"""Synthetic Tasktjik window check; never accesses the real authority."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'runtjik/tests'))
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    config = Path(os.environ['XDG_CONFIG_HOME']) / 'tasktjik'
    config.mkdir(parents=True)
    (config / 'tasktjik.conf').write_text('[tasktjik]\nauthority=""\ndirectory="'+str(temp/'tasks')+'"\n')
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'tasktjik.py', home / 'tasktjik.py')
    os.environ['TASKTJIK_HOME'] = str(home)
    def request(value):
        return json.loads(subprocess.check_output(['python3', '-B', str(home/'tasktjik.py'), 'request'], input=json.dumps(value), text=True))
    task = request({'action':'create','fields':{'title':'Synthetic agent task','assignee':'agent:demo','acceptance':['Render task details']}})
    request({'action':'status','id':task['id'],'revision':task['revision'],'actor':'agent:demo','status':'blocked','blockers':[{'reason':'Awaiting synthetic review'}]})
    shell = home/'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    Component.onCompleted:', '''    IpcHandler {
        target: "tasktjik-test"
        function state(): string { return JSON.stringify({tasks:view.stateData.tasks,error:view.errorMessage}); }
        function refresh(): void { root.request({action:'list'}); }
        function capture(path:string): void { view.grabToImage(image=>image.saveToFile(path)); }
    }
    Component.onCompleted:'''))
    def ipc(method, *args):
        return subprocess.check_output(['quickshell','ipc','--path',str(home/'qml'),'call','tasktjik-test',method,*args],text=True,stderr=subprocess.DEVNULL)
    def ready():
        try:
            return len(json.loads(ipc('state')).get('tasks', [])) == 1
        except subprocess.CalledProcessError:
            return False
    with (temp/'tasktjik.log').open('w') as log:
        process = subprocess.Popen(['quickshell','--path',str(home/'qml'),'--no-duplicate'],stdout=log,stderr=log)
        try:
            wait_for(ready, timeout=20)
            assert not json.loads(ipc('state'))['error']
            ipc('refresh')
            wait_for(ready)
            ipc('capture',str(temp/'tasktjik.png'))
            wait_for(lambda:(temp/'tasktjik.png').is_file())
            assert 'ERROR' not in (temp/'tasktjik.log').read_text()
            print('PASS standalone Tasktjik:',temp/'tasktjik.png')
        finally:
            process.terminate()
            process.wait(timeout=5)
