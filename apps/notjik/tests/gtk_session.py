"""Private notification authority on the current compositor, silently on workspace 7."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
if '--inside' not in sys.argv:
    raise SystemExit(subprocess.call(['dbus-run-session', '--config-file', str(Path(__file__).resolve().parents[2] / 'bartjik/tests/bus.conf'), '--', sys.executable, __file__, '--inside']))


def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)


with tempfile.TemporaryDirectory(prefix='notjik-native-session-') as directory:
    root = Path(directory)
    config = root / 'config.json'
    config.write_text(json.dumps({'theme': 'rose-punk', 'grouping': {'enabled': False},
                                  'placement': {'monitors': 'all', 'edge': 'b', 'position': 'r'}}))
    env = {**os.environ, 'NOTJIK_HOME': str(APP / 'dist'), 'NOTJIK_CONFIG': str(config),
           'NOTJIK_STATE_DIR': str(root / 'state'), 'NOTJIK_TELEGRAM_DIR': str(root / 'telegram'),
           'NOTJIK_SOCKET': str(root / 'notice.sock'), 'XDG_CACHE_HOME': str(root / 'cache'),
           'TJIK_GTK_WINDOWED': '1', 'GTK_A11Y': 'none', 'GIO_USE_VFS': 'local', 'SDL_AUDIODRIVER': 'dummy'}
    rule = f'notjik_acceptance_{os.getpid()}'
    result = hypr('eval', f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",float=true,no_initial_focus=true,no_focus=true,match={{initial_title="^Notjik notifications$"}}}})')
    assert 'error' not in result.lower(), result
    daemon = None
    placed = False

    def request(value):
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(3)
            client.connect(env['NOTJIK_SOCKET'])
            client.sendall(json.dumps(value).encode() + b'\n')
            response = json.loads(client.makefile().readline())
            assert response.get('ok'), response
            return response

    def wait(predicate):
        global placed
        until = time.monotonic() + 12
        while time.monotonic() < until:
            assert daemon.poll() is None, (root / 'daemon.log').read_text()[-1500:]
            for client in json.loads(hypr('-j', 'clients')):
                if client.get('pid') == daemon.pid:
                    assert client['workspace']['id'] == 7, client
                    assert json.loads(hypr('-j', 'activewindow')).get('address') != client['address']
                    placed = True
            try:
                value = predicate()
                if value:
                    return value
            except (OSError, ValueError):
                pass
            time.sleep(.05)
        raise AssertionError('Private notification fixture timed out')

    try:
        with (root / 'daemon.log').open('w') as log:
            daemon = subprocess.Popen([str(APP / 'dist/bin/notjik-gtk'), '--serve'], env=env, stdout=log, stderr=log)
            wait(lambda: request({'op': 'status'}).get('monitors'))
            duplicate = subprocess.run([str(APP / 'dist/bin/notjik-gtk'), '--serve'], env=env, capture_output=True, text=True, timeout=5)
            assert duplicate.returncode != 0 and 'already has an owner' in duplicate.stderr
            request({'op': 'pause'})
            notice = {'id': 'native-fixture', 'title': 'Synthetic acceptance', 'app': 'notjik demo',
                      'timeout': 0, 'transient': True, 'data': {'demo': True}}
            first = request({'op': 'send', 'notice': notice})['id']
            assert request({'op': 'send', 'notice': {**notice, 'title': 'Replacement'}})['id'] == first
            assert len(request({'op': 'list'})['notices']) == 1
            request({'op': 'resume'})
            wait(lambda: placed)
            request({'op': 'dismiss', 'id': first})
            wait(lambda: request({'op': 'status'})['queued'] == 0)
            print('PASS: private single authority, stable replacement IDs, pause/resume and dismissal on workspace 7')
    finally:
        if daemon is not None and daemon.poll() is None:
            daemon.terminate()
            try:
                daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait(timeout=5)
        result = hypr('eval', f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')
        assert 'error' not in result.lower(), result
