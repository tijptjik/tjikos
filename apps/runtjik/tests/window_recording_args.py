"""Validate real encoder arguments with no access to a display or session bus."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
if not shutil.which('gpu-screen-recorder'):
    raise SystemExit('SKIP: gpu-screen-recorder is not installed')
with tempfile.TemporaryDirectory(prefix='runtjik-encoder-args-') as directory:
    root = Path(directory)
    env = {**os.environ, 'DISPLAY': '', 'WAYLAND_DISPLAY': 'no-test-display',
           'XDG_RUNTIME_DIR': directory, 'XDG_STATE_HOME': directory,
           'XDG_CONFIG_HOME': directory, 'HOME': directory,
           'DBUS_SESSION_BUS_ADDRESS': 'unix:path=' + directory + '/no-bus'}
    env.pop('HYPRLAND_INSTANCE_SIGNATURE', None)
    spec = dict(mode='window', fps=30, audio=False, countdown_seconds=3,
                file=str(root / 'must-not-exist.mp4'))
    result = subprocess.run([str(APP / 'runtjik-core'), 'capture-worker', json.dumps(spec)],
                            env=env, capture_output=True, text=True, timeout=15)
    reply = json.loads(result.stdout)
    log = (root / 'runtjik/capture.log').read_text()
    assert result.returncode != 0 and reply['kind'] == 'error', reply
    assert 'Recorder startup failed' in reply['message'], reply
    assert 'usage:' not in log and '-r argument' not in log, log
    assert 'failed to connect to the Wayland server' in log, log
    assert not list(root.rglob('*.mp4'))
    print('PASS: actual worker arguments accepted by installed encoder; unavailable display reported as startup error; no capture')
