"""Clipboard filtering and deletion against isolated synthetic cliphist data."""
import base64
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

from rust_parity import Backend, fixture


@unittest.skipUnless(shutil.which('cliphist'), 'cliphist is required')
class ClipboardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='runtjik-clipboard-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = fixture(self.root, 0)
        Path(self.env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Clipboard"]\n')
        # Pin every invocation to this test database, regardless of host settings.
        self.env['CLIPHIST_DB_PATH'] = str(self.root / 'clips.db')
        self.env['CLIPHIST_CONFIG_PATH'] = str(self.root / 'cliphist.conf')
        (self.root / 'cliphist.conf').write_text('')
        self.clip('store', b'Synthetic note about png images')
        self.clip('store', b'https://example.com')
        self.png = base64.b64decode(
            'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a1XkAAAAASUVORK5CYII=')
        self.clip('store', self.png)

    def clip(self, action, data=None):
        return subprocess.check_output(['cliphist', action], input=data, env=self.env)

    def backend(self, demo=False):
        backend = Backend('rust', self.env, demo)
        self.addCleanup(backend.close)
        return backend

    def test_filter_and_delete_image_preserve_text_and_query(self):
        backend = self.backend()
        mixed = backend.request(op='query', query='/cliphist')['results']
        self.assertEqual(len(mixed), 3)
        query = '/cliphist images png'
        images = backend.request(op='query', query=query)['results']
        self.assertEqual(len(images), 1)
        self.assertTrue(images[0]['name'].startswith('PNG image'))
        result = backend.request(op='activate', id=images[0]['id'], mode='close')
        self.assertEqual(result['kind'], 'results')
        self.assertEqual(result['query'], query)
        self.assertEqual(result['message'], 'Deleted clipboard item')
        self.assertEqual(result['removed_entry'], images[0]['id'])
        self.assertEqual(result['results'][0]['name'], 'No matching clipboard images')
        remaining = self.clip('list')
        self.assertIn(b'Synthetic note about png images', remaining)
        self.assertIn(b'https://example.com', remaining)
        self.assertNotIn(b'binary data', remaining)
        result = backend.request(op='activate', id='Clipboard:empty', mode='close')
        self.assertEqual(result['message'], 'No clipboard item selected')
        self.assertEqual(self.clip('list'), remaining)

    def test_text_deletion_and_copy(self):
        # Capture wl-copy stdin in a private file; never take over the desktop clipboard.
        bindir = self.root / 'bin'
        bindir.mkdir()
        copy = bindir / 'wl-copy'
        copy.write_text('#!/bin/sh\ncat > "$CLIPBOARD_TEST_COPY"\n')
        copy.chmod(0o700)
        self.env.update(PATH=str(bindir) + ':' + os.environ['PATH'],
                        CLIPBOARD_TEST_COPY=str(self.root / 'copy'))
        backend = self.backend()
        rows = backend.request(op='query', query='/cliphist example')['results']
        result = backend.request(op='activate', id=rows[0]['id'])
        self.assertEqual(result['kind'], 'activated')
        self.assertEqual((self.root / 'copy').read_bytes(), b'https://example.com')
        result = backend.request(op='activate', id=rows[0]['id'], mode='close')
        self.assertEqual(result['query'], '/cliphist example')
        self.assertEqual(result['results'][0]['name'], 'No matching clipboard items')
        self.assertNotIn(b'https://example.com', self.clip('list'))
        self.assertIn(b'binary data', self.clip('list'))

    def test_dedupe_uses_full_content_not_truncated_labels(self):
        from PIL import Image, PngImagePlugin
        import io
        def png(color, note):
            stream = io.BytesIO()
            metadata = PngImagePlugin.PngInfo()
            metadata.add_text('note', note)
            Image.new('RGB', (12, 12), color).save(stream, format='PNG', pnginfo=metadata)
            return base64.b64encode(stream.getvalue()).decode()
        values = {'6': png('red', 'first'), '5': png('red', 'other encoding'), '4': png('blue', 'different pixels'),
                  '3': base64.b64encode(b'same preview - one').decode(),
                  '2': base64.b64encode(b'same preview - two').decode(),
                  '1': base64.b64encode(b'same preview - one').decode()}
        bindir = self.root / 'bin'
        bindir.mkdir()
        script = bindir / 'cliphist'
        script.write_text('#!/usr/bin/python3\nimport sys,base64\nvalues=' + repr(values)
            + "\nif sys.argv[1]=='list':\n for key in values: print(key+'\\t'+('[[ binary data 100 bytes png 12x12 ]]' if int(key)>3 else 'same preview'))\n"
            + "elif sys.argv[1]=='decode': sys.stdout.buffer.write(base64.b64decode(values[sys.stdin.read().strip()]))\n")
        script.chmod(0o700)
        self.env['PATH'] = str(bindir) + ':' + os.environ['PATH']
        backend = self.backend()
        for _ in range(200):
            result = backend.request(op='query', query='/cliphist')
            if not result.get('pending'):
                break
            time.sleep(.02)
        self.assertFalse(result.get('pending'))
        self.assertEqual([r['id'] for r in result['results']], ['Clipboard:6', 'Clipboard:4', 'Clipboard:3', 'Clipboard:2'])

    def test_demo_filters_and_never_deletes(self):
        before = self.clip('list')
        backend = self.backend(demo=True)
        for query in ['/cliphist images', '/cliphist IMAGES png', '/cliphist images 32x32']:
            rows = backend.request(op='query', query=query)['results']
            self.assertEqual([row['id'] for row in rows], ['Clipboard:3'])
        result = backend.request(op='activate', id='Clipboard:3', mode='close')
        self.assertEqual(result['message'], 'Demo clipboard deletion; history unchanged')
        self.assertEqual(result['results'][0]['id'], 'Clipboard:3')
        self.assertEqual(self.clip('list'), before)


if __name__ == '__main__':
    unittest.main(verbosity=2)
