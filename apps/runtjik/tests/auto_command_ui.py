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
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    function send(data) {', '''
    property int refreshRequests: 0
    function send(data) {
        if(data.op==='activate' && data.id==='/refresh')refreshRequests++;
''').replace('statistics:root.statistics,', '''
                refreshRequests:root.refreshRequests,message:root.message,
                messageVisible:paneMessage.visible,
                messageCenterX:paneMessage.x+paneMessage.width/2,
                messageCenterY:paneMessage.y+paneMessage.height/2,
                paneWidth:paneMessage.parent.width,paneHeight:paneMessage.parent.height,
                gridY:items.y,gridHeight:items.height,
                statistics:root.statistics,'''))
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
            baseline = state()
            for value in ('/ref', '/refresh', 'refresh'):
                before = state()['refreshRequests']
                ipc('test', value)
                wait_for(lambda: state().get('message', '').startswith('Index rebuilt'))
                current = state()
                assert current['resultQuery'] == '', current
                assert current['refreshRequests'] == before + 1, current
                assert current['messageVisible'], current
                assert abs(current['messageCenterX'] - current['paneWidth'] / 2) < 1, current
                assert abs(current['messageCenterY'] - current['paneHeight'] / 2) < 1, current
                assert current['gridY'] == baseline['gridY'], current
                assert current['gridHeight'] == baseline['gridHeight'], current
                if os.environ.get('RUNTJIK_TEST_CAPTURE'):
                    capture = Path(os.environ['RUNTJIK_TEST_CAPTURE'])
                    ipc('capture', str(capture))
                    wait_for(capture.is_file)
                time.sleep(.3)
                assert state()['refreshRequests'] == before + 1, state()
                query('/', '/')
            query('pin', 'pin')  # Other actions still require activation.
            assert [row['id'] for row in state()['rows']] == ['/pin'], state()
            query('', '')
            query('extension documentation', 'extension documentation')  # A sole browser tab is not auto-activated.
            assert len(state()['rows']) == 1, state()
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
            print('PASS: unique menus and refresh auto-enter once; refresh message is centered without grid movement; deletion, ambiguous commands, other actions, and ordinary results do not auto-enter')
        finally:
            process.terminate()
            process.wait(timeout=5)
