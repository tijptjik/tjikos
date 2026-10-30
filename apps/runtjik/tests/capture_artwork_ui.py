"""Render capture menus and empty histories using only synthetic fixtures."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import tempfile
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="runtjik-empty-", dir="/dev/shm") as empty_directory, private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    for name in ('todo.py', 'browser.py', 'runtjik.py', 'words.py', 'desktop_plugins.py', 'capture_worker.py', 'capture_countdown.py', 'capture_trash.py'):
        shutil.copy2(app / name, home / name)
    # Empty fixture histories; never inspect the user's capture directories.
    backend = home / 'desktop_plugins.py'
    source = backend.read_text().replace("if self.demo:\n            cache =", "if False:\n            cache =")
    backend.write_text(source)
    config = temp / 'runtjik.conf'
    config.write_text('[plugin_settings.Screenshots]\ndirectory=' + json.dumps(str(Path(empty_directory) / 'shots')) + '\n[plugin_settings.Recorder]\ndirectory=' + json.dumps(str(Path(empty_directory) / 'recordings')) + '\n')
    env = {**os.environ, 'RUNTJIK_HOME': str(home), 'RUNTJIK_DEMO': '1', 'RUNTJIK_CONFIG': str(config)}
    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)
    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}
    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'], env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            for query, name in [('/grab', 'screenshots'), ('/rec', 'recordings'), ('/grab history', 'empty-shots'), ('/rec history', 'empty-recordings')]:
                ipc('test', query)
                wait_for(lambda: state().get('resultQuery') == query)
                rows = state()['rows']
                if 'history' in query:
                    assert rows[0]['empty_state'] and rows[0]['disabled']
                else:
                    expected = ['Region', 'Window', 'Workspace', 'Screen'] if query == '/rec' else ['Region', 'Window', 'Screen']
                    assert [row['name'] for row in rows[:len(expected)]] == expected
                    assert len({r['icon'] for r in rows}) == len(rows)
                    assert all(Path(r['icon']).is_file() for r in rows)
                time.sleep(1.2)
                ipc('capture', str(temp / (name + '.png')))
                wait_for(lambda: (temp / (name + '.png')).exists())
                if 'history' in query:
                    subprocess.run(['wtype', '-k', 'Return'], check=True)
                    wait_for(lambda: state().get('resultQuery') == query.split()[0])
            print('PASS: distinct capture artwork and disabled empty history states', temp)
        finally:
            process.terminate()
            process.wait(timeout=5)
