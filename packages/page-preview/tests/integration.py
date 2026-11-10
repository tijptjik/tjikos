"""Native preview contracts with loopback HTTP and synthetic browser processes only."""
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
import time
import unittest
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('PAGE_PREVIEW_TEST_BINARY', ROOT / 'target/release/page-preview'))
PNG = b'\x89PNG\r\n\x1a\nsynthetic'


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        self.server.requests.append(self.path)
        if self.path == '/redirect':
            self.send_response(302)
            self.send_header('Location', '/page')
            self.end_headers()
            return
        if self.path == '/bad-redirect':
            self.send_response(302)
            self.send_header('Location', 'file:///etc/passwd')
            self.end_headers()
            return
        if self.path in ('/image.png', '/favicon.ico'):
            body, kind = PNG, 'image/png'
        elif self.path == '/large':
            body, kind = b'x' * (2 * 1024 * 1024 + 1), 'text/html'
        elif self.path == '/charset':
            body, kind = b'<title>caf\xe9</title>', 'text/html; charset=iso-8859-1'
        elif self.path == '/not-html':
            body, kind = b'{}', 'application/json'
        else:
            body = b"<title>Page &amp; title</title><meta property='og:description' content='Description &amp; detail'><meta property='og:image' content='/image.png'><link rel='icon' href='/favicon.ico'>"
            kind = 'text/html'
        self.send_response(200)
        self.send_header('Content-Type', kind)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


class NativePreviewTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        cls.server.requests = []
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f'http://127.0.0.1:{cls.server.server_port}'

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='native-preview-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.env = {**os.environ, 'XDG_CACHE_HOME': str(self.root / 'cache'),
                    'PATH': str(self.bin), 'NO_PROXY': '*', 'no_proxy': '*'}
        self.cache = self.root / 'cache/tasktjik/page-previews'

    def preview(self, path='/page'):
        result = subprocess.run([str(BINARY)], input=self.base + path, text=True,
                                capture_output=True, env=self.env, check=True, timeout=15)
        return json.loads(result.stdout)

    def fake_browser(self, mode='ok'):
        script = self.bin / 'google-chrome'
        script.write_text('''#!/bin/bash
for arg in "$@"; do
    case "$arg" in
        --screenshot=*) screenshot="${arg#*=}" ;;
        --user-data-dir=*) profile="${arg#*=}" ;;
    esac
done
[[ "$profile" == "$TMPDIR/profile" && ! -e "$profile" ]] || exit 4
''' + ('''
/bin/sleep 120 &
echo "$!" > "$TMPDIR/../../../browser-child"
echo "$TMPDIR" > "$TMPDIR/../../../browser-profile"
wait
''' if mode == 'hang' else r'''
printf '\211PNG\r\n\032\nsynthetic' > "$screenshot"
printf '<title>''' + ('Access denied' if mode == 'blocked' else 'Rendered page') + '''</title>'
'''))
        script.chmod(0o700)

    def test_metadata_without_python_or_browser(self):
        result = self.preview('/redirect')
        self.assertEqual(result['title'], 'Page & title')
        self.assertEqual(result['description'], 'Description & detail')
        self.assertEqual(result['imageKind'], 'metadata')
        self.assertEqual(result['renderError'], 'browser-unavailable')
        self.assertEqual(Path(unquote(urlsplit(result['image']).path)).read_bytes(), PNG)
        self.assertEqual(self.preview('/charset')['title'], 'café')

    def test_browser_render_and_blocked_fallback(self):
        self.fake_browser()
        result = self.preview()
        self.assertEqual(result['imageKind'], 'page')
        self.assertFalse(list(self.cache.glob('render-*')))
        self.fake_browser('blocked')
        result = self.preview('/blocked')
        self.assertEqual(result['renderError'], 'blocked')
        self.assertEqual(result['imageKind'], 'favicon')

    def test_bad_scheme_size_type_and_redirect(self):
        for path in ('/large', '/not-html', '/bad-redirect'):
            self.assertTrue(self.preview(path)['unavailable'])
        result = subprocess.run([str(BINARY)], input='file:///etc/passwd', text=True,
                                capture_output=True, env=self.env, check=True)
        self.assertEqual(json.loads(result.stdout), {})

    def test_old_cache_missing_artwork_and_stream_order(self):
        saved = self.preview()
        key = hashlib.sha256((self.base + '/page').encode()).hexdigest()
        receipt = self.cache / (key + '.json')
        os.utime(receipt, (1, 1))
        count = len(self.server.requests)
        self.assertEqual(self.preview(), saved)
        self.assertEqual(len(self.server.requests), count)
        result = subprocess.run([str(BINARY), '--batch'],
                                input=json.dumps([self.base + '/fresh', self.base + '/page', self.base + '/page']),
                                text=True, capture_output=True, env=self.env, check=True, timeout=15)
        rows = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual([row['url'] for row in rows], [self.base + '/page', self.base + '/fresh'])
        Path(unquote(urlsplit(saved['image']).path)).unlink()
        self.preview()
        self.assertGreater(len(self.server.requests), count)

    def test_cancellation_kills_browser_children_and_removes_profile(self):
        cached = self.preview('/cached')
        self.fake_browser('hang')
        process = subprocess.Popen([str(BINARY), '--batch'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, env=self.env)
        try:
            process.stdin.write(json.dumps([self.base + '/cancel', self.base + '/cached']))
            process.stdin.close()
            # The hit is flushed while the earlier uncached URL is still rendering.
            import select
            self.assertTrue(select.select([process.stdout], [], [], 3)[0])
            self.assertEqual(json.loads(process.stdout.readline()),
                             {'url': self.base + '/cached', 'preview': cached})
            child_file = self.root / 'cache/browser-child'
            profile_file = self.root / 'cache/browser-profile'
            deadline = time.monotonic() + 10
            while not child_file.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(child_file.exists())
            child = int(child_file.read_text())
            profile = Path(profile_file.read_text().strip())
            process.send_signal(signal.SIGTERM)
            process.wait(timeout=3)
            self.assertEqual(process.stdout.read(), '')
            self.assertFalse(profile.exists())
            # A reparented zombie is stopped; init may reap it asynchronously.
            state = Path(f'/proc/{child}/stat')
            self.assertTrue(not state.exists() or state.read_text().split()[2] == 'Z')
            cancel_key = hashlib.sha256((self.base + '/cancel').encode()).hexdigest()
            self.assertFalse((self.cache / (cancel_key + '.json')).exists())
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=8)
            process.stdout.close()
            process.stderr.close()


if __name__ == '__main__':
    unittest.main()
