"""Exercise the OSC metadata received by Kitty's desktop notification hook."""

import base64
import contextlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from tjikup import notifications


class SSHCardTests(unittest.TestCase):
    def command(self, report_text="", result=0):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report"
            report.write_text(report_text)
            stream = io.StringIO()
            with (patch.dict(os.environ, {"SSH_TTY": "/dev/pts/1", "TERM": "xterm-kitty"}, clear=True),
                  patch.object(notifications.socket, "gethostname", return_value="si"),
                  contextlib.redirect_stdout(stream), patch.object(stream, "isatty", return_value=True)):
                notifications.notify_completion(report, result, 79)
        metadata, title = re.search(r"\x1b\]99;([^;]*);([^\x1b]*)", stream.getvalue()).groups()
        fields = dict(field.split("=", 1) for field in metadata.split(":"))
        decode = lambda value: base64.b64decode(value).decode()
        return SimpleNamespace(application_name=decode(fields["f"]), title=decode(title),
                               notification_types=(decode(fields["t"]),))

    def test_remote_cards_use_native_type_and_local_actions(self):
        for report, result, expected, body, detail, timeout in (
            ("", 0, "ALL SYSTEMS GO", "", "1m 19s · 0 stages", "10s"),
            ("WARN\t!\tDirty Chezmoi\t106 files\n", 0, "Dirty Chezmoi",
             "1m 19s · 0 stages", "106 files", "until-dismissed"),
            ("UPDATE\t✗\tBun\tprivate output\n", 1, "1 failed",
             "Failed · UPDATE: Bun", "1m 19s · 0 stages", "until-dismissed"),
        ):
            with self.subTest(expected=expected):
                command = self.command(report, result)
                with (patch.object(notifications.shutil, "which", return_value="/bin/notjikctl"),
                      patch.object(notifications, "capture_focus_command", return_value=["local-focus"]),
                      patch.object(notifications.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as send):
                    self.assertTrue(notifications.ssh_notification_filter(command))
                args = send.call_args.args[0]
                self.assertIn("Tjikup · si", args[2])
                self.assertEqual(args[3], body)
                self.assertEqual(args[args.index("--type") + 1], "status")
                data = json.loads(args[args.index("--data") + 1])
                self.assertEqual(data["state"], expected)
                self.assertEqual(data["detail"], detail)
                self.assertEqual(args[args.index("--timeout") + 1], timeout)
                self.assertEqual(json.loads(args[args.index("--actions") + 1]),
                                 [{"label": "Show window", "command": ["local-focus"]}])
                self.assertNotIn("private output", str(args))

    def test_native_failure_retains_kitty_fallback(self):
        command = self.command()
        with patch.object(notifications, "capture_focus_command", return_value=None):
            with patch.object(notifications.shutil, "which", return_value=None):
                self.assertFalse(notifications.ssh_notification_filter(command))
            for failure in (subprocess.CompletedProcess([], 1), OSError(),
                            subprocess.TimeoutExpired("notjikctl", 3)):
                with (patch.object(notifications.shutil, "which", return_value="notjikctl"),
                      patch.object(notifications.subprocess, "run", **(
                          {"side_effect": failure} if isinstance(failure, Exception)
                          else {"return_value": failure}))):
                    self.assertFalse(notifications.ssh_notification_filter(command))

    def test_unrelated_or_invalid_metadata_does_not_execute(self):
        for app, metadata in (("Other", "tjikup-status-v1:{}"), ("Tjikup", "other"),
                              ("Tjikup", "tjikup-status-v1:{"),
                              ("Tjikup", 'tjikup-status-v1:{"data":{},"body":"","command":["evil"]}'),
                              ("Tjikup", "tjikup-status-v1:" + "x" * 16385)):
            with patch.object(notifications.subprocess, "run") as run:
                self.assertFalse(notifications.ssh_notification_filter(SimpleNamespace(
                    application_name=app, notification_types=(metadata,), title="test")))
                run.assert_not_called()
