"""Render configurable tab favicons with synthetic demo tabs in private Hyprland."""
import json
import os
from pathlib import Path
import shutil
import subprocess

from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor(isolate_pointer=True):
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    home.mkdir()
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace(
        'property string iconSource:', 'readonly property int imageStatus: art.status\n                                property string iconSource:'
    ).replace('function testState(): string {', '''function faviconState(): string {
        return JSON.stringify(Array.from({length:items.count}, (_,i)=> {
            const item=items.itemAtIndex(i);
            return item ? {source:item.iconSource,status:item.imageStatus,marker:item.showStatusMarker} : {};
        }));
    }
    function testState(): string {'''))
    config = temp / 'runtjik.conf'
    env = {**os.environ, 'RUNTJIK_HOME': str(home), 'RUNTJIK_DEMO': '1',
           'RUNTJIK_CONFIG': str(config), 'XDG_STATE_HOME': str(temp / 'state')}

    def ipc(*args):
        return subprocess.check_output(
            ['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'runtjik', *args],
            text=True, stderr=subprocess.DEVNULL, env=env)

    def state(method='testState'):
        try:
            return json.loads(ipc(method))
        except subprocess.CalledProcessError:
            return {}

    for icon in ['overture.png', 'hkgov.png']:
        config.write_text('[plugins]\nenabled=["Browser"]\n'
                          '[plugin_settings.Browser]\n'
                          f"favicon_overrides=[{{domain_regex='^example\\.org$',icon='bundled:{icon}'}}]\n")
        log_path = temp / f'{icon}.log'
        with log_path.open('w') as log:
            process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'],
                                       env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: state().get('rows'), timeout=20)
                ipc('test', '/tabs')
                wait_for(lambda: state().get('resultQuery') == '/tabs')
                rows = state()['rows']
                assert len(rows) == 3, rows
                assert all(not item.get('marker') for item in state('faviconState')), state('faviconState')
                assert all(row['favicon'].endswith('/' + icon) for row in rows), rows
                wait_for(lambda: len(state('faviconState')) == 3 and all(
                    image.get('status') == 1 and image.get('source', '').endswith('/' + icon)
                    for image in state('faviconState')))
                ipc('reopen')
                wait_for(lambda: state('status').get('ready') and state('status').get('visible'))
                screenshot = temp / f'{icon}.png'
                ipc('capture', str(screenshot))
                wait_for(screenshot.exists)
                assert 'Error decoding' not in log_path.read_text(), log_path.read_text()
                print(f'PASS: {icon} loaded in all three tab rows; screenshot: {screenshot}', flush=True)
            finally:
                process.terminate()
                process.wait(timeout=5)
