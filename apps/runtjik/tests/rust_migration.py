"""Native compatibility-removal contracts; only synthetic state under /tmp."""
import json
import os
from pathlib import Path
import subprocess
import socket
import threading
import tempfile
import time
import unittest

from rust_parity import APP, BINARY, Backend, comparable, fixture


def children(backend):
    pid = backend.process.pid
    return Path(f'/proc/{pid}/task/{pid}/children').read_text().split()


class NativeMigrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='runtjik-native-migration-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.env = fixture(self.root, 5)

    def backend(self, name='rust', demo=False):
        backend = Backend(name, self.env, demo)
        self.addCleanup(backend.close)
        return backend

    def test_tasktjik_demo_and_local_state_without_host(self):
        Path(self.env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Todo"]\n[plugin_settings.Todo]\nauthority=""\ndirectory=' + json.dumps(str(self.root / 'tasks')) + '\n')
        for demo in [True, False]:
            native = self.backend(demo=demo)
            state = native.request(op='query', query='/reading')
            self.assertEqual(state['app_state']['collection'], 'reading')
            self.assertEqual(state['app_state']['tasks'], [])
            import uuid
            key = str(uuid.uuid4())
            create = dict(op='app-action', provider='Todo', action='create', request_id=key,
                          fields={'title': 'Synthetic reading', 'kind': 'reading', 'reference': 'https://example.com'})
            state = native.request(**create)['app_state']
            task = state['tasks'][0]
            self.assertEqual(len(native.request(**create)['app_state']['tasks']), 1)
            state = native.request(op='app-action', provider='Todo', action='toggle', id=task['id'], revision=task['revision'])['app_state']
            self.assertTrue(state['tasks'][0]['done'])
            self.assertEqual(native.request(op='app-action', provider='Todo', action='edit', id=task['id'], revision=0, fields={'title':'stale'})['kind'], 'error')
            self.assertEqual(children(native), [])
        reference = self.backend()
        rows = reference.request(op='query', query='/reading')['app_state']['tasks']
        self.assertEqual(rows[0]['title'], 'Synthetic reading')
        self.assertTrue(rows[0]['done'])

    def test_tasktjik_persisted_recurrence_and_vault(self):
        vault = self.root / 'vault'
        vault.mkdir()
        (vault / 'Synthetic.md').write_text('Synthetic plan')
        outside = self.root / 'outside.md'
        outside.write_text('Not a vault plan')
        (vault / 'escape.md').symlink_to(outside)
        Path(self.env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Todo"]\n[plugin_settings.Todo]\nauthority=""\ndirectory=' + json.dumps(str(self.root / 'tasks')) + '\nvault=' + json.dumps(str(vault)) + '\n')
        reference = self.backend()
        for unit, value in [('months', 1), ('years', 1), ('days-of-the-week', '1,5'), ('days-of-the-month', '1,31')]:
            result = reference.request(op='app-action', provider='Todo', action='create', fields={'title':unit,'due':'2024-01-31','repeat':{'unit':unit,'value':value}})
            self.assertEqual(result['kind'], 'app-state')
        native = self.backend()
        self.assertEqual(comparable(native.request(op='query', query='/todo')), comparable(reference.request(op='query', query='/todo')))
        self.assertEqual(native.request(op='app-action', provider='Todo', action='plans')['app_state']['plans'], ['Synthetic.md'])
        self.assertEqual(children(native), [])

    def test_tasktjik_unix_api_precedence_device_and_request_ids(self):
        import http.server
        import socketserver
        records = []
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                records.append((self.path, dict(self.headers), body))
                result = [] if body.get('action') == 'list' else {'id': 'synthetic'}
                data = json.dumps({'result': result}).encode()
                self.send_response(200)
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
        class Server(socketserver.UnixStreamServer):
            pass
        path = self.root / 'tasktjik.sock'
        server = Server(str(path), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        Path(self.env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Todo"]\n[plugin_settings.Todo]\nsocket="/missing/socket"\n')
        config = Path(self.env['XDG_CONFIG_HOME']) / 'tasktjik/tasktjik.conf'
        config.parent.mkdir(parents=True, exist_ok=True)
        config.write_text('[tasktjik]\nsocket=' + json.dumps(str(path)) + '\n')
        native = self.backend()
        self.assertEqual(native.request(op='query', query='/todo')['app_mode'], 'Todo')
        native.request(op='app-action', provider='Todo', action='create', fields={'title':'Synthetic'})
        self.assertEqual(len(records), 3)
        import uuid
        identities = []
        for path, headers, body in records:
            self.assertEqual(path, '/api/v1/request')
            self.assertEqual(headers['X-Tasktjik-Request'], '1')
            identities.append(headers['X-Tasktjik-Device'])
            uuid.UUID(identities[-1])
            if body.get('action') == 'create':
                uuid.UUID(body['request_id'])
        self.assertEqual(len(set(identities)), 1)
        self.assertEqual(children(native), [])

    def test_tasktjik_https_dns_fallback_and_redirect_rejection(self):
        bindir = self.root / 'transport-bin'
        bindir.mkdir()
        calls = self.root / 'transport-calls.jsonl'
        curl = bindir / 'curl'
        curl.write_text('#!/usr/bin/python3\nimport sys,json,pathlib\nargs=sys.argv[1:]\nbody=sys.stdin.read()\nwith pathlib.Path(' + repr(str(calls)) + ').open("a") as f:f.write(json.dumps(args)+"\\n")\nif "--resolve" not in args:\n print("curl: (6) Could not resolve host",file=sys.stderr);sys.exit(6)\nprint(json.dumps({"result":[]}))\nsys.stdout.write("200")\n')
        curl.chmod(0o700)
        tailscale = bindir / 'tailscale'
        tailscale.write_text('#!/usr/bin/python3\nimport json\nprint(json.dumps({"Peer":{"synthetic":{"DNSName":"si.synthetic.ts.net.","TailscaleIPs":["100.64.0.1"]}}}))\n')
        tailscale.chmod(0o700)
        self.env['PATH'] = str(bindir) + ':' + os.environ['PATH']
        Path(self.env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Todo"]\n[plugin_settings.Todo]\nurl="https://si.synthetic.ts.net/tasktjik"\n')
        native = self.backend()
        reply = native.request(op='query', query='/todo')
        self.assertIn('app_state', reply, reply.get('message'))
        self.assertEqual(reply['app_state']['tasks'], [])
        argv = [json.loads(line) for line in calls.read_text().splitlines()]
        self.assertEqual(len(argv), 2)
        self.assertIn('si.synthetic.ts.net:443:100.64.0.1', argv[1])
        self.assertIn('https://si.synthetic.ts.net/tasktjik/api/v1/request', argv[1])
        self.assertEqual(argv[1][0], '--disable')
        self.assertIn('--noproxy', argv[1])
        self.assertNotIn('--location', argv[1])
        curl.write_text('#!/usr/bin/python3\nimport sys\nsys.stdin.read()\nprint("{}")\nsys.stdout.write("302")\n')
        reply = native.request(op='query', query='/todo')
        self.assertEqual(reply['app_mode'], 'Todo')
        self.assertIn('HTTP status 302', reply['message'])
        self.assertEqual(children(native), [])


    def test_browser_host_native_framing_and_cleanup(self):
        import struct
        runtime = self.root / 'runtime'
        runtime.mkdir(mode=0o700)
        env = {**self.env, 'XDG_RUNTIME_DIR': str(runtime)}
        host = subprocess.Popen([str(BINARY), 'browser-host'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.addCleanup(lambda: host.poll() is None and host.kill())
        directory = runtime / 'runtjik-browser'
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            paths = list(directory.glob('*.sock'))
            if paths:
                break
            time.sleep(.01)
        self.assertEqual(len(paths), 1)
        def extension():
            size = struct.unpack('=I', host.stdout.read(4))[0]
            request = json.loads(host.stdout.read(size))
            for value in [{'id':'stale'}, {'id':request['id'], 'windows':[]}]:
                data = json.dumps(value).encode()
                host.stdin.write(struct.pack('=I', len(data)) + data)
            host.stdin.flush()
        thread = threading.Thread(target=extension)
        thread.start()
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(5)
            client.connect(str(paths[0]))
            client.sendall(b'{"op":"list"}\n')
            response = json.loads(client.makefile('rb').readline())
            self.assertEqual(response['windows'], [])
        thread.join(timeout=5)
        host.stdin.close()
        self.assertEqual(host.wait(timeout=5), 0)
        self.assertFalse(paths[0].exists())
        host.stdout.close()

    def test_recorder_worker_owns_only_synthetic_encoder(self):
        runtime = self.root / 'runtime'
        runtime.mkdir(mode=0o700)
        bindir = self.root / 'bin'
        bindir.mkdir()
        encoder = bindir / 'wf-recorder'
        encoder.write_text('#!/usr/bin/python3\nimport signal,time,sys,pathlib\nsignal.signal(signal.SIGINT,lambda *_:sys.exit(0))\npathlib.Path(sys.argv[sys.argv.index("-f")+1]).write_bytes(b"synthetic fixture")\nwhile True: time.sleep(.02)\n')
        encoder.chmod(0o700)
        env = {**self.env, 'XDG_RUNTIME_DIR': str(runtime), 'PATH': str(bindir) + ':' + os.environ['PATH']}
        spec = dict(mode='region', area='0,0 100x100', output=None, workspace=None, fps=30, audio=False, file=str(self.root / 'synthetic.mp4'))
        worker = subprocess.Popen([str(BINARY), 'capture-worker', json.dumps(spec)], env=env, stdout=subprocess.PIPE, text=True)
        def stop_worker():
            if worker.poll() is None:
                worker.terminate()
                worker.wait(timeout=25)
        self.addCleanup(stop_worker)
        self.assertTrue(json.loads(worker.stdout.readline())['ready'])
        path = runtime / 'runtjik-capture/recorder.sock'
        for action in ['status', 'stop']:
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(3)
                client.connect(str(path))
                client.sendall((action + '\n').encode())
                response = json.loads(client.makefile("rb").read())
                self.assertEqual(response['file'], spec['file'])
                self.assertEqual(response['stopping'], action == 'stop')
        self.assertEqual(worker.wait(timeout=10), 0)
        self.assertFalse(path.exists())
        worker.stdout.close()
        self.assertEqual(Path(spec['file']).stat().st_mode & 0o777, 0o600)


    def test_native_config_save_backup_migration_and_permissions(self):
        path = Path(self.env['RUNTJIK_CONFIG'])
        path.write_text('# preserve this\n[ui]\ncolumns = 5 # preserve comment\n')
        path.chmod(0o640)
        backend = self.backend()
        result = backend.request(op='activate', id='/config ui.columns 6')
        self.assertTrue(result['apply_config'])
        self.assertIn('columns = 6 # preserve comment', path.read_text())
        self.assertIn('# preserve this', path.read_text())
        self.assertEqual(path.stat().st_mode & 0o777, 0o640)
        saved = path.read_text()
        result = backend.request(op='activate', id='/config ui.columns 99')
        self.assertEqual(result['kind'], 'error')
        self.assertEqual(path.read_text(), saved)
        self.assertEqual(children(backend), [])
        migrate = lambda: json.loads(subprocess.check_output([str(BINARY), 'migrate-config'], env=self.env))
        result = migrate()
        self.assertTrue(result['changed'])
        backup = Path(result['backup'])
        self.assertEqual(backup.read_text(), saved)
        self.assertEqual(backup.stat().st_mode & 0o777, 0o600)
        self.assertFalse(migrate()['changed'])
        self.assertEqual(len(list(path.parent.glob('*.pre-plugins-*'))), 1)


    def test_browser_native_socket_framing_and_targeted_actions(self):
        runtime = self.root / 'runtime'
        runtime.mkdir(mode=0o700)
        directory = runtime / 'runtjik-browser'
        directory.mkdir(mode=0o700)
        self.env['XDG_RUNTIME_DIR'] = str(runtime)
        Path(self.env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Browser"]\n')
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(directory / 'fixture.sock'))
        server.listen()
        server.settimeout(.1)
        stopped = threading.Event()
        requests = []
        state = {'windows': [{'id': 4, 'title': 'Synthetic browser', 'tabs': [
            {'id': 41, 'title': 'Pinned', 'pinned': True},
            {'id': 42, 'title': 'Article', 'url': 'https://example.org/article',
             'favicon': 'moz-remote-image://?url=https%3A%2F%2Fexample.org%2Ficon.png', 'audible': True}]}]}
        def serve():
            while not stopped.is_set():
                try:
                    client, _ = server.accept()
                except socket.timeout:
                    continue
                with client:
                    request = json.loads(client.makefile('rb').readline())
                    requests.append(request)
                    client.sendall(json.dumps(state if request['op'] == 'list' else {}).encode() + b'\n')
        worker = threading.Thread(target=serve)
        worker.start()
        def cleanup():
            stopped.set()
            worker.join(timeout=2)
            server.close()
        self.addCleanup(cleanup)
        backend = self.backend()
        rows = backend.request(op='query', query='/tabs')['results']
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]['favicon'], 'https://example.org/icon.png')
        self.assertEqual(rows[0]['subtitle'], 'example.org · Playing audio')
        self.assertEqual(backend.request(op='activate', id=rows[0]['id'])['kind'], 'activated')
        backend.request(op='activate', id=rows[0]['id'], mode='close')
        self.assertIn({'op': 'focus', 'window': 4, 'tab': 42}, requests)
        self.assertIn({'op': 'close', 'window': 4, 'tab': 42, 'url': 'https://example.org/article'}, requests)
        self.assertEqual(children(backend), [])

    def test_dictionary_streams_independent_sources_and_discards_stale_jobs(self):
        directory = self.root / 'bin'
        directory.mkdir()
        transport = directory / 'curl'
        transport.write_text('''#!/bin/bash
url="${!#}"
case "$url" in
  *stale*) sleep 0.2; printf '%s\\n200' '[{"meanings":[{"definitions":[{"definition":"stale result"}]}]}]' ;;
  *latest*) sleep 0.05; printf '%s\\n200' '[{"meanings":[{"definitions":[{"definition":"latest result"}]}]}]' ;;
  *wiktionary*) sleep 0.3; printf '%s\\n200' '{"en":[{"partOfSpeech":"verb","definitions":[{"definition":"<b>Slow &amp; useful</b>"}]}]}' ;;
  *) printf '%s\\n200' '[{"meanings":[{"partOfSpeech":"verb","definitions":[{"definition":"Fast result","example":"Synthetic example"}]}]}]' ;;
esac
''')
        transport.chmod(0o755)
        self.env['PATH'] = str(directory) + ':' + self.env['PATH']
        config = Path(self.env['RUNTJIK_CONFIG'])
        config.write_text(config.read_text() + '[plugin_settings.Words]\ndefine_sources=["wiktionary", "free_dictionary"]\n')
        backend = self.backend()
        first = backend.request(op='query', query='/define synthetic')
        self.assertTrue(first['pending'])
        deadline = time.monotonic() + 5
        streamed = False
        while time.monotonic() < deadline:
            reply = backend.request(op='query', query='/define synthetic')
            if reply['pending'] and any(r['name'] == 'Fast result' for r in reply['results']):
                streamed = True
            if not reply['pending']:
                break
            time.sleep(.01)
        self.assertTrue(streamed)
        self.assertFalse(reply['pending'])
        self.assertIn('Slow & useful', [r['name'] for r in reply['results']])
        backend.request(op='query', query='/define stale')
        reply = backend.request(op='query', query='/define latest')
        self.assertEqual(reply['query'], '/define latest')
        deadline = time.monotonic() + 5
        while reply['pending'] and time.monotonic() < deadline:
            time.sleep(.01)
            reply = backend.request(op='query', query='/define latest')
        self.assertFalse(reply['pending'])
        self.assertIn('latest result', [r['name'] for r in reply['results']])
        self.assertNotIn('stale result', [r['name'] for r in reply['results']])
        # No Python compatibility host was needed; curl fixtures are ordinary sh children.
        for pid in children(backend):
            self.assertNotIn('python', Path(f'/proc/{pid}/comm').read_text())


if __name__ == '__main__':
    unittest.main(verbosity=2)
