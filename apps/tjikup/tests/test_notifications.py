"""Completion reporting without running updates or contacting the desktop bus."""

import contextlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "src"))
import tjikup
from tjikup import notifications


class NotificationTests(unittest.TestCase):
    def message(self, text, result=0, **kwargs):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.tsv"
            report.write_text(text)
            return notifications.completion_message(report, result, 83, **kwargs)

    def test_success_deduplicates_stages(self):
        title, body, urgency = self.message("APPLY\t✓\tDotfiles\t1s\n" * 2)
        self.assertEqual(title, "Tjikup complete")
        self.assertIn("1m 23s · Stages completed: 1 · Issues: 0", body)
        self.assertEqual(urgency, "normal")

    def test_zero_exit_with_warnings_is_not_success(self):
        title, body, _ = self.message("WARN\t!\tA & <B>\tprivate output\n" * 2)
        self.assertEqual(title, "Tjikup needs attention")
        self.assertIn("Issues: 1", body)
        self.assertIn("A &amp; &lt;B&gt;", body)
        self.assertNotIn("private output", body)

    def test_failure_and_preview_labels(self):
        title, body, urgency = self.message("", result=1, dry_run=True)
        self.assertEqual(title, "Tjikup preview failed")
        self.assertIn("no changes applied", body)
        self.assertEqual(urgency, "critical")
        self.assertEqual(
            self.message("", result=130, interrupted=True)[0], "Tjikup interrupted"
        )

    def test_missing_report_is_not_all_clear(self):
        title, body, _ = notifications.completion_message(Path("/does-not-exist"), 0, 1)
        self.assertEqual(title, "Tjikup needs attention")
        self.assertIn("unavailable", body)

    def test_delivery_is_bounded_and_nonfatal(self):
        with (
            patch.dict(os.environ, {"DISPLAY": ":1"}, clear=True),
            patch.object(
                notifications.shutil, "which", return_value="/usr/bin/notify-send"
            ),
            patch.object(
                notifications.subprocess,
                "run",
                side_effect=subprocess.TimeoutExpired("notify-send", 3),
            ) as send,
        ):
            notifications.notify_completion(None, 1, 5)
        args = send.call_args.args[0]
        self.assertIn("--app-name=Tjikup", args)
        self.assertIn("--urgency=critical", args)
        self.assertTrue(any(arg.endswith("/tjikup.png") for arg in args))
        self.assertEqual(send.call_args.kwargs["timeout"], 3)

    def test_headless_ssh_and_opt_out_do_not_notify(self):
        for environment in (
            {},
            {"DISPLAY": ":1", "SSH_CONNECTION": "remote"},
            {"DISPLAY": ":1", "TJIKUP_NOTIFY": "0"},
        ):
            with (
                self.subTest(environment=environment),
                patch.dict(os.environ, environment, clear=True),
                patch.object(notifications.subprocess, "run") as send,
            ):
                notifications.notify_completion(None, 0, 1)
                send.assert_not_called()

    def test_cli_notifies_once_for_return_error_and_interrupt(self):
        for outcome, expected in (
            (0, 0),
            (1, 1),
            (RuntimeError("failed"), 1),
            (KeyboardInterrupt(), 130),
        ):
            kwargs = (
                {"side_effect": outcome}
                if isinstance(outcome, BaseException)
                else {"return_value": outcome}
            )
            with (
                self.subTest(outcome=outcome),
                patch.object(tjikup, "main", **kwargs),
                patch.object(notifications, "notify_completion") as send,
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(tjikup.cli(), expected)
                send.assert_called_once()
                self.assertEqual(send.call_args.args[1], expected)

    def test_help_does_not_notify(self):
        with (
            patch.object(tjikup, "main", side_effect=SystemExit(0)),
            patch.object(notifications, "notify_completion") as send,
            self.assertRaises(SystemExit),
        ):
            tjikup.cli()
        send.assert_not_called()


if __name__ == "__main__":
    unittest.main()
