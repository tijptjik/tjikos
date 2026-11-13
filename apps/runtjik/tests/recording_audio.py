"""Exercise source selection and mixer rollback without accessing real audio/video."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='runtjik-audio-') as directory:
    root = Path(directory)
    bindir = root / 'bin'
    bindir.mkdir()
    fake = bindir / 'fake'
    fake.write_text('''#!/usr/bin/python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
with open(os.environ['AUDIO_CALLS'], 'a') as log:
    log.write(json.dumps([name, *sys.argv[1:]]) + '\\n')
if name != 'pactl':
    sys.exit(1)
args = sys.argv[1:]
if args[0] == 'get-default-source': print('test_microphone')
if args[0] == 'get-default-sink': print('test_speakers')
if args[0] == 'load-module':
    if os.environ.get('FAIL_LOOPBACK') and args[1] == 'module-loopback': sys.exit(1)
    counter = pathlib.Path(os.environ['AUDIO_CALLS'] + '.counter')
    value = int(counter.read_text()) + 1 if counter.exists() else 1
    counter.write_text(str(value))
    print(value)
''')
    fake.chmod(0o700)
    for name in ['pactl', 'wf-recorder', 'gpu-screen-recorder']:
        (bindir / name).symlink_to(fake)
    for mode in ['screen', 'region', 'workspace', 'window']:
        for voice, system in [(False, False), (True, False), (False, True), (True, True)]:
            calls_file = root / 'calls'
            calls_file.write_text('')
            env = {**os.environ, 'PATH': str(bindir) + ':' + os.environ['PATH'],
                   'XDG_RUNTIME_DIR': directory, 'XDG_STATE_HOME': directory,
                   'AUDIO_CALLS': str(calls_file)}
            spec = dict(mode=mode, fps=30, microphone=voice, audio=system,
                        file=str(root / 'never-created.mp4'))
            result = subprocess.run([str(APP / 'runtjik-core'), 'capture-worker', json.dumps(spec)],
                                    env=env, capture_output=True, text=True, timeout=15)
            assert result.returncode != 0, result
            calls = [json.loads(line) for line in calls_file.read_text().splitlines()]
            encoder = next(c for c in calls if c[0] != 'pactl')
            if mode == 'window':
                assert not any(c[0] == 'pactl' for c in calls), calls
                source = 'default_output|default_input' if voice and system else 'default_input' if voice else 'default_output'
                assert ('-a' in encoder) == (voice or system), encoder
                if voice or system:
                    assert encoder[encoder.index('-a')+1] == source, encoder
            else:
                source = next((a for a in encoder if a.startswith('--audio=')), '')
                if voice and system:
                    assert source.startswith('--audio=runtjik_record_'), encoder
                    loads = [c for c in calls if c[1] == 'load-module']
                    unloads = [c for c in calls if c[1] == 'unload-module']
                    assert len(loads) == len(unloads) == 3, calls
                    assert [int(c[2]) for c in unloads] == sorted([int(c[2]) for c in unloads], reverse=True), calls
                else:
                    assert source == ('--audio=test_microphone' if voice else '--audio=test_speakers.monitor' if system else ''), encoder
    calls_file.write_text('')
    env['FAIL_LOOPBACK'] = '1'
    spec.update(mode='screen', microphone=True, audio=True)
    result = subprocess.run([str(APP / 'runtjik-core'), 'capture-worker', json.dumps(spec)],
                            env=env, capture_output=True, text=True, timeout=15)
    calls = [json.loads(line) for line in calls_file.read_text().splitlines()]
    assert result.returncode != 0 and calls[-1][1] == 'unload-module', calls
    assert not any(c[0] != 'pactl' for c in calls), calls
    assert not list(root.rglob('*.mp4'))
print('PASS: four audio combinations across four capture modes; startup and partial mixer failure cleanup; no capture')

from rust_parity import Backend, fixture

with tempfile.TemporaryDirectory(prefix='runtjik-audio-settings-') as directory:
    root = Path(directory)
    env = fixture(root, 0)
    env['XDG_RUNTIME_DIR'] = directory
    config = Path(env['RUNTJIK_CONFIG'])
    config.write_text(config.read_text() + '\n[plugins]\nenabled=["Recorder"]\n')
    backend = Backend('rust', env)
    try:
        def states(reply):
            return [a['active'] for a in reply['header']['actions']]
        assert states(backend.request(op='query', query='/rec')) == [False, False]
        reply = backend.request(op='activate', id='/rec', mode='record-microphone')
        assert states(reply) == [True, False], reply
        reply = backend.request(op='activate', id='/rec', mode='record-audio')
        assert states(reply) == [True, True], reply
        assert 'header' not in backend.request(op='query', query='/rec history')
    finally:
        backend.close()
    backend = Backend('rust', env)
    try:
        assert states(backend.request(op='query', query='/rec')) == [True, True]
    finally:
        backend.close()
print('PASS: independent toggles persist across backend restart; hidden in history')
