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
    shutil.copy2(app / 'tasktjik-api', home / 'tasktjik-api')
    os.environ['TASKTJIK_HOME'] = str(home)
    def request(value):
        return json.loads(subprocess.check_output([str(home/'tasktjik-api'), 'request'], input=json.dumps(value), text=True))
    task = request({'action':'create','fields':{'title':'Synthetic agent task','assignee':'agent:demo','acceptance':['Render task details']}})
    request({'action':'status','id':task['id'],'revision':task['revision'],'actor':'agent:demo','status':'blocked','blockers':[{'reason':'Awaiting synthetic review'}]})
    component = home/'qml/TodoApp.qml'
    component.write_text(component.read_text().replace('    id: app', '    id: app\n    property bool testHelpVisible: help.visible\n    function captureHelp(path) { help.contentItem.grabToImage(image=>image.saveToFile(path)); }'))
    shell = home/'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    Component.onCompleted:', '''    IpcHandler {
        target: "tasktjik-test"
        function state(): string { return JSON.stringify({tasks:view.stateData.tasks,error:view.errorMessage,editor:view.editor,sections:view.sections,help:view.testHelpVisible}); }
        function refresh(): void { root.request({action:'list'}); }
        function capture(path:string): void { if(view.testHelpVisible) view.captureHelp(path); else view.grabToImage(image=>image.saveToFile(path)); }
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
            def key(*args):
                subprocess.run(['wtype', *args], check=True)
            def current():
                return json.loads(ipc('state'))
            key('n')
            wait_for(lambda: current()['editor'] == 'edit')
            assert not any(current()['sections'].values()), current()
            ipc('capture', str(temp/'editor.png'))
            wait_for(lambda: (temp/'editor.png').is_file())
            for letter, section in [('g','group'),('t','tags'),('d','due'),('r','repeat'),('l','reference'),('m','reminder')]:
                key('-M', 'alt', letter, '-m', 'alt')
                wait_for(lambda: current()['sections'].get(section))
            ipc('capture', str(temp/'expanded.png'))
            wait_for(lambda: (temp/'expanded.png').is_file())
            key('?')
            wait_for(lambda: current()['help'])
            ipc('capture', str(temp/'help.png'))
            wait_for(lambda: (temp/'help.png').is_file())
            key('-k', 'Escape')
            wait_for(lambda: not current()['help'])
            key('-k', 'Escape')
            wait_for(lambda: current()['editor'] == '')
            key('n')
            wait_for(lambda: current()['editor'] == 'edit')
            assert not any(current()['sections'].values()), current()
            key('Synthetic minimal item')
            key('-k', 'Return')
            wait_for(lambda: len(current()['tasks']) == 2)
            assert 'ERROR' not in (temp/'tasktjik.log').read_text()
            print('PASS standalone Tasktjik:',temp/'tasktjik.png')
        finally:
            process.terminate()
            process.wait(timeout=5)
