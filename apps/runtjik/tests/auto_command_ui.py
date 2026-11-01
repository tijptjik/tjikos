"""Automatic menu entry through the real UI with synthetic demo data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor(isolate_pointer=True):
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    config = temp / 'runtjik.conf'
    config.write_text('[plugins]\nenabled=["Browser"]\n')
    env = {**os.environ, 'RUNTJIK_HOME': str(home), 'RUNTJIK_DEMO': '1',
           'RUNTJIK_CONFIG': str(config), 'XDG_STATE_HOME': str(temp / 'state')}

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
                                        'call', 'runtjik', *args], env=env, text=True,
                                       stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    def query(value, expected):
        ipc('test', value)
        wait_for(lambda: state().get('resultQuery') == expected)
        time.sleep(.2)
        assert state()['resultQuery'] == expected, state()

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'],
                                   env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            query('tabs', '/tabs ')
            assert all(':tab:' in row['id'] for row in state()['rows']), state()
            query('/tab', '/tab')  # Backspace out without being forced back in.
            query('', '')
            query('/ta', '/tabs ')
            query('', '')
            query('/', '/')  # Ambiguous command suggestions remain suggestions.
            assert len(state()['rows']) > 1, state()
            query('', '')
            query('refresh', 'refresh')  # No automatic side effect.
            assert [row['id'] for row in state()['rows']] == ['/refresh'], state()
            query('', '')
            query('extension documentation', 'extension documentation')  # A sole browser tab is not auto-activated.
            assert len(state()['rows']) == 1, state()
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
            print('PASS: bare tabs and unique prefix auto-enter; deletion, ambiguous commands, actions, and ordinary results do not')
        finally:
            process.terminate()
            process.wait(timeout=5)
