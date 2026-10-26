"""Completion reporting without running updates or contacting the desktop bus."""

import contextlib
import io
import json
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
        self.assertTrue(body.startswith("ALL SYSTEMS GO\n"))

    def test_dirty_status_contains_friendly_name_and_safe_count(self):
        title, body, urgency = self.message(
            "WARN\t!\tDirty State ~/.local/share/chezmoi\t82 files\n"
        )
        self.assertTrue(body.startswith("Dirty Chezmoi — 82 files\n"))
        self.assertNotIn("~/.local/share", body)
        self.assertEqual(title, "Tjikup needs attention")
        self.assertEqual(urgency, "normal")
        self.assertNotIn("private detail", self.message(
            "WARN\t!\tDirty State /tmp/repo\tprivate detail\n")[1])

    def test_native_status_and_focus_action(self):
        for report, result, expected, timeout in (
            ("", 0, "ALL SYSTEMS GO", "10s"),
            ("WARN\t!\tDirty State /tmp/repo\t82 files\n", 0,
             "Dirty repo", "until-dismissed"),
            ("FAILED\t✗\tDotfiles\tprivate output\n", 1,
             "1 failed", "until-dismissed"),
        ):
            with (self.subTest(expected=expected), tempfile.TemporaryDirectory() as directory,
                  patch.dict(os.environ, {"WAYLAND_DISPLAY": "wayland-1"}, clear=True),
                  patch.object(notifications.shutil, "which", return_value="/bin/notjikctl"),
                  patch.object(notifications.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as send):
                path = Path(directory) / "report"
                path.write_text(report)
                focus = ["/bin/hyprctl", "dispatch", "focuswindow", "address:0x123"]
                notifications.notify_completion(path, result, 83, focus_command=focus)
                send.assert_called_once()
                args = send.call_args.args[0]
                self.assertEqual(json.loads(args[args.index("--data") + 1])["state"], expected)
                data = json.loads(args[args.index("--data") + 1])
                self.assertEqual(data.get("rainbow-text", False), expected == "ALL SYSTEMS GO")
                self.assertEqual(json.loads(args[args.index("--actions") + 1]),
                                 [{"label": "Show window", "command": focus}])
                self.assertEqual(args[args.index("--timeout") + 1], timeout)
                self.assertNotIn("private output", " ".join(args))
                self.assertNotIn(expected, args[3])
                self.assertEqual(json.loads(args[args.index("--data") + 1])["detail"],
                                 "82 files" if expected == "Dirty repo" else "1m 23s · 0 stages")
                if expected == "Dirty repo":
                    self.assertEqual(args[3], "1m 23s · 0 stages")

    def test_native_layout_keeps_root_cause_out_of_status_row(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report"
            path.write_text(
                "CHECK\t✓\tPrerequisites\tOK\n"
                "CHECK\t✗\tConnection failed\tprivate output\n"
                "FAILED\t✗\tDemo command\tcommand failed\n"
                "FAILED\t✗\tTjikup\tcommand failed\n")
            _, fallback, _, data, body = notifications.completion_payload(
                path, 1, 83, dry_run=True)
            self.assertEqual(data, {"state": "2 failed", "tone": "critical",
                                    "detail": "1m 23s · 1 stage"})
            self.assertEqual(body, "Failed · CHECK: Connection failed\nFailed · Demo command\nPreview · no changes applied")
            self.assertNotIn("private output", body)
            self.assertIn("Connection failed", fallback)

    def test_native_dirty_names_and_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report"
            for subject, note, expected in (
                ("Dirty State ~/.local/share/chezmoi", "82 files", "Dirty Chezmoi — 82 files"),
                ("Dirty Chezmoi", "48 files", "Dirty Chezmoi — 48 files"),
                ("Dirty Chezetc", "1 file", "Dirty Chezetc — 1 file"),
                ("Dirty Chezmoi", "private details", "Dirty Chezmoi"),
            ):
                with self.subTest(subject=subject, note=note):
                    path.write_text(f"WARN\t!\t{subject}\t{note}\n")
                    _, fallback, _, data, body = notifications.completion_payload(path, 0, 83)
                    self.assertEqual(data["state"], expected.split(" — ")[0])
                    self.assertEqual(data["detail"], note if note != "private details" else "")
                    self.assertEqual(body, "1m 23s · 0 stages")
                    self.assertIn(expected, fallback)

    def test_native_dirty_card_matches_requested_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report"
            path.write_text("WARN\t!\tDirty Chezmoi\t106 files\n" + "".join(
                f"APPLY\t✓\tStage {index}\tOK\n" for index in range(46)))
            title, _, _, data, body = notifications.completion_payload(path, 0, 72)
            self.assertEqual(title, "Tjikup needs attention")
            self.assertEqual(body, "1m 12s · 46 stages")
            self.assertEqual(data, {"state": "Dirty Chezmoi", "detail": "106 files",
                                    "tone": "warning"})
            _, _, _, _, preview = notifications.completion_payload(path, 0, 72, dry_run=True)
            self.assertEqual(preview, "1m 12s · 46 stages\nPreview · no changes applied")

    def test_failures_precede_warnings_and_keep_operation_names(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report"
            path.write_text(
                "WARN\t!\tDirty Chezmoi\t48 files\n"
                "CHECK\t!\tBitwarden\tprivate note\n"
                "UPDATE\t✗\tBun\tprivate output\n"
                "UPDATE\tX\tBun\tdifferent output\n"
                "SYNC\t✗\tZed\tprivate output\n"
                "FAILED\t✗\tTjikup\tprivate output\n")
            _, fallback, urgency, data, body = notifications.completion_payload(path, 1, 83)
            self.assertEqual(data["state"], "2 failed · 2 warnings")
            self.assertEqual(body, "Failed · UPDATE: Bun\nFailed · SYNC: Zed\nDirty Chezmoi — 48 files\n+1 more in terminal")
            self.assertEqual(urgency, "critical")
            self.assertIn("Issues: 4", fallback)
            self.assertNotIn("private", fallback + body)
            self.assertNotIn("Failed · Tjikup", fallback + body)

    def test_warning_does_not_hide_an_unexplained_runner_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report"
            path.write_text("WARN\t!\tDirty Chezmoi\t1 file\nFAILED\t✗\tTjikup\tprivate error\n")
            _, _, _, data, body = notifications.completion_payload(path, 1, 1)
            self.assertEqual(data["state"], "1 failed · 1 warning")
            self.assertEqual(body, "Failed · Tjikup\nDirty Chezmoi — 1 file")

    def test_native_failure_falls_back_without_changing_result(self):
        for failure in (subprocess.CompletedProcess([], 1), OSError("missing"),
                        subprocess.TimeoutExpired("notjikctl", 3)):
            with (self.subTest(failure=failure),
                  patch.dict(os.environ, {"DISPLAY": ":1"}, clear=True),
                  patch.object(notifications.shutil, "which", side_effect=lambda name: "/bin/" + name),
                  patch.object(notifications.subprocess, "run", side_effect=[failure, subprocess.CompletedProcess([], 0)]) as send):
                notifications.notify_completion(None, 1, 1)
                self.assertEqual(send.call_count, 2)
                self.assertEqual(send.call_args.args[0][0], "/bin/notify-send")

    def test_focus_uses_origin_process_not_current_active_window(self):
        clients = [{"pid": 42, "address": "0x123"}, {"pid": 99, "address": "0x456"}]
        with (patch.dict(os.environ, {"HYPRLAND_INSTANCE_SIGNATURE": "test"}, clear=True),
              patch.object(notifications.shutil, "which", return_value="/bin/hyprctl"),
              patch.object(notifications, "ancestor_pids", return_value=[60, 42]),
              patch.object(notifications.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(clients))) as probe):
            self.assertEqual(notifications.capture_focus_command(),
                             ["/bin/hyprctl", "dispatch", "focuswindow", "address:0x123"])
            self.assertEqual(probe.call_args_list[0].args[0], ["/bin/hyprctl", "-j", "clients"])
            clients.append({"pid": 42, "address": "0x789"})
            probe.return_value.stdout = json.dumps(clients)
            self.assertIsNone(notifications.capture_focus_command())
            probe.side_effect = subprocess.TimeoutExpired("hyprctl", 1)
            self.assertIsNone(notifications.capture_focus_command())

    def test_focus_uses_lua_dispatch_when_supported(self):
        with (patch.dict(os.environ, {"HYPRLAND_INSTANCE_SIGNATURE": "test"}, clear=True),
              patch.object(notifications.shutil, "which", return_value="/bin/hyprctl"),
              patch.object(notifications, "ancestor_pids", return_value=[60, 42]),
              patch.object(notifications.subprocess, "run", side_effect=[
                  subprocess.CompletedProcess([], 0, '[{"pid":42,"address":"0x123"}]'),
                  subprocess.CompletedProcess([], 0, 'ok\n'),
              ]) as probe):
            self.assertEqual(notifications.capture_focus_command(),
                             ["/bin/hyprctl", "dispatch", 'hl.dsp.focus({ window = "address:0x123" })'])
            self.assertEqual(probe.call_args.args[0], ["/bin/hyprctl", "eval", "assert(hl.dsp.focus)"])

    def test_no_focus_button_when_origin_is_unknown(self):
        with (patch.dict(os.environ, {"WAYLAND_DISPLAY": "wayland-1"}, clear=True),
              patch.object(notifications.shutil, "which", return_value="/bin/notjikctl"),
              patch.object(notifications.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as send):
            notifications.notify_completion(None, 1, 1)
            args = send.call_args.args[0]
            self.assertEqual(json.loads(args[args.index("--actions") + 1]), [])

    def test_focus_is_captured_before_workflow(self):
        order = []
        with (patch.object(notifications, "capture_focus_command", side_effect=lambda: order.append("capture") or ["focus"]),
              patch.object(tjikup, "main", side_effect=lambda: order.append("run") or 0),
              patch.object(notifications, "notify_completion") as send):
            self.assertEqual(tjikup.cli(), 0)
            self.assertEqual(order, ["capture", "run"])
            self.assertEqual(send.call_args.kwargs["focus_command"], ["focus"])

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
                notifications.shutil, "which", side_effect=lambda name: "/usr/bin/notify-send" if name == "notify-send" else None
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
