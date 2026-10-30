"""Native focus and graceful close against synthetic Kitty clients, no keyboard injection."""
import json
import os
from pathlib import Path
import subprocess
import time
from private_compositor import private_compositor
from rust_parity import Backend, fixture


def wait(check):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(.04)
    raise AssertionError('Timed out waiting for synthetic compositor client')


def main():
    with private_compositor():
        root = Path(os.environ['XDG_RUNTIME_DIR']).parent
        state = root / 'native-close'
        state.mkdir()
        env = fixture(state, 0)
        env['HYPRLAND_INSTANCE_SIGNATURE'] = os.environ['HYPRLAND_INSTANCE_SIGNATURE']
        desktop = state / 'data/applications/native-close.desktop'
        desktop.write_text('[Desktop Entry]\nType=Application\nName=Native Close Fixture\n'
                           'Exec=kitty --config NONE --class runtjik-native-close sleep 90\n'
                           'StartupWMClass=runtjik-native-close\nIcon=kitty\n')
        clients = lambda: json.loads(subprocess.check_output(['hyprctl', '-j', 'clients'], env=env))
        own = lambda: [c for c in clients() if c['class'] == 'runtjik-native-close']
        processes = []
        backend = None
        with (root / 'kitty.log').open('w') as log:
            try:
                for i in range(2):
                    processes.append(subprocess.Popen(['kitty', '--config', 'NONE', '-o', 'linux_display_server=wayland',
                                                       '-o', 'confirm_os_window_close=0', '--class', 'runtjik-native-close', '--title', f'Synthetic close {i}',
                                                       'sleep', '90'], env=env, stdout=log, stderr=log))
                wait(lambda: len(own()) == 2)
                backend = Backend('rust', env)
                rows = backend.request(op='query', query='Native Close Fixture')['results']
                entry = next(row for row in rows if row['id'] == 'apps:native-close.desktop')
                assert entry['windows'] == 2, entry
                picker = backend.request(op='activate', id=entry['id'], mode='switch', prepare=True)
                assert picker['kind'] == 'windows' and len(picker['results']) == 2, picker
                address = picker['results'][-1]['id']
                assert backend.request(op='activate', id=entry['id'], mode='switch', address=address)['kind'] == 'activated'
                wait(lambda: json.loads(subprocess.check_output(['hyprctl', '-j', 'activewindow'], env=env)).get('address') == address)
                response = backend.request(op='activate', id=entry['id'], mode='close', address=address)
                assert response['action'] == 'close' and 'graceful close' in response['message'], response
                wait(lambda: len(own()) == 1)
                response = backend.request(op='activate', id=entry['id'], mode='close')
                assert response['action'] == 'close', response
                wait(lambda: not own())
                assert backend.request(op='query', query='Native Close Fixture')['closing'] == []
                assert not Path(f'/proc/{backend.process.pid}/task/{backend.process.pid}/children').read_text().strip()
                print('Native window picker, exact-window focus, targeted graceful close and closing-state cleanup passed')
            finally:
                if backend:
                    backend.close()
                for process in processes:
                    if process.poll() is None:
                        process.terminate()
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == '__main__':
    main()
