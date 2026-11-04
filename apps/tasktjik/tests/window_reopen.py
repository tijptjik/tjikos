"""Closing and reopening must reuse the existing shell and preserve its state."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='tasktjik-window-reopen-') as directory:
    root = Path(directory)
    home = root / 'app'
    shutil.copytree(APP / 'qml', home / 'qml')
    shutil.copy2(APP / 'tasktjik-api', home / 'tasktjik-api')
    (root / 'bin').mkdir()
    shutil.copy2(APP / 'bin/tasktjik', root / 'bin/tasktjik')
    # This isolated renderer must not query or focus the real desktop.
    (root / 'bin/hyprctl').write_text('#!/bin/sh\nexit 1\n')
    (root / 'bin/hyprctl').chmod(0o700)
    (root / 'runtime').mkdir(mode=0o700)
    (root / 'config/tasktjik').mkdir(parents=True)
    (root / 'config/tasktjik/tasktjik.conf').write_text(
        '[tasktjik]\nauthority=""\ndirectory=' + json.dumps(str(root / 'tasks')) + '\n')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtQuick.Window\n').replace(
        '    Component.onCompleted:', '''    IpcHandler {
        target: "window-test"
        function close(): void { view.Window.window.close(); }
    }
    Component.onCompleted:'''))
    env = {**os.environ, 'PATH': str(root / 'bin') + ':' + os.environ['PATH'], 'TASKTJIK_HOME': str(home), 'QT_QPA_PLATFORM': 'offscreen',
           'QT_QUICK_BACKEND': 'software', 'QT_QUICK_CONTROLS_STYLE': 'Basic',
           'XDG_RUNTIME_DIR': str(root / 'runtime'), 'XDG_CONFIG_HOME': str(root / 'config'),
           'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

    def ipc(target, action):
        return subprocess.run(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', target, action],
                              env=env, capture_output=True, text=True, timeout=5)

    def wait_visible(expected):
        for _ in range(100):
            result = ipc('tasktjik', 'status')
            if result.returncode == 0:
                state = json.loads(result.stdout)
                if state['windowVisible'] == expected:
                    return state
            time.sleep(0.1)
        raise AssertionError('Window visibility never became ' + str(expected) + '\n' + result.stderr + '\n' + (root / 'shell.log').read_text()[-4000:])

    with (root / 'shell.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'], env=env, stdout=log, stderr=log)
        try:
            wait_visible(True)
            assert ipc('tasktjik', 'projects').returncode == 0
            for _ in range(2):
                assert ipc('window-test', 'close').returncode == 0
                wait_visible(False)
                assert process.poll() is None, 'Closing should keep this reusable instance alive'
                subprocess.run([str(root / 'bin/tasktjik'), 'show'], env=env, check=True, timeout=10)
                state = wait_visible(True)
                assert state['view'] == 'projects', 'Reopening lost navigation state'
                assert process.poll() is None, 'Launcher replaced the existing instance'
            print('PASS: close/reopen twice reuses the shell and preserves project navigation')
        finally:
            process.terminate()
            process.wait(timeout=10)
