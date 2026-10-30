"""Native Rust measurements. All fixtures, state and captures stay in /tmp.

Fresh means process-cold, filesystem/icon caches retained. No personal applications
are activated. UI runs use a private nested Hyprland compositor and synthetic apps.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP / 'tests'))
from rust_parity import Backend, fixture
from private_compositor import private_compositor


def memory(pid):
    parents = {}
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            parents[int(path.parent.name)] = int(path.read_text().rsplit(')', 1)[1].split()[1])
        except (OSError, ValueError, IndexError):
            pass
    ids = {pid}
    while True:
        more = {p for p, parent in parents.items() if parent in ids}
        if more <= ids:
            break
        ids |= more
    rows = []
    for p in sorted(ids):
        try:
            values = {line.split(':', 1)[0]: int(line.split()[1])
                      for line in Path(f'/proc/{p}/smaps_rollup').read_text().splitlines()
                      if line.startswith(('Pss:', 'Rss:'))}
            rows.append({'name': Path(f'/proc/{p}/comm').read_text().strip(),
                         'pss_kib': values['Pss'], 'rss_kib': values['Rss']})
        except OSError:
            pass
    return {'pss_mib': sum(r['pss_kib'] for r in rows) / 1024,
            'rss_mib': sum(r['rss_kib'] for r in rows) / 1024, 'processes': rows}


def timed(fn):
    start = time.perf_counter_ns()
    value = fn()
    return (time.perf_counter_ns() - start) / 1e6, value


def summarize(rows):
    ordered = sorted(rows)
    return {'median_ms': statistics.median(rows), 'p95_ms': ordered[min(len(rows) - 1, int(len(rows) * .95))],
            'min_ms': min(rows), 'max_ms': max(rows), 'samples_ms': rows}


def backends(root, trials):
    environments = {}
    result = {name: {'cold': [], 'resume': [], 'lookup': [], 'provider_first': []} for name in ('rust',)}
    for name in result:
        directory = root / name
        directory.mkdir()
        env = fixture(directory, 1000)
        # Use a real stock theme so icon lookup/rasterization are included fairly.
        with (directory / 'runtjik.conf').open('a') as out:
            out.write('[theme]\nicon_theme="Adwaita"\n')
        # Provide the available themes without importing applications.
        themes = directory / 'system/icons'
        themes.parent.mkdir(parents=True)
        themes.symlink_to('/usr/share/icons', target_is_directory=True)
        environments[name] = env
        app = Backend(name, env)
        result[name]['indexed'] = app.initial['indexed']
        app.close()  # Populate each implementation's own index/icon caches.
    for trial in range(trials):
        for name in ('rust',):
            elapsed, app = timed(lambda: Backend(name, environments[name]))
            result[name]['cold'].append(elapsed)
            try:
                elapsed, value = timed(lambda: app.request(op='resume'))
                assert value['kind'] == 'results'
                result[name]['resume'].append(elapsed)
                for query in ['fixture', 'app 003', 'fxtr', 'zzzzzzz', 'STRASSE', '中文', 'fixture 09', '']:
                    elapsed, value = timed(lambda: app.request(op='query', query=query, limit=30))
                    assert value['kind'] == 'results'
                    result[name]['lookup'].append(elapsed)
                if trial == trials - 1:
                    result[name]['core_memory'] = memory(app.process.pid)
                elapsed, value = timed(lambda: app.request(op='query', query='/define'))
                assert value['kind'] == 'results'
                result[name]['provider_first'].append(elapsed)
                if trial == trials - 1:
                    result[name]['with_provider_memory'] = memory(app.process.pid)
            finally:
                app.close()
    for values in result.values():
        for key in ('cold', 'resume', 'lookup', 'provider_first'):
            values[key] = summarize(values[key])
    return result


def ui(root, trials):
    result = {name: {'cold': [], 'show': [], 'toggle': []} for name in ('rust',)}
    original = os.environ.copy()
    try:
        with private_compositor():
            private = {k: os.environ[k] for k in ('XDG_RUNTIME_DIR', 'WAYLAND_DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE')}
            time.sleep(2)
            environments = {}
            for name in result:
                directory = root / ('ui-' + name)
                directory.mkdir()
                env = fixture(directory, 1000)
                env.update(private, RUNTJIK_BACKEND=name)
                (directory / 'system/icons').parent.mkdir(parents=True)
                (directory / 'system/icons').symlink_to('/usr/share/icons', target_is_directory=True)
                with (directory / 'runtjik.conf').open('a') as out:
                    out.write('[theme]\nicon_theme="Adwaita"\n')
                environments[name] = env
            def run(name, action):
                subprocess.run([str(APP / 'bin/runtjik'), action], env=environments[name],
                               check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=20)
            def status(name):
                return json.loads(subprocess.check_output([str(APP / 'bin/runtjik'), 'status'], env=environments[name], stderr=subprocess.DEVNULL))
            def ready(name):
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    value = status(name)
                    if value['visible'] and value['ready']:
                        assert not value['placementError'] and not value['clickWatchError'], value
                        return value
                    time.sleep(.002)
                raise AssertionError('UI did not become ready')
            for name in result:
                run(name, 'show')
                ready(name)
                run(name, 'quit')
            for trial in range(trials):
                for name in ('rust',):
                    try:
                        elapsed, _ = timed(lambda: run(name, 'show'))
                        state = ready(name)
                        result[name]['cold'].append(elapsed)
                        result[name]['indexed'] = state['indexed']
                        pid = state['pid']
                        run(name, 'close')
                        elapsed, _ = timed(lambda: run(name, 'show'))
                        assert ready(name)['pid'] == pid
                        result[name]['show'].append(elapsed)
                        run(name, 'close')
                        def toggle():
                            run(name, 'toggle')
                            assert ready(name)['pid'] == pid
                        elapsed, _ = timed(toggle)
                        result[name]['toggle'].append(elapsed)
                        if trial == trials - 1:
                            time.sleep(1)
                            result[name]['open_memory'] = memory(pid)
                            run(name, 'close')
                            time.sleep(1)
                            result[name]['hidden_memory'] = memory(pid)
                    finally:
                        run(name, 'quit')
            for values in result.values():
                for key in ('cold', 'show', 'toggle'):
                    values[key] = summarize(values[key])
    finally:
        os.environ.clear()
        os.environ.update(original)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('output', type=Path)
    parser.add_argument('--ui', action='store_true')
    parser.add_argument('--trials', type=int, default=15)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('Choose a new output file')
    root = Path(tempfile.mkdtemp(prefix='runtjik-rust-bench-'))
    print('Private fixtures: ' + str(root), flush=True)
    result = {'date': time.strftime('%Y-%m-%d'), 'kernel': platform.release(), 'trials': args.trials,
              'method': 'Rust-only trials; 1000 synthetic desktop entries plus Unicode fixture and any system Flatpak exports; process-cold, retained OS and icon caches. Backend timings include pipe JSON serialization. UI endpoint is layer/input readiness, not first painted pixels. PSS/RSS sum root and descendants; compositor excluded. No user data or config changed.'}
    result['binary_sha256'] = hashlib.sha256((APP / 'runtjik-core').read_bytes()).hexdigest()
    result['baseline_commit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=APP, text=True).strip()
    result['backend'] = backends(root, args.trials)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    if args.ui:
        result['ui'] = ui(root, args.trials)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({group: {name: {key: value['median_ms'] for key, value in values.items() if isinstance(value, dict) and 'median_ms' in value}
                              for name, values in result[group].items()} for group in ('backend', 'ui') if group in result}, indent=2))


if __name__ == '__main__':
    main()
