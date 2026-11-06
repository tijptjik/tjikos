"""Super and launcher Stop against a synthetic encoder in a private compositor."""
import json
import os
from pathlib import Path
import select
import shlex
import shutil
import socket
import subprocess
import time
from private_compositor import private_compositor, wait_for
from rust_parity import fixture

app = Path(os.environ.get('RUNTJIK_TEST_HOME', Path(__file__).resolve().parents[1]))
with private_compositor():
    # wtype uses a compact synthetic keymap, not physical keyboard keycodes.
    subprocess.run(['hyprctl', 'eval', 'hl.config({input={resolve_binds_by_sym=true},misc={layers_hog_keyboard_focus=true}})'],
                   check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'RECORD'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'RECORD' for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors']))))
    subprocess.run(['hyprctl', 'eval', 'hl.monitor({output="WAYLAND-1",disabled=true})'],
                   check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="RECORD"}))'], check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    # Inspection only: the actual non-demo Stop action and process remain intact.
    shell.write_text(shell.read_text().replace("if(!root.demo)throw Error('Test state requires --demo');", '')
                     .replace("if(!root.demo)throw Error('Test control requires --demo');", '')
                     .replace('    function receive(msg) {', '''
    property bool stopFeedbackShown: false
    FrameAnimation {
        running: root.opened
        onTriggered: {
            if(root.stopRequested || root.results.some(row=>row.name==='Saving recording…'))
                root.stopFeedbackShown=true;
        }
    }
    function receive(msg) {
''').replace('statistics:root.statistics,', '''
                stopPoint:(()=>{const tile=items.itemAtIndex(root.results.findIndex(row=>row.id==='Recorder:stop'));return tile?tile.mapToItem(null,tile.width/2,tile.height/2):null;})(),
                stopFeedbackShown:root.stopFeedbackShown,stopRequested:root.stopRequested,message:root.message,statistics:root.statistics,'''))
    config_root = temp / 'fixture'
    config_root.mkdir()
    env = fixture(config_root, 0)
    env['HYPRLAND_INSTANCE_SIGNATURE'] = os.environ['HYPRLAND_INSTANCE_SIGNATURE']
    env['RUNTJIK_HOME'] = str(home)
    env.pop('RUNTJIK_DEMO', None)
    Path(env['RUNTJIK_CONFIG']).write_text('[index]\npath_commands=false\n[ui]\nmonitor="RECORD"\n[plugins]\nenabled=["Recorder"]\n[plugin_settings.Recorder]\ndirectory=' + json.dumps(str(temp / 'captures')) + '\n')
    bindir = temp / 'bin'
    bindir.mkdir()
    encoder = bindir / 'wf-recorder'
    encoder.write_text('''#!/usr/bin/python3
import os, pathlib, signal, sys, time
output = pathlib.Path(sys.argv[sys.argv.index('-f') + 1])
output.write_bytes(b'synthetic fixture, not a screen capture')
def stop(*_):
    pathlib.Path(os.environ['STOP_STAMP']).write_text(str(time.monotonic()))
    time.sleep(.6)  # Simulate saving while the GUI must remain responsive.
    sys.exit(0)
signal.signal(signal.SIGINT, stop)
while True: time.sleep(.01)
''')
    encoder.chmod(0o700)
    notify = bindir / 'notify-send'
    notify.write_text('#!/bin/sh\nexit 0\n')
    notify.chmod(0o700)
    # Never submit synthetic recording notifications to the real desktop.
    notjik = bindir / 'notjikctl'
    notjik.write_text('#!/usr/bin/python3\nimport json, os, pathlib, sys\npathlib.Path(os.environ["STOP_STAMP"] + ".notification").write_text(json.dumps(sys.argv[1:]))\n')
    notjik.chmod(0o700)
    env['PATH'] = str(bindir) + ':' + os.environ['PATH']
    capture_dir = temp / 'captures'
    capture_dir.mkdir()
    worker = None
    gui = None
    pointer = None

    def start_worker(name):
        stamp = temp / (name + '.stopped')
        spec = dict(mode='region', area='0,0 100x100', output=None, workspace=None,
                    fps=30, audio=False, file=str(capture_dir / (name + '.mp4')))
        process = subprocess.Popen([str(home / 'runtjik-core'), 'capture-worker', json.dumps(spec)],
                                   env={**env, 'STOP_STAMP': str(stamp)}, stdout=subprocess.PIPE, text=True)
        assert select.select([process.stdout], [], [], 8)[0], 'Worker startup timed out'
        response = json.loads(process.stdout.readline())
        assert response.get('ready'), response
        return process, stamp

    def status():
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(.5)
            client.connect(str(Path(env['XDG_RUNTIME_DIR']) / 'runtjik-capture/recorder.sock'))
            client.sendall(b'status\n')
            return json.loads(client.makefile('rb').read())

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
                                        'call', 'runtjik', *args], env=env, text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    try:
        for key in ['Super_L', 'Super_R']:
            worker, stamp = start_worker(key)
            # An ordinary Super chord must keep recording.
            subprocess.run(['wtype', '-M', 'logo', '-P', key, '-s', '50', '-k', 'a', '-s', '50', '-p', key, '-m', 'logo'], check=True)
            assert not status()['stopping'] and not stamp.exists()
            start = time.monotonic()
            subprocess.run(['wtype', '-M', 'logo', '-P', key, '-s', '50', '-p', key, '-m', 'logo'], check=True)
            wait_for(stamp.exists, timeout=3)
            latency = float(stamp.read_text()) - start
            assert latency < .25, (key, latency)
            assert status()['stopping']
            assert worker.wait(timeout=5) == 0
            notice = json.loads(Path(str(stamp) + '.notification').read_text())
            actions = json.loads(notice[notice.index('--actions') + 1])
            assert [action['label'] for action in actions] == ['View video', 'Share on Telegram']
            assert actions[0]['identifier'] == 'default'
            assert actions[0]['command'] == ['vlc', '--', str(capture_dir / (key + '.mp4'))]
            assert actions[1]['command'][1:] == ['share-recording-telegram', str(capture_dir / (key + '.mp4'))]
            worker.stdout.close()
            worker = None
            assert not (Path(env['XDG_RUNTIME_DIR']) / 'runtjik-capture/recorder.sock').exists()
            bindings = json.loads(subprocess.check_output(['hyprctl', '-j', 'binds'], text=True))
            assert not any(b.get('description') == 'Stop Runtjik recording' for b in bindings), bindings
            print(f'PASS: {key} stopped in {latency * 1000:.0f} ms; chord preserved; hook cleaned up', flush=True)

        worker, stamp = start_worker('launcher-stop')
        with (temp / 'launcher.log').open('w') as log:
            gui = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
            wait_for(lambda: state().get('rows'))
            wait_for(lambda: json.loads(ipc('status')).get('ready'))
            ipc('test', '/rec ')
            wait_for(lambda: state().get('resultQuery') == '/rec ')
            # Click the rendered Stop tile with a real Wayland virtual pointer.
            protocol = Path(__file__).with_name('virtual_pointer.xml')
            subprocess.run(['wayland-scanner', 'client-header', str(protocol), str(temp / 'pointer.h')], check=True)
            subprocess.run(['wayland-scanner', 'private-code', str(protocol), str(temp / 'protocol.c')], check=True)
            flags = shlex.split(subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True))
            subprocess.run(['cc', str(Path(__file__).with_name('pointer.c')), str(temp / 'protocol.c'),
                            '-I' + str(temp), '-o', str(temp / 'pointer'), *flags], check=True)
            pointer = subprocess.Popen([str(temp / 'pointer'), 'RECORD'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            def point(command):
                pointer.stdin.write(command + '\n')
                pointer.stdin.flush()
                assert pointer.stdout.readline().strip() == 'ok'
            position = wait_for(lambda: state().get('stopPoint'))
            monitor = next(m for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'])) if m['name'] == 'RECORD')
            point(f"m {int(position['x'] * 10000 * monitor['scale'] / monitor['width'])} {int(position['y'] * 10000 * monitor['scale'] / monitor['height'])}")
            wait_for(lambda: abs(json.loads(subprocess.check_output(['hyprctl', '-j', 'cursorpos']))['x']
                                 - (position['x'] + monitor['x'])) < 2)
            time.sleep(.05)
            point('b 1')
            time.sleep(.05)
            start = time.monotonic()
            point('b 0')
            try:
                wait_for(stamp.exists, timeout=3)
            except ValueError:
                print('After Stop click:', state(), json.loads(ipc('status')), flush=True)
                subprocess.run(['grim', '-o', 'RECORD', str(temp / 'stop-failure.png')], check=True)
                raise
            latency = float(stamp.read_text()) - start
            assert latency < .25, latency
            current = state()
            assert current.get('stopFeedbackShown'), current
            assert not current['message'].startswith('Recorder:'), current
            assert worker.wait(timeout=5) == 0
            worker.stdout.close()
            worker = None
            wait_for(lambda: not any(row['id'] == 'Recorder:stop' for row in state().get('rows', [])), timeout=2)
            assert json.loads(ipc('status'))['visible']
            print(f'PASS: launcher Stop reached encoder in {latency * 1000:.0f} ms, saving feedback shown, tile removed promptly', flush=True)
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
    finally:
        if pointer is not None:
            pointer.stdin.close()
            pointer.wait(timeout=5)
            pointer.stdout.close()
        if gui is not None and gui.poll() is None:
            gui.terminate()
            gui.wait(timeout=5)
        if worker is not None:
            if worker.poll() is None:
                worker.terminate()
                worker.wait(timeout=25)
            worker.stdout.close()
        shutil.rmtree(home)
        shutil.rmtree(config_root)
