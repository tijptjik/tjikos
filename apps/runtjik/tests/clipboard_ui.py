"""Synthetic clipboard image previews and imv directory navigation on private Wayland."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(app))
import desktop_plugins as desktop
import runtjik

with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    images = temp / 'images'
    images.mkdir()
    for name, color in [('a.png', 'pink'), ('b.png', 'blue')]:
        subprocess.run(['magick', '-size', '320x180', 'xc:' + color, str(images / name)], check=True)
    subprocess.run(['cliphist', 'store'], input=(images / 'a.png').read_bytes(), check=True)
    provider = desktop.Clipboard()
    settings = runtjik.config()
    def preview():
        rows = provider.query('/cliphist', settings)
        return next((row for row in rows if row['data'].get('thumbnail')), None)
    row = wait_for(preview)
    assert Path(row['data']['thumbnail']).stat().st_mode & 0o777 == 0o600
    provider.activate(runtjik.Entry(**row), settings, 'launch')
    assert subprocess.check_output(['wl-paste', '--no-newline']) == (images / 'a.png').read_bytes()
    provider.executor.shutdown()
    provider.preview_dir.cleanup()
    config = temp / 'hyprland.lua'
    with config.open('a') as stream:
        stream.write('\nhl.window_rule({name="image-viewer-lightbox",float=true,center=true,size={"75%","80%"},match={class="^(org.gnome.Loupe|imv)$"}})\n')
    subprocess.run(['hyprctl', 'reload'], check=True, stdout=subprocess.DEVNULL)
    viewer = subprocess.Popen(['imv', '-n', str(images / 'b.png'), str(images)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        def client():
            return next((c for c in desktop.hypr('clients') if c['class'] == 'imv'), None)
        wait_for(lambda: client() and 'b.png' in client()['title'])
        assert client()['floating']
        subprocess.run(['imv-msg', str(viewer.pid), 'prev'], check=True)
        wait_for(lambda: 'a.png' in client()['title'])
        subprocess.run(['imv-msg', str(viewer.pid), 'next'], check=True)
        wait_for(lambda: 'b.png' in client()['title'])
    finally:
        viewer.terminate()
        viewer.wait(timeout=5)
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    for name in ('todo.py', 'browser.py', 'runtjik.py', 'words.py', 'desktop_plugins.py', 'capture_worker.py', 'capture_countdown.py', 'capture_trash.py'):
        shutil.copy2(app / name, home / name)
    conf = temp / 'runtjik.conf'
    conf.write_text('')
    env = {**os.environ, 'RUNTJIK_HOME': str(home), 'RUNTJIK_DEMO': '1', 'RUNTJIK_CONFIG': str(conf)}
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
            ipc('test', '/cliphist')
            wait_for(lambda: any(row.get('thumbnail') for row in state().get('rows', [])))
            time.sleep(.3)
            ipc('capture', str(temp / 'clipboard.png'))
            wait_for(lambda: (temp / 'clipboard.png').exists())
            print('PASS: private image clipboard round-trip, inline previews, imv floating and next/previous', temp)
        finally:
            process.terminate()
            process.wait(timeout=5)
