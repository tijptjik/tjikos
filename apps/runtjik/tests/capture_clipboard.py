"""Copy capture history items with isolated files and a private clipboard sink."""
import os
from pathlib import Path
import unittest
import tempfile

from rust_parity import Backend, fixture


class CaptureClipboardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='runtjik-capture-copy-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = fixture(self.root, 0)
        runtime = self.root / 'runtime'
        runtime.mkdir(mode=0o700)
        self.env['XDG_RUNTIME_DIR'] = str(runtime)
        self.captures = self.root / 'captures'
        self.captures.mkdir()
        Path(self.env['RUNTJIK_CONFIG']).write_text(
            '[index]\npath_commands=false\n[plugins]\nenabled=["Screenshots","Recorder"]\n'
            f'[plugin_settings.Screenshots]\ndirectory="{self.captures}"\n'
            f'[plugin_settings.Recorder]\ndirectory="{self.captures}"\n')
        bindir = self.root / 'bin'
        bindir.mkdir()
        copy = bindir / 'wl-copy'
        copy.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$COPY_ARGS"\ncat > "$COPY_DATA"\n')
        copy.chmod(0o700)
        self.env.update(PATH=str(bindir) + ':' + os.environ['PATH'],
                        COPY_ARGS=str(self.root / 'args'), COPY_DATA=str(self.root / 'data-copy'))

    def backend(self, demo=False):
        backend = Backend('rust', self.env, demo)
        self.addCleanup(backend.process.stdout.close)
        self.addCleanup(backend.close)
        return backend

    def test_images_and_videos_copy_payloads_without_opening_or_deleting(self):
        backend = self.backend()
        for ext, mime in [('png', 'image/png'), ('jpg', 'image/jpeg'),
                          ('JPEG', 'image/jpeg'), ('webp', 'image/webp'),
                          ('mp4', 'text/uri-list'), ('mkv', 'text/uri-list'),
                          ('webm', 'text/uri-list')]:
            with self.subTest(ext=ext):
                path = self.captures / f'capture space # 中文.{ext}'
                payload = b'synthetic fixture\x00\xff\n'
                path.write_bytes(payload)
                query = '/rec history' if mime == 'text/uri-list' else '/grab history'
                rows = backend.request(op='query', query=query)['results']
                row = next(row for row in rows if row['name'] == path.name)
                result = backend.request(op='activate', id=row['id'], mode='copy')
                self.assertEqual(result['kind'], 'copied')
                self.assertEqual(result['entry'], row['id'])
                self.assertIn('Copied', result['message'])
                self.assertEqual((self.root / 'args').read_text(), f'--type\n{mime}\n')
                expected = (path.as_uri() + '\r\n').encode() if mime == 'text/uri-list' else payload
                self.assertEqual((self.root / 'data-copy').read_bytes(), expected)
                self.assertEqual(path.read_bytes(), payload)

    def test_missing_or_replaced_file_does_not_touch_clipboard(self):
        path = self.captures / 'gone.png'
        path.write_bytes(b'synthetic')
        backend = self.backend()
        row = backend.request(op='query', query='/grab history')['results'][0]
        path.unlink()
        for symlink in [False, True]:
            if symlink:
                target = self.root / 'outside.png'
                target.write_bytes(b'outside')
                path.symlink_to(target)
            result = backend.request(op='activate', id=row['id'], mode='copy')
            self.assertIn('Capture is no longer available', result['message'])
            self.assertFalse((self.root / 'data-copy').exists())

    def test_copy_failure_is_reported(self):
        path = self.captures / 'failure.png'
        path.write_bytes(b'synthetic')
        (self.root / 'bin/wl-copy').write_text('#!/bin/sh\nexit 1\n')
        backend = self.backend()
        row = backend.request(op='query', query='/grab history')['results'][0]
        result = backend.request(op='activate', id=row['id'], mode='copy')
        self.assertIn('Could not copy capture to clipboard', result['message'])

    def test_demo_never_changes_clipboard(self):
        backend = self.backend(demo=True)
        for query in ['/grab history', '/rec history']:
            row = backend.request(op='query', query=query)['results'][0]
            result = backend.request(op='activate', id=row['id'], mode='copy')
            self.assertEqual(result['message'], 'Demo capture copy; clipboard unchanged')
            self.assertFalse((self.root / 'data-copy').exists())

    def test_copy_rejects_commands_and_empty_gallery(self):
        backend = self.backend()
        backend.request(op='query', query='/grab history')
        for entry in ['Screenshots:empty', '/refresh', 'Recorder:stop']:
            result = backend.request(op='activate', id=entry, mode='copy')
            self.assertIn('Select an image or video to copy', result['message'])
            self.assertFalse((self.root / 'data-copy').exists())


if __name__ == '__main__':
    unittest.main(verbosity=2)
