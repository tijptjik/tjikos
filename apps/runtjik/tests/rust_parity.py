"""Black-box protocol parity against the Python reference, with isolated synthetic data."""
import hashlib
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]


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
        argv = ['python3', '-B', str(APP / 'runtjik.py')] if backend == 'python' else [str(APP / 'runtjik-core')]
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

    def settled_query(self, query):
        deadline = time.monotonic() + 20
        while True:
            result = self.request(op='query', query=query)
            if not result.get('pending'):
                return result
            assert time.monotonic() < deadline, f'Provider did not settle: {query}'
            time.sleep(0.01)

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=10)
        assert self.process.returncode == 0


def comparable(value):
    if isinstance(value, dict):
        # Providers create independent temporary preview paths. Compare the
        # generated image bytes, while still requiring each preview to exist.
        return {k: hashlib.sha256(Path(v).read_bytes()).hexdigest()
                if k == 'thumbnail' and v else comparable(v)
                for k, v in value.items() if k not in ('icon',)}
    if isinstance(value, list):
        return [comparable(v) for v in value]
    return value


def main():
    with tempfile.TemporaryDirectory(prefix='runtjik-parity-') as temp:
        env = fixture(temp)
        py = Backend('python', env)
        rust = Backend('rust', env)
        checks = 0
        try:
            assert comparable(py.initial) == comparable(rust.initial), (py.initial, rust.initial)
            for query in ['', 'fixture', 'app 003', 'fxtr', 'zzzzzzz', 'STRASSE', '中文', '/run', '!!', '?', '/processes', '/', '!', '/config', '/config ui.columns', '/bindings', '/define', '/cliphist', '/tabs']:
                # Clipboard/browser external state is tested only through synthetic demo below.
                if query in ('/cliphist', '/tabs'):
                    continue
                a = py.request(op='query', query=query, serial=7)
                b = rust.request(op='query', query=query, serial=7)
                assert comparable(a) == comparable(b), (query, a, b)
                checks += 1
            for offset in [0, 17, 34, 249, 10000]:
                a = py.request(op='query', query='fixture', offset=offset, limit=17)
                b = rust.request(op='query', query='fixture', offset=offset, limit=17)
                assert comparable(a) == comparable(b), (offset, a, b)
                checks += 1
            for request in [dict(op='query', offset=-1), dict(op='query', limit=0), dict(op='query', limit=True), dict(op='nonsense')]:
                assert py.request(**request)['kind'] == rust.request(**request)['kind'] == 'error'
                checks += 1
            a = py.request(op='resume')
            b = rust.request(op='resume')
            assert comparable(a) == comparable(b)
            # A native search must not start the compatibility host.
            fresh = Backend('rust', env)
            try:
                fresh.request(op='query', query='fixture')
                children = Path(f'/proc/{fresh.process.pid}/task/{fresh.process.pid}/children').read_text().strip()
                assert children == '', children
            finally:
                fresh.close()
        finally:
            py.close()
            rust.close()
        # Native ordinary activation must remain independent of the Python host.
        native = Backend('rust', env)
        try:
            response = native.request(op='activate', id='apps:fixture-0000.desktop', mode='switch', prepare=True)
            assert response == {'kind': 'launch', 'entry': 'apps:fixture-0000.desktop', 'serial': 0}, response
            response = native.request(op='activate', id='apps:fixture-0000.desktop', mode='launch')
            assert response['kind'] == 'activated', response
            time.sleep(.1)
            children = Path(f'/proc/{native.process.pid}/task/{native.process.pid}/children').read_text().split()
            assert all('python' not in Path(f'/proc/{pid}/comm').read_text() for pid in children)
            checks += 2
        finally:
            native.close()
        py = Backend('python', env)
        rust = Backend('rust', env)
        try:
            # Same SQLite counters and frecency after a native launch.
            assert comparable(py.initial) == comparable(rust.initial)
            assert py.initial['results'][0]['id'] == 'apps:fixture-0000.desktop'
            # Reject bad edits on resume and keep the last usable settings.
            config = Path(env['RUNTJIK_CONFIG'])
            original = config.read_text()
            for extra in ['[ui]\ncolumns=99\n', '[ui]\nwidth=true\n',
                          '[plugin_settings.Words]\ntimeout_seconds=0\n',
                          '[plugin_settings.Recorder]\ndirectory="xdg:VIDEOS/../bad"\n',
                          '[plugin_settings.Screenshots]\ndirectory="' + str(APP) + '/captures"\n']:
                config.write_text(original + extra)
                assert py.request(op='resume')['kind'] == rust.request(op='resume')['kind'] == 'error'
                assert comparable(py.request(op='query', query='fixture')) == comparable(rust.request(op='query', query='fixture'))
                checks += 1
            config.write_text(original + '[ui]\nmax_results=17\n')
            assert comparable(py.request(op='resume')) == comparable(rust.request(op='resume'))
            assert len(rust.request(op='query', query='fixture')['results']) == 17
            config.write_text(original)
            # Discovery invalidation and Fish parsing retain executable names only.
            fish = Path(env['XDG_DATA_HOME']) / 'fish/fish_history'
            fish.parent.mkdir(parents=True)
            fish.write_text('- cmd: printf synthetic-argument\n- cmd: env EXAMPLE=synthetic /usr/bin/true\n- cmd: echo hello | cat\n')
            config.write_text(original.replace('path_commands=false', 'path_commands=true'))
            assert comparable(py.request(op='resume')) == comparable(rust.request(op='resume'))
            assert comparable(py.request(op='query', query='/run')) == comparable(rust.request(op='query', query='/run'))
            checks += 3
            config.write_text(original)
        finally:
            py.close()
            rust.close()
        for demo in [True]:
            py = Backend('python', env, demo)
            rust = Backend('rust', env, demo)
            try:
                for query in ['/cliphist', '/grab', '/rec', '/todo', '/reading', '/define test', '!define test']:
                    a = py.settled_query(query)
                    b = rust.settled_query(query)
                    # Todo demo timestamps and generated IDs are intentionally independent.
                    if query in ('/todo', '/reading'):
                        assert a['app_mode'] == b['app_mode'] == 'Todo'
                    else:
                        assert comparable(a) == comparable(b), (query, a, b)
                    checks += 1
                for request in [dict(op='activate', id='/pin'), dict(op='activate', id='/grid'),
                                dict(op='activate', id='apps:firefox.desktop'),
                                dict(op='activate', id='apps:firefox.desktop', mode='switch', prepare=True),
                                dict(op='activate', id='apps:firefox.desktop', mode='close')]:
                    a = py.request(**request)
                    b = rust.request(**request)
                    assert comparable(a) == comparable(b), (request, a, b)
                    checks += 1
            finally:
                py.close()
                rust.close()
        print(f'{checks} protocol comparisons passed, plus startup/resume and lazy-host checks')


if __name__ == '__main__':
    main()
