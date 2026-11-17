"""Real Reading layer focus recovery and shortcuts; synthetic demo state only."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import struct
import zlib

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP.parents[1] / 'packages/tjik-gtk/scripts'))
from display import rendering_display
from private_compositor import wait_for

with rendering_display():
    root = Path(os.environ['XDG_RUNTIME_DIR']).parent
    config = root / 'reading.conf'
    config.write_text('[plugins]\nenabled=["Todo"]\n[ui]\nclose_on_outside_click=false\nauto_launch_applications=false\nauto_launch_commands=false\n')
    env = {**os.environ, 'RUNTJIK_HOME': str(APP / 'dist'), 'RUNTJIK_CONFIG': str(config),
           'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache'),
           'GTK_A11Y': 'none', 'GSK_RENDERER': 'cairo'}
    env.pop('TJIK_GTK_WINDOWED', None)
    binary = APP / 'dist/runtjik-gtk'

    def run(action, *args):
        return json.loads(subprocess.check_output([str(binary), action, *args, '--demo'], env=env, text=True) or '{}')

    def state():
        return run('status')

    def key(*args):
        subprocess.run(['wtype', *args], env=env, check=True)
        time.sleep(.2)

    pointer = None
    try:
        run('show')
        wait_for(lambda: state().get('ready') and not state()['busy'])
        for n in range(5 if os.environ.get("READING_SCREENSHOT_DIR") else 2):
            run('test-request', json.dumps({'op': 'app-action', 'provider': 'Todo', 'action': 'create',
                'fields': {'kind': 'reading', 'title': ['Designing quieter interfaces', 'A field guide to Hong Kong neighbourhoods', 'Small tools for a slower internet', 'Notes on maps, typography and everyday navigation', 'The weekend reading list'][n], 'group': 'All',
                           'reference': f'https://example.test/{n}'}}))
            wait_for(lambda: not state()['busy'])
        if os.environ.get('READING_SCREENSHOT_DIR'):
            run('test-query', '/todo')
            wait_for(lambda: state()['app_state'].get('gtk_view') == 'tasks' and not state()['busy'])
            time.sleep(.4)
            output = Path(os.environ['READING_SCREENSHOT_DIR'])
            output.mkdir(parents=True, exist_ok=True)
            subprocess.run(['hyprctl', 'dismissnotify'], check=True, stdout=subprocess.DEVNULL)
            layers = json.loads(subprocess.check_output(['hyprctl', '-j', 'layers'], text=True))
            bounds = next(c for monitor in layers.values() for rows in monitor['levels'].values() for c in rows if c['namespace'] == 'runtjik-gtk')
            subprocess.run(['grim', '-g', f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}', str(output / 'tasks-compact.png')], check=True)
        run('test-query', '/reading')
        wait_for(lambda: state()['app_state'].get('gtk_view') == 'reading' and not state()['busy'])
        if os.environ.get('READING_SCREENSHOT_DIR'):
            output = Path(os.environ['READING_SCREENSHOT_DIR'])
            output.mkdir(parents=True, exist_ok=True)
            subprocess.run(['hyprctl', 'dismissnotify'], check=True, stdout=subprocess.DEVNULL)
            time.sleep(.4)
            layers = json.loads(subprocess.check_output(['hyprctl', '-j', 'layers'], text=True))
            bounds = next(c for monitor in layers.values() for rows in monitor['levels'].values() for c in rows if c['namespace'] == 'runtjik-gtk')
            subprocess.run(['grim', '-g', f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}', str(output / 'reading-list.png')], check=True)
            print('Reading screenshot:', output / 'reading-list.png')
        run('test-response', json.dumps({'kind': 'app-state', 'app_state': {'message': 'Opened reading reference'}}))
        assert state()['message'] == '', 'Host must not repeat routine Tasktjik statuses'
        key('-k', 'Return')
        wait_for(lambda: not state()['busy'])
        assert state()['app_state'].get('gtk_view') == 'reading'
        # Reproduce a pinned/open Reading panel after handing focus to another app.
        run('outside-click')
        assert state()['visible']
        protocol = APP / 'tests/virtual_pointer.xml'
        subprocess.run(['wayland-scanner', 'client-header', str(protocol), str(root / 'pointer.h')], check=True)
        subprocess.run(['wayland-scanner', 'private-code', str(protocol), str(root / 'protocol.c')], check=True)
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True).split()
        subprocess.run(['cc', str(APP / 'tests/pointer.c'), str(root / 'protocol.c'), '-I' + str(root), '-o', str(root / 'pointer'), *flags], check=True)
        pointer = subprocess.Popen([str(root / 'pointer'), 'GTK'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

        def point(command):
            pointer.stdin.write(command + '\n')
            pointer.stdin.flush()
            assert pointer.stdout.readline().strip() == 'ok'

        layers = json.loads(subprocess.check_output(['hyprctl', '-j', 'layers'], text=True))
        bounds = next(c for monitor in layers.values() for rows in monitor['levels'].values() for c in rows if c['namespace'] == 'runtjik-gtk')
        if os.environ.get('READING_SCREENSHOT_DIR'):
            point(f'm {int((bounds["x"] + 118) * 10)} {int((bounds["y"] + 154) * 10000 / 680)}')
            point('b 1'); point('b 0')
            point('m 0 0')
            time.sleep(.4)
            subprocess.run(['grim', '-g', f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}', str(output / 'reading-grid.png')], check=True)
            point(f'm {int((bounds["x"] + 55) * 10)} {int((bounds["y"] + 154) * 10000 / 680)}')
            point('b 1'); point('b 0')
            time.sleep(.2)
        point(f'm {int((bounds["x"] + 90) * 10)} {int((bounds["y"] + 260) * 10000 / 680)}')
        point('b 1'); point('b 0')
        time.sleep(.2)
        key('-M', 'shift', '-k', 'Return', '-m', 'shift')
        wait_for(lambda: state()['app_state'].get('gtk_view') == 'editor')
        # An external Wayland clipboard owner exercises real image transfer.
        def chunk(kind, data):
            return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
        png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', 1, 1, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(b'\x00\x14\x28\x3c\xff')) + chunk(b'IEND', b'')
        images_before = sum(len(t.get('images') or []) for t in state()['app_state']['tasks'])
        subprocess.run(['wl-copy', '--type', 'image/png'], input=png, env=env, check=True)
        key('-M', 'ctrl', '-k', 'v', '-m', 'ctrl')
        wait_for(lambda: sum(len(t.get('images') or []) for t in state()['app_state']['tasks']) == images_before + 1 and not state()['busy'])
        assert state()['app_state'].get('gtk_view') == 'editor', 'Image paste must keep the editor open'
        key('-k', 'Escape')
        wait_for(lambda: state()['app_state'].get('gtk_view') == 'reading')
        key('-k', 's')
        wait_for(lambda: state()['app_state'].get('gtk_view') == 'snooze')
        key('-k', 'Escape')
        wait_for(lambda: state()['app_state'].get('gtk_view') == 'reading')
        done_before = sum(task.get('done') is True for task in state()['app_state']['tasks'])
        key('-k', 'd')
        wait_for(lambda: sum(task.get('done') is True for task in state()['app_state']['tasks']) == done_before + 1 and not state()['busy'])
        before = len(state()['app_state']['tasks'])
        key('-M', 'ctrl', '-k', 'Return', '-m', 'ctrl')
        wait_for(lambda: len(state()['app_state']['tasks']) == before - 1 and not state()['busy'])
        print('PASS: real layer click reclaims input, Shift+Enter edits, Ctrl+V attaches clipboard image, S opens snooze, D marks read, Escape returns, Ctrl+Enter deletes')
    finally:
        if pointer:
            pointer.terminate()
            pointer.wait(timeout=5)
        run('quit')
