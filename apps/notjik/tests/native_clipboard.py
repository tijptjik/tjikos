"""Native clipboard CLI contracts using synthetic clipboard, socket and IPC fixtures."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import unittest

APP = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("NOTJIK_TEST_BINARY", APP / "dist/bin/notjik-core"))


class NativeClipboardTests(unittest.TestCase):
    def run_clipboard(self, *, connected=True, response=None, types="text/plain", data=b"**synthetic**\n"):
        temporary = tempfile.TemporaryDirectory(prefix="notjik-clipboard-cli-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        (root / "bin").mkdir()
        (root / "data").write_bytes(data)
        (root / "types").write_text(types)
        paste = root / "bin/wl-paste"
        paste.write_text("#!/usr/bin/python3\nimport os, pathlib, sys\np = pathlib.Path(os.environ['FIXTURE'])\n"
                         "with (p / 'reads').open('a') as f: f.write('read\\n')\n"
                         "sys.stdout.buffer.write((p / ('types' if '--list-types' in sys.argv else 'data')).read_bytes())\n")
        paste.chmod(0o755)
        ipc = root / "bin/quickshell"
        ipc.write_text("#!/usr/bin/python3\nimport os, pathlib, sys\np = pathlib.Path(os.environ['FIXTURE'])\n"
                       "(p / 'notice').write_bytes(pathlib.Path(sys.argv[-1]).read_bytes())\nprint('{\"ok\":true}')\n")
        ipc.chmod(0o755)
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(root / "control.sock"))
        server.listen()
        server.settimeout(10)
        self.addCleanup(server.close)
        requests, errors = [], []

        def serve():
            try:
                for index in range(2 if connected and types != "application/octet-stream" else 1):
                    conn, _ = server.accept()
                    with conn, conn.makefile("rb") as stream:
                        requests.append(json.loads(stream.readline()))
                        result = {"ok": True, "authorized": connected, "connected": connected} if index == 0 else response
                        if result is not None:
                            conn.sendall(json.dumps(result).encode() + b"\n")
            except Exception as error:
                errors.append(error)

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        env = {**os.environ, "PATH": str(root / "bin") + ":" + os.environ["PATH"],
               "FIXTURE": str(root), "NOTJIK_HOME": str(APP), "NOTJIK_TELEGRAM_DIR": str(root / "telegram"),
               "NOTJIK_STATE_DIR": str(root / "state"), "XDG_RUNTIME_DIR": str(root),
               "NOTJIK_TELEGRAM_SOCKET": str(root / "control.sock")}
        result = subprocess.run([str(BINARY), "telegram", "save-clipboard"], env=env,
                                capture_output=True, timeout=15)
        thread.join(timeout=11)
        self.assertFalse(thread.is_alive())
        self.assertEqual(errors, [])
        self.assertNotIn(data, result.stdout + result.stderr)
        return root, result, requests

    def test_native_text_snapshot_and_queue_ack(self):
        root, result, requests = self.run_clipboard(response={"ok": True, "queued": True})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([r["action"] for r in requests], ["status", "saveClipboard"])
        directory = root / "telegram/clipboard-outbox" / requests[1]["token"]
        self.assertEqual(json.loads((directory / "items.json").read_text()), [{"text": "**synthetic**\n"}])
        self.assertFalse((root / "notice").exists(), "Queue acknowledgement is not delivery")

    def test_disconnected_does_not_read_clipboard(self):
        root, result, requests = self.run_clipboard(connected=False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(requests), 1)
        self.assertFalse((root / "reads").exists())
        self.assertEqual(json.loads((root / "notice").read_text())["notice"]["timeout"], 5000)

    def test_lost_ack_retains_snapshot(self):
        root, result, requests = self.run_clipboard()
        self.assertEqual(result.returncode, 1)
        self.assertTrue((root / "telegram/clipboard-outbox" / requests[1]["token"]).exists())

    def test_definite_rejection_removes_snapshot(self):
        root, result, _ = self.run_clipboard(response={"ok": False, "error": "synthetic rejection"})
        self.assertEqual(result.returncode, 1)
        self.assertEqual(list((root / "telegram/clipboard-outbox").iterdir()), [])

    def test_image_precedes_text_and_preserves_bytes(self):
        root, result, requests = self.run_clipboard(response={"ok": True}, types="image/png\ntext/plain", data=b"synthetic image")
        self.assertEqual(result.returncode, 0, result.stderr)
        directory = root / "telegram/clipboard-outbox" / requests[1]["token"]
        self.assertEqual((directory / "0/clipboard.png").read_bytes(), b"synthetic image")

    def test_unsupported_format_never_queues(self):
        _, result, requests = self.run_clipboard(types="application/octet-stream")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(requests), 1)


if __name__ == "__main__":
    unittest.main()
