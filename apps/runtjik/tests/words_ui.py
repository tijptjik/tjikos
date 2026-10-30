"""Render synthetic word lookups in an isolated compositor; never call APIs."""
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
    for name in ('todo.py', 'browser.py', 'runtjik.py', 'words.py', 'desktop_plugins.py', 'capture_worker.py', 'capture_countdown.py'):
        shutil.copy2(app / name, home / name)
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('function testState(): string {',
        'function submitWord(): void {root.selected=0;root.activate(false);}\n        function testState(): string {'))
    config = temp / 'runtjik.conf'
    config.write_text('[theme]\nicon_theme="rose-punk"\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'))
    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)
    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}
    def query(text):
        ipc('test', text)
        wait_for(lambda: state().get('resultQuery') == text)
    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'], stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            query('/config plugin_settings.Words')
            assert len(state()['rows']) == 4
            ipc('capture', str(temp / 'word-settings.png'))
            query('?')
            assert all(row['windows'] > 0 for row in state()['rows'])
            query('/running')
            assert state()['rows'] == []
            for command in ('define', 'synonym'):
                query('/' + command + ' serendipity')
                wait_for(lambda: any('Synthetic dictionary' in row['subtitle'] for row in state()['rows']))
                ipc('capture', str(temp / (command + '.png')))
                time.sleep(.3)
            for command in ('/grab history', '/rec history', '!'):
                query(command)
                time.sleep(.3)
                ipc('capture', str(temp / ('gallery.png' if command == '/grab history' else 'recordings.png' if command == '/rec history' else 'commands.png')))
                time.sleep(.2)
            print('PASS: plugin settings, ? processes, removed /running, define and synonym UI', flush=True)
        finally:
            process.terminate()
            process.wait(timeout=5)
            print('Word UI evidence:', temp, flush=True)
