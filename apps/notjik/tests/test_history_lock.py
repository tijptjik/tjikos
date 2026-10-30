import copy
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
import notjik
from lock_watch import LockObserver


def event(object_id, opcode, payload=b""):
    return struct.pack("=II", object_id, ((8 + len(payload)) << 16) | opcode) + payload


class LockTests(unittest.TestCase):
    def test_initial_state_requires_subscription_roundtrip(self):
        observer = LockObserver()
        self.assertEqual(observer.state, "unknown")
        observer.feed(event(3, 0, struct.pack("=I", 1)))
        self.assertEqual(observer.state, "unknown")
        observer.feed(event(6, 0, struct.pack("=I", 1)))
        self.assertEqual(observer.state, "unlocked")

    def test_started_locked_does_not_flash_unlocked(self):
        observer = LockObserver()
        observer.feed(event(5, 0) + event(6, 0, struct.pack("=I", 1)))
        self.assertEqual(observer.state, "locked")
        observer.feed(event(5, 1))
        self.assertEqual(observer.state, "unlocked")
        observer.close()
        self.assertEqual(observer.state, "unknown")

    def test_fragmented_events(self):
        observer = LockObserver()
        data = event(5, 0)
        observer.feed(data[:3])
        self.assertEqual(observer.state, "unknown")
        observer.feed(data[3:])
        self.assertEqual(observer.state, "locked")


class HistoryTests(unittest.TestCase):
    def test_weekly_retention_has_no_count_cap_and_expires_without_new_events(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"NOTJIK_STATE_DIR": directory}):
            db = notjik.database()
            self.addCleanup(db.close)
            now = dt.datetime(2026, 10, 25, 12, tzinfo=dt.timezone.utc)
            cutoff = now - dt.timedelta(days=7)
            history = {**notjik.DEFAULTS["history"], "maxEvents": 0, "maxAgeDays": 7, "drawerLimit": 0}
            with db:
                for i in range(1205):
                    moment = cutoff - dt.timedelta(milliseconds=1) if i == 0 else cutoff + dt.timedelta(milliseconds=i-1)
                    event = {"session": "week", "event": "received", "time": moment.isoformat(timespec="milliseconds"),
                             "notice": {"id": str(i), "received": moment.timestamp()*1000, "title": str(i)}}
                    db.execute("INSERT INTO events(time,event,notice_id,app,payload) VALUES(?,?,?,?,?)",
                               (event["time"], "received", str(i), "demo", json.dumps(event)))
                self.assertTrue(notjik.prune_history(db, history, now))
            items = notjik.history_notices(db, 0)
            self.assertEqual(len(items), 1204)
            self.assertEqual(items[-1]["historyKey"], "week/1")  # Exact boundary is retained.
            self.assertEqual(len(notjik.history_notices(db, 1000)), 1000)
            with db:
                self.assertFalse(notjik.prune_history(db, history, now))
                self.assertTrue(notjik.prune_history(db, history, now + dt.timedelta(days=1)))
            self.assertEqual(notjik.history_notices(db, 0), [])

    def test_worker_expires_idle_history_without_receiving_another_notification(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"NOTJIK_STATE_DIR": directory}):
            db = notjik.database()
            cfg = copy.deepcopy(notjik.DEFAULTS)
            cfg["history"].update(maxEvents=0, maxAgeDays=7, drawerLimit=0)
            notjik.record(db, cfg, {"event":"received", "notice":{"id":"idle"}})
            snapshots = []
            def receive(value):
                if "history" not in value:
                    return
                snapshots.append(value["history"])
                if len(snapshots) == 1:
                    old = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=8)).isoformat(timespec="milliseconds")
                    with db:
                        db.execute("UPDATE events SET time = ?", (old,))
            observer = Mock(socket=None, state="unlocked")
            with patch.object(notjik, "database", return_value=db), \
                    patch.object(notjik, "load_config", return_value=cfg), \
                    patch.object(notjik, "LockObserver", return_value=observer), \
                    patch.object(notjik, "emit", side_effect=receive), \
                    patch.object(notjik.time, "monotonic", side_effect=[1, 62]), \
                    patch.object(notjik.select, "select", side_effect=[([], [], []), ([sys.stdin], [], [])]), \
                    patch.object(notjik.os, "read", return_value=b""), patch.object(notjik.os, "umask"):
                notjik.worker(ROOT)
            self.assertEqual([len(items) for items in snapshots], [1, 0])
            observer.close.assert_called_once()

    def test_age_and_count_limits_combine_and_disabled_history_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"NOTJIK_STATE_DIR": directory}):
            db = notjik.database()
            self.addCleanup(db.close)
            cfg = copy.deepcopy(notjik.DEFAULTS)
            cfg["history"].update(maxEvents=2, maxAgeDays=7)
            for i in range(3):
                notjik.record(db, cfg, {"event":"received", "notice":{"id":str(i)}})
            self.assertEqual(len(notjik.history_notices(db, 0)), 2)
            cfg["history"]["enabled"] = False
            with db:
                self.assertFalse(notjik.prune_history(db, cfg["history"], dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=8)))
            self.assertEqual(len(notjik.history_notices(db, 0)), 2)

    def test_dedup_keeps_sessions_separate_and_never_replays_actions(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"NOTJIK_STATE_DIR": directory}):
            db = notjik.database()
            try:
                for session, kind, title in [("old", "received", "old title"), ("old", "dismissed", "updated"), ("new", "received", "new session")]:
                    notjik.record(db, notjik.DEFAULTS, {"session": session, "event": kind,
                        "notice": {"id": "local-1", "title": title, "received": 1 if session == "old" else 2,
                                   "image": "image://temporary", "actions": [{"label": "Run", "command": ["false"]}]}})
                items = notjik.history_notices(db, 100)
                self.assertEqual(len(items), 2)
                self.assertEqual(items[0]["historyKey"], "new/local-1")
                self.assertEqual(items[1]["title"], "updated")
                self.assertEqual(items[1]["lastEvent"], "dismissed")
                self.assertEqual(items[0]["actions"], [])
                self.assertEqual(items[0]["image"], "")
            finally:
                db.close()
