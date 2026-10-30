import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
import notjik


class IPCPrivacyTests(unittest.TestCase):
    def setUp(self):
        runtime = tempfile.TemporaryDirectory()
        self.addCleanup(runtime.cleanup)
        environment = patch.dict(os.environ, {"XDG_RUNTIME_DIR": runtime.name})
        environment.start()
        self.addCleanup(environment.stop)

    def test_private_payload_never_enters_argv_and_is_removed(self):
        request = {"op": "send", "notice": {"body": "synthetic private message 香港"}}
        paths = []

        def run(argv, **kwargs):
            path = Path(argv[-1])
            paths.append(path)
            self.assertEqual(argv[-2], "requestFile")
            self.assertNotIn("synthetic private", " ".join(argv))
            self.assertEqual(json.loads(path.read_text()), request)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)
            return subprocess.CompletedProcess(argv, 0, '{"ok":true,"id":"test"}', '')

        with patch.object(notjik.subprocess, "run", side_effect=run):
            self.assertEqual(notjik.ipc(ROOT, request)["id"], "test")
        self.assertFalse(paths[0].parent.exists())

    def test_timeout_removes_private_request(self):
        paths = []

        def run(argv, **kwargs):
            paths.append(Path(argv[-1]))
            raise subprocess.TimeoutExpired(argv, 5)

        with patch.object(notjik.subprocess, "run", side_effect=run):
            with self.assertRaises(subprocess.TimeoutExpired) as caught:
                notjik.ipc(ROOT, {"notice": {"body": "synthetic private message"}})
        self.assertNotIn("synthetic private", str(caught.exception))
        self.assertFalse(paths[0].parent.exists())

    def test_oversized_request_never_launches(self):
        with patch.object(notjik.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "128 KiB"):
                notjik.ipc(ROOT, {"body": "x" * 131073})
        run.assert_not_called()

    @unittest.skipUnless(shutil.which("quickshell"), "Quickshell required")
    def test_real_qml_file_transport(self):
        # Run only the production IPC handler in an isolated offscreen shell.
        # No notification service, real history, or Telegram account is loaded.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "qml").mkdir()
            source = (ROOT / "qml/shell.qml").read_text()
            handler = source[source.index("    FileView {\n        id: ipcRequestFile"):source.index("    Process {\n        id: backend")]
            (root / "qml/shell.qml").write_text(
                'import QtQuick\nimport Quickshell\nimport Quickshell.Io\nShellRoot {\n'
                'id: root\nfunction handle(value) { return {ok:true, value:value}; }\n' + handler + '\n}')
            env = {**os.environ, "QT_QPA_PLATFORM": "offscreen", "QSG_RHI_BACKEND": "software"}
            with tempfile.TemporaryFile() as log:
                process = subprocess.Popen(["quickshell", "--path", str(root / "qml")],
                                           env=env, stdout=log, stderr=log)
                try:
                    with patch.dict(os.environ, env):
                        for attempt in range(50):
                            try:
                                response = notjik.ipc(root, {"body": "synthetic one 香港"})
                                break
                            except ValueError:
                                if process.poll() is not None or attempt == 49:
                                    log.seek(0)
                                    self.fail(log.read().decode(errors="replace"))
                                time.sleep(0.1)
                        self.assertEqual(response["value"]["body"], "synthetic one 香港")
                        self.assertEqual(notjik.ipc(root, {"body": "synthetic two"})["value"]["body"], "synthetic two")
                        bad = root / "bad.json"
                        for content in ("invalid JSON", '"' + 'x' * 131073 + '"'):
                            bad.write_text(content)
                            result = subprocess.run(["quickshell", "ipc", "--path", str(root / "qml"),
                                                     "call", "notjik", "requestFile", str(bad)],
                                                    capture_output=True, text=True, timeout=5)
                            self.assertFalse(json.loads(result.stdout)["ok"])
                finally:
                    process.terminate()
                    process.wait(timeout=5)
