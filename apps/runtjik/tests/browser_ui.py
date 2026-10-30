"""Render only synthetic browser fixtures in a private Hyprland compositor."""
import json
import base64
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
    for path in app.glob('*.py'):
        shutil.copy2(path, home / path.name)
    provider = home / 'browser.py'
    icon = 'data:image/svg+xml;base64,' + base64.b64encode(b'<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="6" fill="#ff53a6"/><circle cx="16" cy="16" r="8" fill="#191724"/></svg>').decode()
    provider.write_text(provider.read_text().replace("'url': 'https://example.org/firefox'", "'url': 'https://example.org/firefox', 'favicon': " + repr(icon)).replace("'Firefox extension documentation'", "'Firefox extension documentation with a long title that wraps across two lines for alignment testing'").replace("'https://example.org/hyprland'", "'about:blank'"))
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('scrollY:items.contentY,rows:',
                                               'scrollY:items.contentY,heading:resultsHeading.visible,columns:root.gridColumns,list:root.listMode,rows:'))
    config = temp / 'runtjik.conf'
    config.write_text('[plugins]\nenabled=["Browser"]\n[plugin_settings.Browser]\ncolumns=3\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'))

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
            for query, count, is_list in [('/browser', 2, False), ('/zen HYPRLAND', 1, False),
                                          ('/tabs', 3, True), ('/tabs Firefox', 1, True), ('Firefox extension', 1, False)]:
                ipc('test', query)
                wait_for(lambda: state().get('resultQuery') == query)
                result = state()
                assert len(result['rows']) == count, result
                assert result['list'] == is_list, result
                assert not result['heading'], result
                if query.startswith('/browser') or query.startswith('/zen'):
                    assert result['columns'] == 3, result
                if query == 'Firefox extension':
                    assert result['rows'][0]['favicon'].startswith('data:image/'), result
                time.sleep(0.2)
                ipc('capture', str(temp / (('general' if not query.startswith('/') else 'tabs' if is_list else 'windows') + ('-filtered.png' if ' ' in query else '.png'))))
            time.sleep(0.2)
            print('PASS: browser grid, aliases, tab list, title filtering')
        finally:
            process.terminate()
            process.wait(timeout=5)
            print('Synthetic browser UI evidence:', temp)
