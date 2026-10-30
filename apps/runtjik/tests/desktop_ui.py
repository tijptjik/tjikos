"""Desktop plugin UI and real capture tools against an isolated --demo scene only."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from unittest.mock import patch

from private_compositor import private_compositor, wait_for
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import desktop_plugins as desktop
import runtjik

app = Path(__file__).resolve().parents[1]
with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    home.mkdir()
    shutil.copytree(app / 'qml', home / 'qml')
    for name in ('todo.py', 'runtjik.py', 'words.py', 'desktop_plugins.py', 'capture_worker.py', 'capture_countdown.py'):
        shutil.copy2(app / name, home / name)
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('function testState(): string {', 'function protectDemo(): void {panel.pinned=true;}\n        function testState(): string {'))
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
        fixture_window = None
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            for command in ('/cliphist', '/rec', '/screenrecord', '/grab', '/shot', '/screenshot'):
                query(command)
                assert state()['rows'], command
                ipc('capture', str(temp / (command[1:] + '.png')))
                time.sleep(.2)
            for name in ('Clipboard', 'Recorder', 'Screenshots'):
                query('/config plugin_settings.' + name)
                assert state()['rows']
            # Real tools see only the nested compositor and its synthetic --demo launcher.
            settings = runtjik.config()
            settings['plugin_settings']['Screenshots'].update(directory=str(temp / 'screenshots'), copy_to_clipboard=False)
            settings['plugin_settings']['Recorder'].update(directory=str(temp / 'recordings'), audio=False)
            query('/grab')
            ipc('protectDemo')
            monitor = desktop.hypr('monitors')[0]
            shot = desktop.Screenshots()
            row = shot.query('/grab screen', settings)[0]
            result = shot.activate(runtjik.Entry(**row), settings, 'launch')
            assert result['close_launcher']
            from PIL import Image
            image = next((temp / 'screenshots').glob('*.png'))
            with Image.open(image) as screenshot:
                assert screenshot.width > 0 and screenshot.height > 0
            with patch.object(desktop, 'selection', return_value='0,0 200x120'):
                shot.capture({'capture': 'region'}, settings['plugin_settings']['Screenshots'])
            fixture_window = subprocess.Popen(['kitty', '--config', 'NONE', '--class', 'runtjik-capture-fixture',
                                               '--title', 'Synthetic capture fixture', '/bin/sh', '-c',
                                               'printf "Synthetic capture test\\n"; sleep 60'], stdout=log, stderr=log)
            wait_for(lambda: any(c.get('class') == 'runtjik-capture-fixture' for c in desktop.hypr('clients')))
            row = next(row for row in shot.query('/grab window', settings) if row['name'] == 'Synthetic capture fixture')
            shot.activate(runtjik.Entry(**row), settings, 'launch')
            assert len(list((temp / 'screenshots').glob('*.png'))) == 3
            fixture_window.terminate()
            fixture_window.wait(timeout=5)
            fixture_window = None
            recorder = desktop.Recorder()
            row = recorder.query('/rec screen', settings)[0]
            result = recorder.activate(runtjik.Entry(**row), settings, 'launch')
            assert result['close_launcher'] and desktop.recorder_request()
            time.sleep(.7)
            query('/rec')
            time.sleep(.7)
            desktop.recorder_request('stop')
            wait_for(lambda: desktop.recorder_request() is None, timeout=25)
            video = next((temp / 'recordings').glob('*.mp4'))
            info = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_streams', '-of', 'json', str(video)], text=True))
            assert any(stream['codec_type'] == 'video' and stream['width'] > 0 for stream in info['streams'])
            row = recorder.query('/rec workspace', settings)[0]
            recorder.activate(runtjik.Entry(**row), settings, 'launch')
            time.sleep(.5)
            subprocess.run(['hyprctl', 'dispatch', 'hl.dsp.focus({workspace="' + str(monitor['activeWorkspace']['id'] + 1) + '"})'], check=True, stdout=subprocess.DEVNULL)
            wait_for(lambda: desktop.recorder_request() is None, timeout=25)
            # Fixture database only: no reads or changes to the user's clipboard history.
            subprocess.run(['cliphist', 'store'], input=b'Synthetic desktop-plugin clipboard fixture', check=True)
            row = desktop.Clipboard().query('/cliphist synthetic', settings)[0]
            assert row['data'].get('clip_id')
            desktop.Clipboard().activate(runtjik.Entry(**row), settings, 'launch')
            assert subprocess.check_output(['wl-paste', '--no-newline']) == b'Synthetic desktop-plugin clipboard fixture'
            print('PASS: plugin/alias/settings UI; real isolated screenshot, video finalization, workspace stop, clipboard round-trip', flush=True)
        finally:
            if desktop.recorder_request():
                desktop.recorder_request('stop')
                wait_for(lambda: desktop.recorder_request() is None, timeout=25)
            if fixture_window is not None:
                fixture_window.terminate()
                fixture_window.wait(timeout=5)
            process.terminate()
            process.wait(timeout=5)
            print('Desktop plugin evidence:', temp, flush=True)
