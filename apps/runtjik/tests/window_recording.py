"""Portal selection/countdown/save ordering with synthetic executables only."""
import json
import os
from pathlib import Path
import select
import socket
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='runtjik-window-') as directory:
    root = Path(directory)
    bindir = root / 'bin'
    bindir.mkdir()
    encoder = bindir / 'gpu-screen-recorder'
    encoder.write_text('''#!/usr/bin/python3
import json, os, pathlib, signal, socket, sys, time
args = sys.argv
assert args[args.index('-w') + 1] == 'portal'
assert args[args.index('-replay-storage') + 1] == 'ram'
assert int(args[args.index('-r') + 1]) > 2
if os.environ.get('FAIL_STARTUP') == '1':
    print('gsr error: synthetic startup failure', file=sys.stderr)
    sys.exit(2)
log = pathlib.Path(os.environ['EVENTS'])
def event(value):
    with log.open('a') as out: out.write(value + '\\n')
event('picker')
signal.signal(signal.SIGINT, lambda *_: sys.exit(0))
path = pathlib.Path(args[args.index('-ro') + 1]) / 'generated.mp4'
with socket.socket(socket.AF_UNIX) as server:
    server.bind(args[args.index('-ipc') + 1])
    server.listen()
    time.sleep(.6)
    if os.environ.get('CANCEL_PICKER') == '1': sys.exit(0)
    event('selected')
    recording = False
    while True:
        client, _ = server.accept()
        with client:
            request = json.loads(client.makefile().readline())
            reply = {'id': request['id'], 'result': 'ok'}
            if request['name'] == 'start-replay-recording':
                assert 'countdown-complete' in log.read_text()
                event('recording')
                path.write_bytes(b'synthetic video')
                recording = True
            elif recording:
                time.sleep(.7)  # The status socket must stay responsive while saving.
                event('saved')
                reply['data'] = str(path)
                recording = False
            else:
                reply.update(result='error', data='No recording is running')
            client.sendall((json.dumps(reply) + '\\n').encode())
''')
    countdown = bindir / 'quickshell'
    countdown.write_text('''#!/usr/bin/python3
import os, pathlib, time
log = pathlib.Path(os.environ['EVENTS'])
assert log.read_text().splitlines() == ['picker', 'selected']
with log.open('a') as out: out.write('countdown\\n')
time.sleep(.2)
if os.environ.get('CANCEL_COUNTDOWN') != '1':
    pathlib.Path(os.environ['RUNTJIK_COUNTDOWN_SUCCESS']).touch()
    with log.open('a') as out: out.write('countdown-complete\\n')
''')
    notice = bindir / 'notjikctl'
    notice.write_text('''#!/usr/bin/python3
import json, os, pathlib, sys
assert pathlib.Path(os.environ['VIDEO']).read_bytes() == b'synthetic video'
assert pathlib.Path(os.environ['EVENTS']).read_text().splitlines()[-1] == 'saved'
assert not (pathlib.Path(os.environ['XDG_RUNTIME_DIR']) / 'runtjik-capture/recorder.sock').exists()
pathlib.Path(os.environ['NOTICE']).write_text(json.dumps(sys.argv[1:]))
''')
    (bindir / 'ffprobe').write_text('''#!/usr/bin/python3
import os, pathlib, sys
assert sys.argv[-2:] == ['-i', os.environ['VIDEO']]
assert pathlib.Path(os.environ['VIDEO']).read_bytes() == b'synthetic video'
print('83.25')
''')
    for name, contents in {'hyprctl': '#!/bin/sh\necho "[]"\n',
                           'notify-send': '#!/bin/sh\nexit 0\n'}.items():
        (bindir / name).write_text(contents)
    for executable in bindir.iterdir():
        executable.chmod(0o700)
    for cancellation in ('', 'CANCEL_PICKER', 'CANCEL_COUNTDOWN', 'FAIL_STARTUP'):
        case = root / (cancellation or 'success')
        case.mkdir()
        events = case / 'events'
        env = {**os.environ, 'PATH': str(bindir) + ':' + os.environ['PATH'],
               'XDG_RUNTIME_DIR': str(case), 'XDG_STATE_HOME': str(case), 'EVENTS': str(events),
               'NOTJIK_HOME': str(APP.parent / 'notjik')}
        env.pop('HYPRLAND_INSTANCE_SIGNATURE', None)
        if cancellation:
            env[cancellation] = '1'
        output = case / "recording with spaces ' and ;.mp4"
        notification = case / 'notification.json'
        env.update(VIDEO=str(output), NOTICE=str(notification))
        spec = dict(mode='window', fps=30, audio=False, file=str(output), countdown_seconds=3)
        worker = subprocess.Popen([str(APP / 'runtjik-core'), 'capture-worker', json.dumps(spec)],
                                  env=env, stdout=subprocess.PIPE, text=True)
        try:
            assert select.select([worker.stdout], [], [], 8)[0], 'Worker readiness timeout'
            response = json.loads(worker.stdout.readline())
            if cancellation == 'FAIL_STARTUP':
                assert response['kind'] == 'error', response
                assert 'synthetic startup failure' in response['message'], response
                assert worker.wait(timeout=5) != 0
                assert not output.exists()
            elif cancellation:
                assert response == {'cancelled': True}, response
                assert worker.wait(timeout=5) == 0, (worker.returncode, events.read_text(), worker.stdout.read(), (case / 'runtjik/capture.log').read_text())
                assert not output.exists()
                assert 'recording' not in events.read_text().splitlines()
            else:
                assert response == {'ready': True}, response
                subprocess.run([str(APP / 'runtjik-core'), 'stop-recording'], env=env, check=True)
                with socket.socket(socket.AF_UNIX) as status:
                    status.settimeout(.5)
                    status.connect(str(case / 'runtjik-capture/recorder.sock'))
                    status.sendall(b'status\n')
                    assert json.loads(status.makefile().read())['stopping']
                assert worker.wait(timeout=5) == 0, (worker.returncode, events.read_text(), worker.stdout.read(), (case / 'runtjik/capture.log').read_text())
                assert output.read_bytes() == b'synthetic video'
                assert events.read_text().splitlines() == ['picker', 'selected', 'countdown', 'countdown-complete', 'recording', 'saved']
                notice = json.loads(notification.read_text())
                assert notice[:2] == ['send', 'Screen recording'], notice
                assert notice[2] == 'Duration: 1:23', notice
                assert str(output) not in notice[2]
                image = Path(notice[notice.index('--image') + 1])
                assert image.is_file() and image.parent.name == 'bonsai' and image.suffix == '.png'
                assert notice[notice.index('--type') + 1] == 'status', notice
                assert json.loads(notice[notice.index('--data') + 1])['tone'] == 'success'
                assert notice[notice.index('--timeout') + 1] == 'never'
                actions = json.loads(notice[notice.index('--actions') + 1])
                assert [action['label'] for action in actions] == ['View video', 'Share on Telegram']
                assert actions[0]['identifier'] == 'default'
                assert actions[0]['command'] == ['vlc', '--', str(output)]
                assert actions[1]['command'] == [str(APP / 'runtjik-core'), 'share-recording-telegram', str(output)]
            if cancellation:
                assert not notification.exists(), 'Cancelled/failed capture advertised a saved video'
            assert not (case / 'runtjik-capture/recorder.sock').exists()
            if cancellation != 'FAIL_STARTUP':
                assert not list(case.glob('.runtjik-window-*'))
            print('PASS: window flow', cancellation or 'selection -> countdown -> recording -> saved')
        finally:
            if worker.poll() is None:
                worker.terminate()
                worker.wait(timeout=25)
            worker.stdout.close()
