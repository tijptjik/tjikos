"""Exercise the real QML/Rust boundary in a private compositor using --demo only."""
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import subprocess
import time
from private_compositor import private_compositor
from rust_parity import fixture

APP = Path(__file__).resolve().parents[1]


def main():
    with private_compositor():
        root = Path(os.environ['XDG_RUNTIME_DIR']).parent
        os.environ.update(RUNTJIK_HOME=str(APP), RUNTJIK_DEMO='1', RUNTJIK_BACKEND='rust',
                          XDG_STATE_HOME=str(root / 'state'), RUNTJIK_CONFIG=str(root / 'runtjik.conf'))
        (root / 'runtjik.conf').write_text('[theme]\nicon_theme="Adwaita"\n')
        base = ['quickshell', 'ipc', '--path', str(APP / 'qml'), 'call', 'runtjik']
        def ipc(*args):
            return subprocess.check_output(base + list(args), text=True, stderr=subprocess.DEVNULL)
        def status():
            return json.loads(ipc('status'))
        def wait(check):
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                try:
                    value = check()
                    if value:
                        return value
                except (subprocess.CalledProcessError, ValueError, KeyError):
                    pass
                time.sleep(.03)
            raise AssertionError('Timed out waiting for QML/Rust state')
        def run(action):
            subprocess.run([str(APP / 'bin/runtjik'), action], check=True)
        try:
            run('show')
            wait(lambda: status()['ready'] and status()['visible'])
            initial = status()
            assert initial['indexed'] == 15 and initial['total'] == 15, initial
            pid = initial['pid']
            for query, expected in [('fire', 1), ('todo', 1), ('zzzzzz', 0), ('', 15), ('/define test', 2), ('/cliphist', 3), ('/grab', 4), ('/grab history', 9), ('/rec history', 9), ('/todo', 0), ('/reading', 0)]:
                ipc('test', query)
                wait(lambda: json.loads(ipc('testState'))['resultQuery'] == query)
                rows = json.loads(ipc('testState'))['rows']
                assert len(rows) == expected, (query, rows)
            countdown_result = root / 'countdown-complete'
            env = {**os.environ, 'RUNTJIK_COUNTDOWN_SECONDS':'1', 'RUNTJIK_COUNTDOWN_SUCCESS':str(countdown_result)}
            countdown = subprocess.run(['quickshell', '--path', str(APP / 'qml/countdown')], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=10)
            assert countdown.returncode == 0 and countdown_result.exists(), countdown.stdout
            ipc('test', '')
            wait(lambda: json.loads(ipc('testState'))['resultQuery'] == '')
            image = root / 'rust-demo.png'
            ipc('capture', str(image))
            wait(image.exists)
            # Demo mode intentionally exits on dismissal. Warm behavior uses
            # synthetic desktop files in a separate private state directory.
            run('quit')
            os.environ.pop('RUNTJIK_DEMO', None)
            warm = root / 'warm'
            warm.mkdir()
            os.environ.update(fixture(warm, 5))
            run('show')
            pid = status()['pid']
            for _ in range(3):
                run('close')
                assert status()['pid'] == pid and not status()['visible'] and status()['ready']
                run('show')
                assert status()['pid'] == pid and status()['visible'] and status()['ready']
            run('quit')
            with ThreadPoolExecutor(max_workers=4) as pool:
                list(pool.map(lambda _: run('show'), range(4)))
            assert status()['ready'] and status()['visible']
            print('Real QML search, empty results, provider responses, screenshot, warm reopen and concurrent cold opens passed')
            print('Synthetic screenshot: ' + str(image))
        finally:
            run('quit')


if __name__ == '__main__':
    main()
