"""Exercise the stats pane in a real compositor with synthetic --demo activity."""
import json
import os
from pathlib import Path
import subprocess
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor():
    # The parent can hide its nested output; keep a dedicated test monitor alive.
    subprocess.run(['hyprctl', 'output', 'create', 'headless'], check=True, stdout=subprocess.DEVNULL)
    monitor = wait_for(lambda: next((m['name'] for m in json.loads(
        subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))
        if m['name'].startswith('HEADLESS')), None), timeout=20)
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor=' + json.dumps(monitor) + '}))'],
                   check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    config = temp / 'runtjik.conf'
    config.write_text('[ui]\nmonitor=' + json.dumps(monitor) + '\n[theme]\nicon_theme="rose-punk"\n')
    os.environ.update(RUNTJIK_HOME=str(app), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'), QT_QUICK_CONTROLS_STYLE='Basic')

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(app / 'qml'),
                                       'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(app / 'qml'), '--no-duplicate'],
                                   stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            wait_for(lambda: json.loads(ipc('status')).get('ready') and json.loads(ipc('status')).get('monitor'), timeout=20)
            ipc('test', 'stats')
            wait_for(lambda: state().get('resultQuery') == 'stats')
            assert state()['rows'][0]['id'] == '/stats', state()
            subprocess.run(['wtype', '-k', 'Return'], check=True)
            try:
                wait_for(lambda: state().get('statsVisible'))
            except ValueError:
                print('Stats UI state:', state(), ipc('status'))
                raise
            snapshot = state()
            assert snapshot['resultQuery'] == '/stats', snapshot
            assert snapshot['statsWeeks'] == 26, snapshot
            assert len(snapshot['statistics']['calendar']) == 182
            assert len(snapshot['statistics']['metrics']) == 5
            ipc('capture', str(temp / 'stats.png'))
            wait_for(lambda: (temp / 'stats.png').is_file())
            subprocess.run(['wtype', '-k', 'Escape'], check=True)
            wait_for(lambda: state().get('resultQuery') == '' and not state().get('statsVisible'))
            ipc('test', '/stats')
            try:
                wait_for(lambda: state().get('statsVisible'))
            except ValueError:
                print('Stats UI state:', state(), ipc('status'))
                raise
            # Narrow layouts retain seven rows and reduce week columns.
            config.write_text('[ui]\nmonitor=' + json.dumps(monitor) + '\nwidth=480\nheight=500\n[stats]\nenabled=false\n[theme]\nicon_theme="rose-punk"\n')
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
            assert 'ReferenceError' not in (temp / 'launcher.log').read_text()
            assert 'ERROR' not in (temp / 'launcher.log').read_text()
        finally:
            process.terminate()
            process.wait(timeout=5)
        process = subprocess.Popen(['quickshell', '--path', str(app / 'qml'), '--no-duplicate'],
                                   stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('resultQuery') == '')
            ipc('test', '/stats')
            try:
                wait_for(lambda: state().get('statsVisible'))
            except ValueError:
                print('Stats UI state:', state(), ipc('status'))
                raise
            assert 4 <= state()['statsWeeks'] < 26, state()
            wait_for(lambda: json.loads(ipc('status')).get('ready') and json.loads(ipc('status')).get('monitor'), timeout=20)
            ipc('capture', str(temp / 'stats-narrow.png'))
            wait_for(lambda: (temp / 'stats-narrow.png').is_file())
            subprocess.run(['wtype', '-k', 'Next'], check=True)
            wait_for(lambda: state().get('statsScrollY', 0) > 0)
            ipc('capture', str(temp / 'stats-narrow-scrolled.png'))
            wait_for(lambda: (temp / 'stats-narrow-scrolled.png').is_file())
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
            assert 'ReferenceError' not in (temp / 'launcher.log').read_text()
            assert 'ERROR' not in (temp / 'launcher.log').read_text()
            print('PASS stats search, activation, seven-row graph, Escape and narrow layout:', temp)
        finally:
            process.terminate()
            process.wait(timeout=5)
