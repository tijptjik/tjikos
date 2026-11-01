"""Saved and clipboard capture sources, using only private synthetic fixtures."""
import json
import os
from pathlib import Path
import subprocess
import struct
import zlib
import tempfile
import time
import unittest

from rust_parity import Backend, fixture


class CaptureHistoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='runtjik-history-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = fixture(self.root, 0)
        self.env.update(CLIPHIST_DB_PATH=str(self.root / 'clips.db'),
                        CLIPHIST_CONFIG_PATH=str(self.root / 'cliphist.conf'))
        (self.root / 'cliphist.conf').write_text('')
        self.saved = self.root / 'saved'
        self.other = self.root / 'other'
        self.saved.mkdir()
        self.other.mkdir()
        def chunk(kind, data):
            return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
        self.png = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', 1, 1, 8, 2, 0, 0, 0))
                    + chunk(b'IDAT', zlib.compress(b'\0\x9c\xcf\xd8')) + chunk(b'IEND', b''))
        (self.saved / 'Runtjik.png').write_bytes(self.png)
        (self.other / 'Hyprshot.png').write_bytes(self.png)
        os.utime(self.saved / 'Runtjik.png', (100, 100))
        os.utime(self.other / 'Hyprshot.png', (200, 200))
        (self.other / 'linked.png').symlink_to(self.saved / 'Runtjik.png')
        self.clip('store', b'Synthetic text mentioning png')
        self.clip('store', self.png)
        bindir = self.root / 'bin'
        bindir.mkdir()
        for name, script in {
            'wl-copy': '#!/bin/sh\ncat > "$HISTORY_COPY"\n',
            'viewer': '#!/bin/sh\ncat "$1" > "$HISTORY_VIEW"\n',
        }.items():
            path = bindir / name
            path.write_text(script)
            path.chmod(0o700)
        self.env.update(PATH=str(bindir) + ':' + os.environ['PATH'],
                        HISTORY_COPY=str(self.root / 'copy'), HISTORY_VIEW=str(self.root / 'view'))
        self.config()

    def clip(self, action, data=None):
        return subprocess.check_output(['cliphist', action], input=data, env=self.env)

    def config(self, include=True):
        Path(self.env['RUNTJIK_CONFIG']).write_text(
            '[index]\npath_commands=false\n[launch]\nisolate=false\n'
            '[plugins]\nenabled=["Screenshots","Recorder"]\n'
            '[plugin_settings.Screenshots]\n'
            f'directory={json.dumps(str(self.saved))}\n'
            f'history_directories={json.dumps([str(self.other), str(self.saved), str(self.root / "missing")])}\n'
            f'include_clipboard={str(include).lower()}\nviewer=["{self.root}/bin/viewer"]\n'
            f'[plugin_settings.Recorder]\ndirectory="{self.saved}"\n')

    def backend(self, demo=False):
        backend = Backend('rust', self.env, demo)
        self.addCleanup(backend.process.stdout.close)
        self.addCleanup(backend.close)
        return backend

    def test_sources_filter_preview_open_copy_delete(self):
        backend = self.backend()
        result = backend.request(op='query', query='/grab history')
        for _ in range(150):
            if not result.get('pending'):
                break
            time.sleep(.02)
            result = backend.request(op='query', query='/grab history')
        rows = result['results']
        self.assertEqual([row['name'] for row in rows[:2]], ['Hyprshot.png', 'Runtjik.png'])
        self.assertEqual(len(rows), 3)
        clip = rows[2]
        self.assertTrue(clip['name'].startswith('Clipboard · PNG image'))
        self.assertTrue(Path(clip['thumbnail']).is_file())
        self.assertEqual(backend.request(op='activate', id=clip['id'])['kind'], 'activated')
        for _ in range(100):
            if (self.root / 'view').exists():
                break
            time.sleep(.02)
        self.assertEqual((self.root / 'view').read_bytes(), self.png)
        reply = backend.request(op='activate', id=clip['id'], mode='copy')
        self.assertEqual(reply['kind'], 'copied')
        self.assertEqual((self.root / 'copy').read_bytes(), self.png)
        reply = backend.request(op='activate', id=clip['id'], mode='close')
        self.assertEqual(reply['query'], '/grab history')
        self.assertEqual(reply['removed_entry'], clip['id'])
        self.assertEqual(len(reply['results']), 2)
        self.assertIn(b'Synthetic text', self.clip('list'))
        self.assertTrue((self.saved / 'Runtjik.png').exists())
        self.assertEqual([row['name'] for row in backend.request(op='query', query='/ss history hyprshot')['results']], ['Hyprshot.png'])

    def test_opt_out_and_missing_cliphist_keep_files_available(self):
        self.config(include=False)
        backend = self.backend()
        self.assertEqual(len(backend.request(op='query', query='/grab history')['results']), 2)
        self.config()
        broken = self.root / 'bin/cliphist'
        broken.write_text('#!/bin/sh\nexit 1\n')
        broken.chmod(0o700)
        backend = self.backend()
        self.assertEqual(len(backend.request(op='query', query='/grab history')['results']), 2)

    def test_removed_clipboard_image_fails_without_success(self):
        backend = self.backend()
        row = backend.request(op='query', query='/grab history 1x1')['results'][0]
        self.clip('delete', row['id'].rsplit(':', 1)[1].encode())
        reply = backend.request(op='activate', id=row['id'], mode='copy')
        self.assertNotEqual(reply['kind'], 'copied')
        self.assertFalse((self.root / 'copy').exists())

    def test_viewer_keeps_original_after_backend_exits_and_cleans_up(self):
        viewer = self.root / 'bin/viewer'
        viewer.write_text('#!/bin/sh\nprintf "%s" "$1" > "$HISTORY_VIEW.path"\nsleep .2\ncat "$1" > "$HISTORY_VIEW"\n')
        backend = Backend('rust', self.env)
        try:
            row = backend.request(op='query', query='/grab history 1x1')['results'][0]
            self.assertEqual(backend.request(op='activate', id=row['id'])['kind'], 'activated')
        finally:
            backend.close()
            backend.process.stdout.close()
        for _ in range(100):
            if (self.root / 'view').exists():
                break
            time.sleep(.02)
        self.assertEqual((self.root / 'view').read_bytes(), self.png)
        original = Path((self.root / 'view.path').read_text())
        for _ in range(100):
            if not original.exists():
                break
            time.sleep(.02)
        self.assertFalse(original.exists())

    def test_history_settings_reject_invalid_paths_and_types(self):
        from rust_parity import BINARY
        for value in ['[42]', '["relative/path"]', json.dumps([str(Path(__file__).resolve().parents[1])]), 'true']:
            Path(self.env['RUNTJIK_CONFIG']).write_text(
                '[plugin_settings.Screenshots]\nhistory_directories=' + value + '\n')
            result = subprocess.run([str(BINARY), 'check'], env=self.env, capture_output=True)
            self.assertNotEqual(result.returncode, 0, value)

    def test_demo_never_reads_private_sources(self):
        before = self.clip('list')
        rows = self.backend(demo=True).request(op='query', query='/grab history')['results']
        self.assertEqual(len(rows), 9)
        self.assertTrue(all(row['name'].startswith('Demo capture') for row in rows))
        self.assertEqual(before, self.clip('list'))


if __name__ == '__main__':
    unittest.main(verbosity=2)
