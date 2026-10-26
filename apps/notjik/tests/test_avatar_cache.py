import copy
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import notjik


class AvatarCacheTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        env = patch.dict(os.environ, {"NOTJIK_STATE_DIR": str(self.root), "NOTJIK_TELEGRAM_DIR": str(self.root / "telegram")})
        env.start()
        self.addCleanup(env.stop)
        self.db = notjik.database()
        self.addCleanup(self.db.close)
        self.photo = self.root / "telegram/db/profile_photos/photo.jpg"
        self.photo.parent.mkdir(parents=True)
        self.photo.write_bytes(b"synthetic full size photo")
        self.notice = {"id":"test", "type":"telegram", "title":"Maya", "image":self.photo.as_uri(), "data":{"chatId":"123"}}

    def test_persists_without_history_and_survives_source_eviction(self):
        cfg = copy.deepcopy(notjik.DEFAULTS)
        cfg["history"]["enabled"] = False
        notjik.record(self.db, cfg, {"event":"received", "notice":self.notice})
        image = notjik.avatar_index(self.db)["byChat"]["123"]
        self.assertNotEqual(image, self.photo.as_uri())
        self.photo.unlink()
        self.assertEqual(notjik.avatar_index(self.db)["byTitle"]["Maya"], image)
        self.assertEqual(Path(image.removeprefix("file://")).stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM events").fetchone()[0], 0)

    def test_namesakes_do_not_share_native_avatar(self):
        notjik.cache_avatar(self.db, self.notice)
        notjik.cache_avatar(self.db, {**self.notice, "data":{"chatId":"456"}})
        index = notjik.avatar_index(self.db)
        self.assertEqual(index["byTitle"]["Maya"], "")
        self.assertEqual(len(index["byChat"]), 2)

    def test_native_thumbnail_and_demo_cannot_replace_photo(self):
        notjik.cache_avatar(self.db, self.notice)
        expected = notjik.avatar_index(self.db)
        for notice in ({**self.notice, "type":"standard", "image":"image://qsimage/1"},
                       {**self.notice, "data":{"chatId":"123", "demo":True}},
                       {**self.notice, "image":(self.root / "outside.jpg").as_uri()}):
            notjik.cache_avatar(self.db, notice)
        self.assertEqual(notjik.avatar_index(self.db), expected)

    def test_new_photo_replaces_old_cache_file(self):
        notjik.cache_avatar(self.db, self.notice)
        old = notjik.avatar_index(self.db)["byChat"]["123"]
        self.photo.write_bytes(b"updated photo")
        notjik.cache_avatar(self.db, self.notice)
        self.assertNotEqual(notjik.avatar_index(self.db)["byChat"]["123"], old)
        self.assertFalse(Path(old.removeprefix("file://")).exists())

    def test_upgrade_backfills_existing_history(self):
        with self.db:
            self.db.execute("DROP TABLE avatars")
            self.db.execute("INSERT INTO events(time,event,payload) VALUES(?,?,?)", ("now", "received", json.dumps({"notice":self.notice})))
        other = notjik.database()
        try:
            self.assertIn("123", notjik.avatar_index(other)["byChat"])
        finally:
            other.close()
