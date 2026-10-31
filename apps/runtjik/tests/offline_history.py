"""Exercise closed-browser history through the native launcher with synthetic data."""
import json
from pathlib import Path
import sqlite3
import tempfile
import time
import unittest

from rust_parity import Backend, fixture
from browser_fixture import BrowserFixture


class OfflineHistoryTests(unittest.TestCase):
    def test_closed_browser_search_and_profile_launch(self):
        with tempfile.TemporaryDirectory(prefix="runtjik-offline-history-") as temp:
            root = Path(temp)
            env = fixture(root, count=0)
            runtime = root / "runtime"
            runtime.mkdir(mode=0o700)
            env["XDG_RUNTIME_DIR"] = str(runtime)
            profile = root / "profile with spaces"
            profile.mkdir()
            database = profile / "places.sqlite"
            writer = sqlite3.connect(database)
            writer.executescript("""
                PRAGMA journal_mode=WAL;
                CREATE TABLE moz_places(guid TEXT, url TEXT, title TEXT,
                    visit_count INTEGER, last_visit_date INTEGER, hidden INTEGER);
                INSERT INTO moz_places VALUES
                    ('synthetic', 'https://example.org/history', 'Synthetic visit', 2, 2000000, 0),
                    ('unsafe', 'file:///private', 'Private file', 1, 3000000, 0);
            """)
            writer.commit()
            received = root / "launch.json"
            launcher = root / "synthetic-browser"
            launcher.write_text(
                "#!/usr/bin/env python3\nimport json, sys\nfrom pathlib import Path\n"
                f"Path({str(received)!r}).write_text(json.dumps(sys.argv[1:]))\n"
            )
            launcher.chmod(0o700)
            with (root / "runtjik.conf").open("a") as config:
                config.write(
                    '\n[plugins]\nenabled=["Browser"]\n[plugin_settings.Browser]\n'
                    f"history_profiles = [{json.dumps(str(profile))}]\n"
                    f"history_browser = [{json.dumps(str(launcher))}]\n"
                )
            backend = Backend("rust", env)
            try:
                self.assertNotIn("bridge disconnected", backend.initial.get("message", ""))
                response = backend.request(op="query", query="/history synthetic")
                self.assertEqual(len(response["results"]), 1, response.get("message"))
                entry = response["results"][0]
                self.assertIn("offline", entry["subtitle"])
                denied = backend.request(op="activate", id=entry["id"], mode="close")
                self.assertIn("read-only", denied["message"])
                opened = backend.request(op="activate", id=entry["id"])
                self.assertEqual(opened["kind"], "activated")
                deadline = time.monotonic() + 5
                while not received.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertEqual(json.loads(received.read_text()), [
                    "--profile", str(profile), "--new-tab", "https://example.org/history"
                ])
                writer.execute("DELETE FROM moz_places WHERE guid = 'synthetic'")
                writer.commit()
                stale = backend.request(op="activate", id=entry["id"])
                self.assertIn("removed", stale["message"])
                empty = backend.request(op="query", query="/history")
                self.assertEqual(empty["results"], [])
            finally:
                backend.close()
                backend.process.stdout.close()
                writer.close()


class OfflineHistorySearchTests(BrowserFixture):
    def test_title_url_literal_search_and_excluded_visits(self):
        self.profile([
            ("title", "https://example.org/a", "A MiXeD title 100%_done 中文", 1, 1000, 0),
            ("url", "http://example.org/MiXeD", None, 1, 2000, 0),
            ("file", "file:///mixed", "mixed", 1, 9000, 0),
            ("hidden", "https://example.org/hidden", "mixed", 1, 8000, 1),
            ("unvisited", "https://example.org/bookmark", "mixed", 0, 7000, 0),
            ("undated", "https://example.org/undated", "mixed", 1, None, 0),
        ])
        self.start()
        self.assertEqual([r["name"] for r in self.query("/history mixed")["results"]],
                         ["http://example.org/MiXeD", "A MiXeD title 100%_done 中文"])
        for term in ["100%_done", "中文", "EXAMPLE.ORG/A"]:
            with self.subTest(term=term):
                self.assertEqual(len(self.query("/history " + term)["results"]), 1)
        self.assertEqual(self.query("/history absent")["results"], [])
        self.assertIn("too long", self.query("/history " + "x" * 513)["message"])
        self.assertEqual(self.query("MiXeD")["results"], [])

    def test_global_recency_cap_pagination_and_matching_before_limit(self):
        for parity in range(2):
            self.profile([
                (f"visit-{i}", f"https://example.org/{i}",
                 "Old needle" if i == 0 else f"Visit {i}",
                 1000 if i == 0 else 1, (i + 1) * 1000, 0)
                for i in range(parity, 240, 2)
            ])
        self.start()
        response = self.query(limit=200)
        self.assertEqual(response["total"], 100)
        self.assertEqual([r["name"] for r in response["results"]],
                         [f"Visit {i}" for i in range(239, 139, -1)])
        page = self.query(offset=95, limit=20)
        self.assertEqual(len(page["results"]), 5)
        self.assertFalse(page["has_more"])
        needle = self.query("/history needle")["results"]
        self.assertEqual([r["name"] for r in needle], ["Old needle"])
        second = self.query("/history example.org/239")["results"][0]
        self.assertEqual(self.backend.request(op="activate", id=second["id"])["kind"], "activated")
        self.assertEqual(self.opened(), ["--profile", str(self.profiles[1]),
                                       "--new-tab", "https://example.org/239"])

    def test_committed_wal_changes_and_changed_url_revalidation(self):
        _, writer = self.profile([
            ("visit", "https://example.org/old", "Original", 1, 1000, 0),
        ])
        self.start()
        entry = self.query()["results"][0]
        denied = self.backend.request(op="activate", id=entry["id"], mode="close")
        self.assertIn("read-only", denied["message"])
        self.assertEqual(writer.execute("SELECT count(*) FROM moz_places").fetchone()[0], 1)
        writer.execute("UPDATE moz_places SET url = 'https://example.org/new', title = 'Updated'")
        writer.commit()
        stale = self.backend.request(op="activate", id=entry["id"])
        self.assertIn("removed", stale["message"])
        self.assertFalse(self.received.exists())
        self.assertEqual(self.query()["results"][0]["name"], "Updated")
        writer.execute("UPDATE moz_places SET hidden = 1")
        self.assertEqual(len(self.query()["results"]), 1)  # Uncommitted writes stay invisible.
        writer.commit()
        self.assertEqual(self.query()["results"], [])

    def test_missing_profile_reports_error_without_creating_database(self):
        missing = self.root / "missing profile"
        self.profiles.append(missing)
        self.start()
        response = self.query()
        self.assertEqual(response["results"], [])
        self.assertIn("check the profile path", response["message"])
        self.assertFalse(missing.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
