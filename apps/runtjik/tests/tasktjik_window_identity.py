"""Match Tasktjik's real Wayland window against its desktop entry, in isolation."""
import json
import os
from pathlib import Path
import shutil
import subprocess

from private_compositor import private_compositor, wait_for
from rust_parity import Backend, fixture


TASKTJIK = Path(os.environ.get('TASKTJIK_TEST_HOME', Path(__file__).resolve().parents[2] / 'tasktjik'))


def main():
    with private_compositor(isolate_pointer=True):
        root = Path(os.environ['XDG_RUNTIME_DIR']).parent
        home = root / 'tasktjik'
        shutil.copytree(TASKTJIK / 'qml', home / 'qml')
        # No real authority, tasks, or credentials enter this renderer.
        api = home / 'tasktjik-api'
        api.write_text('#!/usr/bin/env python3\nimport json, sys\njson.load(sys.stdin)\n'
                       'print(json.dumps({"tasks": [], "groups": [], "today": "2026-11-03"}))\n')
        api.chmod(0o700)
        shell = home / 'qml/shell.qml'
        source = shell.read_text()
        state = root / 'launcher'
        state.mkdir()
        env = fixture(state, 0)
        env.update(HYPRLAND_INSTANCE_SIGNATURE=os.environ['HYPRLAND_INSTANCE_SIGNATURE'],
                   TASKTJIK_HOME=str(home), QT_QUICK_CONTROLS_STYLE='Basic')
        shutil.copy2(TASKTJIK / 'examples/tasktjik.desktop',
                     state / 'data/applications/tasktjik.desktop')
        backend = Backend('rust', env)

        def clients():
            return json.loads(subprocess.check_output(['hyprctl', '-j', 'clients'], env=env))

        def row():
            rows = backend.request(op='query', query='Tasktjik')['results']
            return next(item for item in rows if item['id'] == 'apps:tasktjik.desktop')

        try:
            for identity in (False, True):
                # Reproduce the previously installed shell before checking the fix.
                shell.write_text(source if identity else source.replace('//@ pragma AppId tasktjik\n', ''))
                with (root / f'tasktjik-{identity}.log').open('w') as log:
                    process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')],
                                               env=env, stdout=log, stderr=log)
                    try:
                        window = wait_for(lambda: next((c for c in clients()
                                                        if c['pid'] == process.pid and c['mapped']), None))
                        if not identity:
                            assert window['class'] != 'tasktjik', window
                            assert row()['windows'] == 0, row()
                            print('Reproduced: missing AppId gives class', window['class'],
                                  'and Runtjik windows=0', flush=True)
                            continue
                        assert window['class'] == 'tasktjik', window
                        assert row()['windows'] == 1, row()
                        running = backend.request(op='query', query='?Tasktjik')['results']
                        assert any(item['id'] == 'apps:tasktjik.desktop' for item in running), running
                        picker = backend.request(op='activate', id=row()['id'], mode='switch', prepare=True)
                        assert picker['kind'] == 'windows' and len(picker['results']) == 1, picker
                        assert picker['results'][0]['id'] == window['address'], picker
                        print('PASS: Tasktjik identity, running query and window picker')
                    finally:
                        process.terminate()
                        process.wait(timeout=10)
                        wait_for(lambda: not any(c['pid'] == process.pid for c in clients()))
        finally:
            backend.close()


if __name__ == '__main__':
    main()
