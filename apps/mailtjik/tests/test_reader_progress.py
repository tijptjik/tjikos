import hashlib
from pathlib import Path
from types import SimpleNamespace
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'backend'))
from mailtjik_progress import QueryProgress

class ReaderProgressTests(unittest.TestCase):
    def test_transfer_eta_verification_and_search(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            store = SimpleNamespace(workdir=root, cache=root)
            part = {'index': {'path': 'synthetic', 'plaintext_sha256': 'abc', 'cipher_bytes': 262144}}
            path = root / (hashlib.sha256(b'synthetic').hexdigest() + '.partial')
            path.write_bytes(b'x' * 65536)
            now = [0.0]
            progress = QueryProgress(store, [part], clock=lambda: now[0])
            now[0] = 4
            self.assertIsNone(progress.snapshot()['eta_seconds'])
            path.write_bytes(b'x' * 131072)
            snap = progress.snapshot()
            self.assertEqual(snap['eta_seconds'], 8)
            self.assertEqual(snap['ready_indexes'], 0)
            path.write_bytes(b'x' * 262144)
            self.assertEqual(progress.snapshot()['phase'], 'verifying')
            progress.index_ready()
            progress.searching()
            progress.index_searched()
            snap = progress.snapshot()
            self.assertEqual(snap['phase'], 'searching')
            self.assertEqual(snap['searched_indexes'], 1)
            self.assertNotIn('synthetic', str(snap))

    def test_warm_cache_and_exception_stops_monitor(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'indexes').mkdir()
            (root / 'indexes/abc.sqlite').touch()
            part = {'index': {'path': 'private', 'plaintext_sha256': 'abc', 'cipher_bytes': 100}}
            events = []
            progress = QueryProgress(SimpleNamespace(workdir=root, cache=root), [part], events.append)
            with self.assertRaises(ValueError):
                with progress:
                    raise ValueError('fixture')
            self.assertFalse(progress.thread.is_alive())
            self.assertEqual(events[0]['phase'], 'checking')
            self.assertEqual(events[0]['cached_indexes'], 1)
            self.assertEqual(events[0]['ready_indexes'], 0)
            self.assertIsNone(events[0]['eta_seconds'])
