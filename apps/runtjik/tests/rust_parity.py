"""Native protocol contracts against frozen migration fixtures; isolated synthetic data."""
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('RUNTJIK_TEST_BINARY', str(APP / 'runtjik-core')))


def fixture(root, count=250):
    root = Path(root)
    env = {**os.environ, 'RUNTJIK_HOME': str(APP), 'XDG_CONFIG_HOME': str(root / 'config'),
           'XDG_DATA_HOME': str(root / 'data'), 'XDG_DATA_DIRS': str(root / 'system'),
           'XDG_CACHE_HOME': str(root / 'cache'), 'XDG_STATE_HOME': str(root / 'state'),
           'RUNTJIK_CONFIG': str(root / 'runtjik.conf')}
    env.pop('HYPRLAND_INSTANCE_SIGNATURE', None)
    (root / 'runtjik.conf').write_text('[index]\npath_commands=false\n[launch]\nisolate=false\n')
    apps = root / 'data/applications'
    apps.mkdir(parents=True)
    for i in range(count):
        (apps / f'fixture-{i:04}.desktop').write_text(
            f'[Desktop Entry]\nType=Application\nName=Fixture App {i:04}\n'
            'Exec=/usr/bin/true\nIcon=application-x-executable\nKeywords=synthetic;launcher;\n')
    (apps / 'unicode.desktop').write_text('[Desktop Entry]\nType=Application\nName=Straße 中文\nExec=/usr/bin/true\nIcon=application-x-executable\n')
    return env


class Backend:
    def __init__(self, backend, env, demo=False):
        assert backend == 'rust', 'Only the native backend is supported'
        argv = [str(BINARY)]
        self.process = subprocess.Popen(argv + ['serve'] + (['--demo'] if demo else []), env=env,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.initial = self.read()

    def read(self):
        if not select.select([self.process.stdout], [], [], 20)[0]:
            raise AssertionError('Backend timed out')
        line = self.process.stdout.readline()
        if not line:
            raise AssertionError(f'Backend exited: {self.process.poll()}')
        return json.loads(line)

    def request(self, **request):
        self.process.stdin.write(json.dumps(request) + '\n')
        self.process.stdin.flush()
        return self.read()

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=10)
        assert self.process.returncode == 0


def comparable(value):
    if isinstance(value, dict):
        return {k: comparable(v) for k, v in value.items() if k not in ('icon',)}
    if isinstance(value, list):
        return [comparable(v) for v in value]
    return value


def normalized(value, root):
    if isinstance(value, dict):
        return {k: ('<preview>' if k == 'thumbnail' and v else '<today>' if k == 'today' else normalized(v, root)) for k, v in value.items() if k not in ('icon', 'mutation_accepted')}
    if isinstance(value, list):
        return [normalized(v, root) for v in value]
    if isinstance(value, str):
        return value.replace(str(root), '@FIXTURE@').replace(str(APP), '@APP@')
    return value


def main():
    checks = 0
    for session in json.loads((APP / 'tests/fixtures/protocol.json').read_text()):
        with tempfile.TemporaryDirectory(prefix='runtjik-contract-') as temp:
            backend = Backend('rust', fixture(temp), session['demo'])
            try:
                for case in session['cases']:
                    result = backend.request(**case['request'])
                    for _ in range(100):
                        if not result.get('pending'):
                            break
                        time.sleep(.02)
                        result = backend.request(**case['request'])
                    actual = normalized(result, temp)
                    expected=normalized(case['response'], temp)
                    assert actual == expected, (case['request'], actual, expected)
                    checks += 1
            finally:
                backend.close()
    print(f'{checks} frozen native protocol contracts passed')


if __name__ == '__main__':
    main()
