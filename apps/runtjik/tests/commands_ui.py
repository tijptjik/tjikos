"""Find slash commands through general search using synthetic launcher data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
from private_compositor import private_compositor, wait_for

app = Path(os.environ.get('RUNTJIK_TEST_HOME', Path(__file__).resolve().parents[1]))
with private_compositor():
    if os.environ.get('TASKTJIK_TEST_HOME'):
        os.environ['TASKTJIK_HOME'] = os.environ['TASKTJIK_TEST_HOME']
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    for path in app.glob('*.py'):
        shutil.copy2(path, home / path.name)
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('resultQuery:root.resultQuery,scrollY:',
        'appMode:root.appMode,editor:todoApp.editor,resultQuery:root.resultQuery,scrollY:'))
    config = temp / 'runtjik.conf'
    config.write_text('[theme]\nicon_theme="rose-punk"\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'), QT_QUICK_CONTROLS_STYLE='Basic')

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
                                       'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'],
                                   stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            subprocess.run(['wtype', 'todo'], check=True)
            wait_for(lambda: state().get('resultQuery') == 'todo')
            assert state()['rows'][0]['id'] == '/todo', state()
            ipc('capture', str(temp / 'search.png'))
            wait_for(lambda: (temp / 'search.png').is_file())
            subprocess.run(['wtype', '-k', 'Return'], check=True)
            wait_for(lambda: state().get('appMode') == 'Todo')
            subprocess.run(['wtype', 'n'], check=True)
            wait_for(lambda: state().get('editor') == 'edit')
            ipc('capture', str(temp / 'editor.png'))
            wait_for(lambda: (temp / 'editor.png').is_file())
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
            assert 'ERROR' not in (temp / 'launcher.log').read_text()
            print('PASS general todo search, Enter opens Todo, N opens editor:', temp)
        finally:
            process.terminate()
            process.wait(timeout=5)
