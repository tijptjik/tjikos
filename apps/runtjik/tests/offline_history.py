"""Exercise closed-browser history through the native launcher with synthetic data."""
import json
from pathlib import Path
import sqlite3
import tempfile
import time
import unittest

from rust_parity import Backend, fixture


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


if __name__ == "__main__":
    unittest.main(verbosity=2)
