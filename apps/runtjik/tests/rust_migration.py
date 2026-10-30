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

from rust_parity import APP, Backend, comparable, fixture


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

    def test_menus_general_commands_config_and_close_without_host(self):
        native = self.backend(demo=True)
        reference = self.backend('python', demo=True)
        for query in ['todo', 'reading', 'clipboard', 'bindings', '/', '!config', '/config', '/bindings', '/define', '/synonym test']:
            with self.subTest(query=query):
                a = native.request(op='query', query=query)
                b = reference.request(op='query', query=query)
                self.assertEqual(comparable(a), comparable(b))
        for request in [dict(op='activate', id='/config ui.columns'),
                        dict(op='activate', id='/config ui.columns 6'),
                        dict(op='activate', id='/stats'),
                        dict(op='activate', id='apps:firefox.desktop', mode='close')]:
            self.assertEqual(comparable(native.request(**request)), comparable(reference.request(**request)))
        self.assertEqual(children(native), [])

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
        migrate = lambda: json.loads(subprocess.check_output([str(APP / 'runtjik-core'), 'migrate-config'], env=self.env))
        result = migrate()
        self.assertTrue(result['changed'])
        backup = Path(result['backup'])
        self.assertEqual(backup.read_text(), saved)
        self.assertEqual(backup.stat().st_mode & 0o777, 0o600)
        self.assertFalse(migrate()['changed'])
        self.assertEqual(len(list(path.parent.glob('*.pre-plugins-*'))), 1)

    def test_browser_scope_general_results_and_actions_without_host(self):
        Path(self.env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Browser"]\n')
        native = self.backend(demo=True)
        reference = self.backend('python', demo=True)
        self.assertEqual(comparable(native.initial), comparable(reference.initial))
        for query in ['/browser', '/zen', '/tabs Firefox', 'Firefox']:
            self.assertEqual(comparable(native.request(op='query', query=query)), comparable(reference.request(op='query', query=query)))
        for app in [native, reference]:
            rows = app.request(op='query', query='/browser')['results']
            self.assertEqual(app.request(op='activate', id=rows[0]['id'], mode='switch')['text'], '/tabs ')
        self.assertEqual(comparable(native.request(op='query', query='/tabs')), comparable(reference.request(op='query', query='/tabs')))
        for app in [native, reference]:
            tab = app.request(op='query', query='/tabs')['results'][0]
            self.assertEqual(app.request(op='activate', id=tab['id'])['kind'], 'activated')
            self.assertEqual(app.request(op='activate', id=tab['id'], mode='close')['message'], 'Tab closed')
        self.assertEqual(comparable(native.request(op='query', query='')), comparable(reference.request(op='query', query='')))
        self.assertEqual(children(native), [])

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
