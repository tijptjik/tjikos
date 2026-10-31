"""Synthetic Places profiles and real native hosts; never use a personal browser."""
import json
from pathlib import Path
import sqlite3
import struct
import subprocess
import tempfile
import threading
import time
import unittest

from rust_parity import Backend, BINARY, fixture


class BrowserFixture(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="runtjik-browser-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.env = fixture(self.root, count=0)
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)
        self.env["XDG_RUNTIME_DIR"] = str(self.runtime)
        self.profiles = []
        self.received = self.root / "launch.json"
        self.launcher = self.root / "synthetic-browser"
        self.launcher.write_text(
            "#!/usr/bin/env python3\nimport json, sys\nfrom pathlib import Path\n"
            f"Path({str(self.received)!r}).write_text(json.dumps(sys.argv[1:]))\n"
        )
        self.launcher.chmod(0o700)

    def profile(self, rows):
        profile = self.root / f"profile {len(self.profiles)} with spaces"
        profile.mkdir()
        self.profiles.append(profile)
        db = sqlite3.connect(profile / "places.sqlite")
        self.addCleanup(db.close)
        db.executescript("""
            PRAGMA journal_mode=WAL;
            CREATE TABLE moz_places(guid TEXT, url TEXT, title TEXT,
                visit_count INTEGER, last_visit_date INTEGER, hidden INTEGER);
        """)
        db.executemany("INSERT INTO moz_places VALUES (?, ?, ?, ?, ?, ?)", rows)
        db.commit()
        return profile, db

    def start(self):
        with (self.root / "runtjik.conf").open("a") as config:
            config.write(
                '\n[plugins]\nenabled=["Browser"]\n[plugin_settings.Browser]\n'
                f"history_profiles = {json.dumps([str(p) for p in self.profiles])}\n"
                f"history_browser = {json.dumps([str(self.launcher)])}\n"
            )
        self.backend = Backend("rust", self.env)
        self.addCleanup(self.backend.process.stdout.close)
        self.addCleanup(self.backend.close)

    def query(self, text="/history", **fields):
        return self.backend.request(op="query", query=text, **fields)

    def opened(self):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                return json.loads(self.received.read_text())
            except (FileNotFoundError, json.JSONDecodeError):
                time.sleep(0.01)
        self.fail("Synthetic browser did not receive a launch")

    def bridge(self, history):
        bridge = NativeBridge(self.env, self.runtime, history)
        self.addCleanup(bridge.close)
        return bridge


class NativeBridge:
    """Real Rust host with a synthetic extension on its native-messaging pipes."""
    def __init__(self, env, runtime, history):
        self.history = history
        self.error = None
        self.requests = []
        self.failures = []
        directory = runtime / "runtjik-browser"
        previous = set(directory.glob("*.sock"))
        self.process = subprocess.Popen(
            [str(BINARY), "browser-host"], env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        )
        self.worker = threading.Thread(target=self.serve, daemon=True)
        self.worker.start()
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            paths = set(directory.glob("*.sock")) - previous
            if paths:
                self.path = paths.pop()
                return
            if self.process.poll() is not None:
                break
            time.sleep(0.01)
        self.close()
        raise AssertionError("Native host did not create its session socket")

    def read_exact(self, size):
        data = b""
        while len(data) < size:
            chunk = self.process.stdout.read(size - len(data))
            if not chunk:
                raise EOFError
            data += chunk
        return data

    def serve(self):
        try:
            while True:
                size = struct.unpack("=I", self.read_exact(4))[0]
                message = json.loads(self.read_exact(size))
                self.requests.append(message)
                if self.error:
                    response = {"error": self.error}
                elif message["op"] == "history":
                    term = message["text"].lower()
                    response = {"history": [row for row in self.history if term in
                        (row["title"] + " " + row["url"]).lower()]}
                elif message["op"] == "list":
                    response = {"windows": [{"id": 1, "title": "Synthetic window",
                        "tabs": [{"id": 11, "title": "Synthetic tab",
                                  "url": "https://example.org/live"}]}]}
                else:
                    response = {}
                data = json.dumps({"id": message["id"], **response}).encode()
                self.process.stdin.write(struct.pack("=I", len(data)) + data)
                self.process.stdin.flush()
        except EOFError:
            pass
        except Exception as error:
            self.failures.append(error)

    def close(self, crash=False):
        if self.process.poll() is None:
            if crash:
                self.process.kill()
            else:
                self.process.stdin.close()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
                raise AssertionError("Native host did not stop")
        self.worker.join(timeout=5)
        self.process.stdout.close()
        self.process.stdin.close()
        assert not self.worker.is_alive(), "Synthetic extension did not stop"
        assert not self.failures, self.failures
