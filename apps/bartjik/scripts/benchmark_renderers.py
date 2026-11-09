"""Explicitly authorized, serial full-bar benchmark on the current desktop.

Uses a temporary runtime unit override, restores the original service in finally,
never changes user configuration, and never runs two mutation authorities together.
Startup is backend/UI readiness, NOT a measurement of the first presented GPU frame.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import statistics
import subprocess
import time
import tempfile
import copy
from measure_process_tree import snapshot

APP = Path(__file__).resolve().parents[1]
DIST = APP / 'dist'
UNIT = 'bartjik.service'


def systemctl(*args):
    return subprocess.check_output(['systemctl', '--user', *args], text=True).strip()


def request(renderer, operation):
    if renderer == 'quickshell':
        return json.loads(subprocess.check_output(['quickshell', 'ipc', '--path', str(DIST / 'qml'),
            'call', 'bartjik', 'request', json.dumps({'op': operation, 'bar': 'all'})], stderr=subprocess.DEVNULL, text=True, timeout=3))
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(3)
        stream.connect(str(Path(os.environ['XDG_RUNTIME_DIR']) / 'bartjik-gtk.sock'))
        stream.sendall((json.dumps({'action': operation, 'bar': 'all'}) + '\n').encode())
        return json.loads(stream.makefile().readline())


def measure(renderer, seconds):
    root = int(systemctl('show', UNIT, '-p', 'MainPID', '--value'))
    group = systemctl('show', UNIT, '-p', 'ControlGroup', '--value')
    cpu_file = Path('/sys/fs/cgroup') / group.lstrip('/') / 'cpu.stat'
    def cpu():
        return int(dict(line.split() for line in cpu_file.read_text().splitlines())['usage_usec'])
    samples = []
    start = time.monotonic()
    initial = cpu()
    for _ in range(seconds):
        samples.append(snapshot(root))
        time.sleep(1)
    elapsed = time.monotonic() - start
    usage = cpu() - initial
    after = request(renderer, 'bars')
    return {'seconds': elapsed, 'pss_mib_median': statistics.median(sum(p['pss_kib'] for p in rows) / 1024 for rows in samples),
        'cpu_percent_one_core': usage / 1e6 / elapsed * 100,
        'cpu_scope': 'entire service cgroup, including exited helpers',
        'processes_last_sample': samples[-1], 'bar_count': len(after['bars']),
        'concealed': [b['concealed'] for b in after['bars']]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--allow-live-displays', action='store_true', required=True)
    parser.add_argument('--samples', type=int, default=3)
    parser.add_argument('--seconds', type=int, default=20)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.samples < 1 or args.seconds < 1:
        parser.error('positive samples and seconds required')
    override = Path(os.environ['XDG_RUNTIME_DIR']) / 'systemd/user/bartjik.service.d/90-bartjik-benchmark.conf'
    if override.exists():
        raise RuntimeError('Existing benchmark override; refusing to replace it')
    override.parent.mkdir(parents=True, exist_ok=True)
    temporary = tempfile.TemporaryDirectory(prefix='bartjik-benchmark-')
    config_path = Path(temporary.name) / 'config.json'
    original_config = json.loads(Path(os.environ.get('BARTJIK_CONFIG', str(Path.home() / '.config/bartjik/config.json'))).read_text())
    config_path.write_text(json.dumps(original_config))
    report = {'startup_definition': 'restart request to backend ready and allocated bars; not first presented frame',
        'cache_state': 'process-cold, warm filesystem/cache; no global cache flushing',
        'hashes': {name: hashlib.sha256((DIST / 'bin' / name).read_bytes()).hexdigest()
                   for name in ['bartjik-gtk', 'bartjik-gtk-core', 'bartjik-core']}, 'renderers': {}}
    try:
        for renderer in ['quickshell', 'gtk']:
            config_path.write_text(json.dumps(original_config))
            command = f'{DIST}/bin/bartjik --quickshell' if renderer == 'quickshell' else f'{DIST}/bin/bartjik --gtk serve --reserve'
            override.write_text('[Service]\nExecStart=\nExecStart=' + command +
                '\nUnsetEnvironment=BARTJIK_GTK_COMPARE_FILE BARTJIK_GTK_WORKSPACE\nEnvironment=BARTJIK_CONFIG=' + str(config_path) + '\n')
            systemctl('daemon-reload')
            starts = []
            for _ in range(args.samples):
                began = time.monotonic()
                systemctl('restart', UNIT)
                deadline = began + 20
                while True:
                    try:
                        state = request(renderer, 'status')
                        bars = request(renderer, 'bars')['bars']
                        if state.get('ready') and bars and all(b['geometry']['width'] > 0 for b in bars):
                            break
                    except (OSError, ValueError, subprocess.SubprocessError):
                        pass
                    if time.monotonic() > deadline:
                        raise RuntimeError(f'{renderer} did not become ready')
                    time.sleep(.02)
                starts.append((time.monotonic() - began) * 1000)
            result = {'startup_ms': starts, 'startup_median_ms': statistics.median(starts)}
            for mode, operation in [('visible_idle', 'show-reveal'), ('hidden_idle', 'hide-reveal')]:
                configuration = copy.deepcopy(original_config)
                for bar in configuration['bars']:
                    bar['autoHide'] = mode == 'hidden_idle'
                config_path.write_text(json.dumps(configuration))
                time.sleep(2)
                assert request(renderer, operation)['ok']
                time.sleep(2)
                result[mode] = measure(renderer, args.seconds)
                expected = mode == 'hidden_idle'
                if any(v != expected for v in result[mode]['concealed']):
                    raise RuntimeError(f'{renderer} {mode} visibility changed; discard this sample')
            report['renderers'][renderer] = result
            args.output.write_text(json.dumps(report, indent=2) + '\n')
    finally:
        override.unlink(missing_ok=True)
        systemctl('daemon-reload')
        systemctl('restart', UNIT)
        temporary.cleanup()
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
