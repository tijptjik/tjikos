"""Single-result activation and independent settings in the rendered demo UI."""
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
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    function send(data) {', '''
    property var activations: []
    function send(data) {
        // App activation first checks for existing windows, then sends the launch.
        if(data.op==='activate' && !data.prepare)activations=activations.concat([data.id]);
''').replace('statistics:root.statistics,', 'activations:root.activations,statistics:root.statistics,'))
    config = temp / 'runtjik.conf'
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

    def query(value, expected=None):
        ipc('test', value)
        wait_for(lambda: state().get('resultQuery') == (value if expected is None else expected))
        time.sleep(.25)

    for applications, commands in ((True, True), (False, True), (True, False), (False, False)):
        # The first case intentionally omits the switches to exercise defaults.
        switches = '' if applications and commands else (
            f'[ui]\nauto_launch_applications={str(applications).lower()}\n'
            f'auto_launch_commands={str(commands).lower()}\n')
        config.write_text(switches + '[plugins]\nenabled=["Browser"]\n')
        with (temp / 'launcher.log').open('w') as log:
            process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'],
                                       env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: state().get('rows'), timeout=20)
                query('calculator')
                current = state()
                assert len(current['rows']) == 1, current
                assert current['activations'].count('apps:org.gnome.Calculator.desktop') == int(applications), current
                query('calculato')  # Deleting still leaves one app; never relaunch.
                assert state()['activations'] == current['activations'], state()
                query('')
                query('web browser')  # Multiple applications remain manual.
                assert len(state()['rows']) > 1, state()
                assert state()['activations'] == current['activations'], state()
                query('')
                query('tabs', '/tabs ' if commands else 'tabs')
                assert state()['activations'].count('/tabs') == int(commands), state()
                query('')
                query('extension documentation')  # A sole browser tab remains manual.
                assert len(state()['rows']) == 1, state()
                assert len(state()['activations']) == int(applications) + int(commands), state()
                assert 'TypeError' not in (temp / 'launcher.log').read_text()
            finally:
                process.terminate()
                process.wait(timeout=5)
    print('PASS: sole applications activate once; independent defaults and all four settings combinations work')
