"""Exercise the real reading editor with synthetic demo tabs in a private compositor."""
import json
import os
from pathlib import Path
import shutil
import subprocess

from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'READING'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'READING' for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    subprocess.run(['hyprctl', 'eval', 'hl.monitor({output="READING",mode="1000x680@60",position="-3000x0",scale=1})'], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="READING"}))'], check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('function testState(): string {', '''
        function readingState(): string {
            return JSON.stringify({mode:root.appMode,editor:todoApp.editor,draft:todoApp.draftId,
                collection:todoApp.collection,tasks:root.appState.tasks || [],selected:todoApp.selectedId,
                pending:!!todoApp.pendingSave,message:root.message,query:root.resultQuery,rows:root.results.length});
        }
        function readingAction(action: string): void {
            if(action==='cancel')todoApp.cancelEditor();
            else if(action==='save')todoApp.saveEditor();
            else if(action==='leave')todoApp.exitApp();
        }
        function testState(): string {'''))
    config = temp / 'runtjik.conf'
    config.write_text('[plugins]\nenabled=["Browser","Todo"]\n[theme]\nicon_theme="rose-punk"\n[ui]\nmonitor="READING"\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'), QT_QUICK_CONTROLS_STYLE='Basic')

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'runtjik', *args],
                                       text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('readingState'))
        except subprocess.CalledProcessError:
            return {}

    def capture():
        ipc('test', '/tabs')
        wait_for(lambda: state().get('query') == '/tabs' and state().get('rows') == 3)
        subprocess.run(['wtype', '-s', '150', '-M', 'shift', '-k', 'Return', '-m', 'shift'], check=True)
        try:
            wait_for(lambda: state().get('editor') == 'edit' and state().get('draft'))
        except ValueError:
            raise AssertionError(state())
        assert state()['collection'] == 'reading', state()

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'], stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            wait_for(lambda: json.loads(ipc('status')).get('ready'), timeout=20)
            capture()
            ipc('capture', str(temp / 'reading-editor.png'))
            wait_for(lambda: (temp / 'reading-editor.png').exists())
            ipc('readingAction', 'cancel')
            wait_for(lambda: state().get('editor') == '')
            assert state()['tasks'] == [], state()
            ipc('readingAction', 'leave')
            capture()
            # Hiding and reopening a capture must reopen the editor too.
            ipc('readingAction', 'leave')
            capture()
            ipc('readingAction', 'save')
            wait_for(lambda: state().get('editor') == '' and len(state().get('tasks', [])) == 1)
            saved = state()
            assert not saved['pending'] and not saved['draft'], saved
            assert saved['selected'] == saved['tasks'][0]['id'], saved
            assert saved['tasks'][0]['kind'] == 'reading', saved
            ipc('capture', str(temp / 'reading-saved.png'))
            wait_for(lambda: (temp / 'reading-saved.png').exists())
            print('PASS: Shift+Enter, reading editor, cancel, reopen, save acknowledgement and selected saved item')
            print('Synthetic captures: ' + str(temp))
        finally:
            process.terminate()
            process.wait(timeout=8)
