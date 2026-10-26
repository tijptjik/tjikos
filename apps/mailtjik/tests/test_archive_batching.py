"""Exercise encrypted chunk boundaries, folder aliases and bounded retrieval."""
import hashlib
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))
from email_archive_annual import Engine, Store, SCHEMA, connect, file_hash, sha
from email_archives import retrieve


class LocalStore(Store):
    def __init__(self, root, recipient, identity):
        super().__init__(root / "work", recipient, identity, "synthetic")
        self.remote_files = root / "remote"
        self.remote_files.mkdir()
        self.downloads = 0
        self.uploads = 0

    def upload(self, path, key):
        self.uploads += 1
        target = self.remote_files / hashlib.sha256(key.encode()).hexdigest()
        shutil.copyfile(path, target)
        self.assert_digest = file_hash(target)
        assert self.assert_digest == file_hash(path)
        return self.assert_digest

    def rclone(self, *args, **kwargs):
        assert args[0] == "copyto"
        self.downloads += 1
        key = args[1].removeprefix("synthetic/")
        shutil.copyfile(self.remote_files / sha(key.encode()), args[2])


@unittest.skipUnless(shutil.which("age-keygen") and shutil.which("zstd"), "age and zstd required")
class EncryptedBatchingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="mailtjik-fixture-")
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        identity = root / "key"
        subprocess.run(["age-keygen", "-o", str(identity)], check=True, capture_output=True)
        recipient = subprocess.check_output(["age-keygen", "-y", str(identity)], text=True).strip()
        self.store = LocalStore(root, recipient, identity)
        self.db = connect(root / "ledger.sqlite")
        self.addCleanup(self.db.close)
        self.db.executescript(SCHEMA)
        self.engine = Engine(self.store, self.db, "fixture")

    def test_many_folders_share_one_encrypted_record_and_resume(self):
        raw = b"Subject: synthetic\r\n\r\nbody\r\n"
        record = b"From fixture Thu Jan 01 00:00:00 1970\n" + raw
        digest = sha(raw)
        for n in range(100):
            self.engine.put("reader", "fixture@example.invalid", "2025", digest, record, f"folder-{n}")
        self.engine.flush()
        self.assertEqual(self.store.uploads, 1)
        self.assertEqual(self.db.execute("SELECT sum(records) FROM objects").fetchone()[0], 1)
        self.assertEqual(self.db.execute("SELECT count(*) FROM locations").fetchone()[0], 100)
        self.assertEqual(self.db.execute("SELECT count(DISTINCT offset) FROM locations").fetchone()[0], 1)
        self.assertFalse(self.engine.pending)
        resumed = Engine(self.store, self.db, "fixture")
        resumed.put("reader", "fixture@example.invalid", "2025", digest, record, "new-folder")
        resumed.flush()
        self.assertEqual(self.store.uploads, 1)
        self.assertEqual(self.db.execute("SELECT count(*) FROM locations").fetchone()[0], 101)

    def test_chunk_split_and_single_chunk_cached_restore(self):
        self.engine.part_bytes = 100
        raws = [b"Subject: synthetic " + str(n).encode() + b"\r\n\r\n" + b"x" * 60 for n in range(3)]
        for raw in raws:
            self.engine.put("raw", "fixture@example.invalid", "2025", sha(raw), raw)
        self.engine.flush()
        self.assertEqual(self.store.uploads, 3)
        for n in range(2):
            target = Path(self.temp.name) / f"restored-{n}.eml"
            self.assertTrue(retrieve(self.store, self.db, sha(raws[1]), "raw", target))
            self.assertEqual(target.read_bytes(), raws[1])
        self.assertEqual(self.store.downloads, 1)


if __name__ == "__main__":
    unittest.main()
