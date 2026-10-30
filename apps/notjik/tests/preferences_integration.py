"""Private-bus QML acceptance: DND survives backend and whole-app restarts."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(os.environ.get('NOTJIK_TEST_ROOT', str(Path(__file__).resolve().parents[1])))
sys.path.insert(0, str(ROOT / "tests/reference"))
import notjik
from native_fixture import copy_file


def wait_for(check):
    end = time.monotonic() + 8
    while time.monotonic() < end:
        try:
            result = check()
            if result:
                return result
        except (ValueError, OSError, json.JSONDecodeError):
            pass
        time.sleep(.05)
    raise AssertionError('Notification preference did not reach expected state')


def run():
    if not os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/'):
        raise RuntimeError('Run on a private bus with dbus-run-session')
    with tempfile.TemporaryDirectory(prefix='notjik-mode-test-') as tmp:
        root = Path(tmp)
        fixture = root/'app'
        shutil.copytree(ROOT/'qml', fixture/'qml',copy_function=copy_file)
        for path in ROOT.iterdir():
            if path.name != 'qml':
                (fixture/path.name).symlink_to(path)
        state = root/'state'
        state.mkdir()
        pref = state/'preferences.json'
        pref.write_text('{"paused": true}')
        config = root/'config.json'
        config.write_text('{"theme":"rose-punk"}')
        os.environ['NOTJIK_STATE_DIR'] = str(state)
        os.environ['NOTJIK_CONFIG'] = str(config)
        proc = None
        with (root/'qml.log').open('w+') as log:
            def call(op):
                return notjik.ipc(fixture, {'op':op})
            def start():
                nonlocal proc
                proc = subprocess.Popen(['quickshell','--path',str(fixture/'qml'),'--no-color'], stdout=log, stderr=log)
                return wait_for(lambda: call('status'))
            def stop():
                nonlocal proc
                if proc:
                    proc.terminate()
                    proc.wait(timeout=8)
                    proc = None
            try:
                assert start()['paused'] is True
                for expected, command in [(False,'resume'), (True,'toggle')]:
                    assert call(command)['paused'] is expected
                    wait_for(lambda: json.loads(pref.read_text())['paused'] is expected)
                    call('reload')
                    wait_for(lambda: call('status')['paused'] is expected and not call('status')['historyError'])
                    stop()
                    assert start()['paused'] is expected
                config.write_text('{"theme":"rose-punk","maxVisible":3}')
                time.sleep(1.2)
                assert call('status')['paused'] is True
                print('PASS: DND restored before ready; resume/toggle saved; backend reload, app restart, and config reload preserve both modes')
            except Exception:
                log.flush()
                print((root/'qml.log').read_text()[-6000:])
                raise
            finally:
                stop()


if __name__ == '__main__':
    run()
