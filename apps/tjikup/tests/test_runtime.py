import contextlib
import io
import os
from pathlib import Path
import re
import sys
import tempfile
import unittest
from unittest.mock import patch
from tjikup import runtime


class RuntimeTests(unittest.TestCase):
    def test_completion_banner_colors_and_centering(self):
        warning = "WARN\t!\tExample warning\tdetail\n"
        failure = "FAILED\t✗\tExample failure\tdetail\n"
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report"
            for width in (24, 80, 103, 120):
                for color in ("0", "1"):
                    for content in ("", warning, failure):
                        with self.subTest(width=width, color=color, content=content):
                            report.write_text(content * 2)
                            with (
                                patch.dict(os.environ, {"TJIKUP_COLUMNS": str(width),
                                                        "TJIKUP_COLOR": color}),
                                contextlib.redirect_stdout(io.StringIO()) as output,
                            ):
                                runtime.report_summary(report)
                            lines = output.getvalue().splitlines()
                            banner = lines[1]
                            self.assertEqual(lines[2], "")
                            plain = re.sub(r"\x1b\[[0-9;]*m", "", banner)
                            message = "NEEDS ATTENTION" if content else "ALL SYSTEMS GO"
                            self.assertEqual(plain, " " * ((width - len(message)) // 2) + message)
                            if color == "0":
                                self.assertNotIn("\x1b", output.getvalue())
                            elif content:
                                self.assertIn("\x1b[31;1mNEEDS ATTENTION\x1b[0m", banner)
                            else:
                                colors = set(re.findall(r"\x1b\[1;38;2;([0-9;]+)m", banner))
                                self.assertEqual(len(colors), 6)
                                self.assertTrue(banner.endswith("\x1b[0m"))
                            # Rendering must not duplicate issues or modify the report.
                            self.assertEqual(report.read_text(), content * 2)
                            if content:
                                self.assertEqual(output.getvalue().count("detail"), 1)

    def test_unicode_fragments_and_exit_failure(self):
        output = io.StringIO()
        with (
            contextlib.redirect_stdout(output),
            self.assertRaisesRegex(runtime.UpdateError, "failed"),
        ):
            runtime.run_stream(
                [
                    sys.executable,
                    "-c",
                    "import os,time; os.write(1, bytes([226])); time.sleep(.01); os.write(1, bytes([156,147])); raise SystemExit(3)",
                ],
                Path.cwd(),
            )
        self.assertEqual(output.getvalue(), "✓")

    def test_stderr_relay_is_incremental_and_drains_after_stdout_closes(self):
        with tempfile.TemporaryDirectory() as directory:
            ack = Path(directory) / 'ack'
            output, errors = io.StringIO(), io.StringIO()

            def receive(value):
                errors.write(value)
                if errors.getvalue() == 'continue? ':
                    ack.touch()

            child = (
                "import os,pathlib,sys,time; "
                "sys.stderr.write('continue? '); sys.stderr.flush(); "
                "ack=pathlib.Path(sys.argv[1]); deadline=time.monotonic()+1\n"
                "while not ack.exists() and time.monotonic()<deadline: time.sleep(.01)\n"
                "assert ack.exists(), 'prompt was buffered'; "
                "os.close(1); os.write(2, bytes([226])); time.sleep(.02); "
                "os.write(2, bytes([156,147]))"
            )
            with contextlib.redirect_stdout(output):
                runtime.run_stream([sys.executable, '-c', child, str(ack)], Path.cwd(),
                                   stderr_handler=receive, timeout=3)
            self.assertEqual(output.getvalue(), '')
            self.assertEqual(errors.getvalue(), 'continue? ✓')

    def test_stream_timeout(self):
        with (
            contextlib.redirect_stdout(io.StringIO()),
            self.assertRaisesRegex(runtime.UpdateError, "timed out"),
        ):
            runtime.run_stream(
                [sys.executable, "-c", "import time; time.sleep(10)"],
                Path.cwd(),
                timeout=0.05,
            )

    def test_frame_end_is_forwarded_before_child_writes_another_line(self):
        with tempfile.TemporaryDirectory() as directory:
            acknowledged = Path(directory) / 'ack'

            class Terminal(io.StringIO):
                def write(self, value):
                    result = super().write(value)
                    if self.getvalue().endswith('\x1b[?2026l'):
                        acknowledged.touch()
                    return result

            output = Terminal()
            child = (
                "import pathlib,sys,time; "
                "sys.stdout.write('\\x1b[?2026hrow\\n\\x1b[?2026l'); sys.stdout.flush(); "
                "ack=pathlib.Path(sys.argv[1]); deadline=time.monotonic()+1\n"
                "while not ack.exists() and time.monotonic()<deadline: time.sleep(.01)\n"
                "sys.exit(0 if ack.exists() else 3)"
            )
            with contextlib.redirect_stdout(output):
                runtime.run_stream([sys.executable, '-c', child, str(acknowledged)], Path.cwd(), timeout=3)
            self.assertEqual(output.getvalue(), '\x1b[?2026hrow\n\x1b[?2026l')

    def test_killed_live_child_restores_terminal(self):
        output = io.StringIO()
        with patch.dict(os.environ, {'TJIKUP_CHILD': '1', 'TJIKUP_PROGRESS': '1'}), \
                contextlib.redirect_stdout(output), \
                self.assertRaisesRegex(runtime.UpdateError, 'timed out'):
            runtime.run_stream([sys.executable, '-c',
                               "import sys,time; sys.stdout.write('\\x1b[?25l\\x1b[?2026h'); sys.stdout.flush(); time.sleep(10)"],
                               Path.cwd(), timeout=.2)
        self.assertTrue(output.getvalue().endswith('\x1b[?25h\x1b[?2026l'))

    def test_full_report_keeps_text_that_display_truncates(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report"
            with (
                patch.dict(
                    os.environ,
                    {
                        "TJIKUP_REPORT_FILE": str(report),
                        "TJIKUP_COLUMNS": "24",
                        "TJIKUP_COLOR": "0",
                    },
                ),
                contextlib.redirect_stdout(io.StringIO()) as output,
            ):
                runtime.status_msg("SYNC", "✓", "中文配置" * 20, "long note" * 20)
            self.assertEqual(runtime.display_width(output.getvalue().rstrip()), 23)
            self.assertIn("中文配置" * 20, report.read_text())

    def test_nested_stream_preserves_forwarded_color_and_width(self):
        output = io.StringIO()
        with (
            patch.dict(
                os.environ,
                {"TJIKUP_CHILD": "1", "TJIKUP_COLOR": "1", "TJIKUP_COLUMNS": "103"},
            ),
            contextlib.redirect_stdout(output),
        ):
            runtime.run_stream(
                [
                    sys.executable,
                    "-c",
                    "import os; print(os.environ['TJIKUP_COLOR'],os.environ['TJIKUP_COLUMNS'])",
                ],
                Path.cwd(),
            )
        self.assertEqual(output.getvalue(), "1 103\n")


class OutcomeStyleTests(unittest.TestCase):
    def test_no_change_display_and_report_agree_before_truncation(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report"
            with patch.dict(os.environ, {"TJIKUP_COLOR": "0", "TJIKUP_COLUMNS": "24",
                                         "TJIKUP_REPORT_FILE": str(report)}), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                runtime.status_msg("INSTALL", "✓", "Example", "no changes")
            self.assertTrue(output.getvalue().startswith("SKIP    ✓ "))
            self.assertEqual(report.read_text(), "SKIP\t✓\tExample\tno changes\n")

    def test_warning_precedes_skip_and_truncated_subtype_stays_dim(self):
        with patch.dict(os.environ, {"TJIKUP_COLOR": "1", "TJIKUP_COLUMNS": "24"}):
            row = runtime.format_status("SKIP", "!", "Assets (long subtype)")
        self.assertIn("\x1b[33;1mSKIP", row)
        self.assertIn("\x1b[33m!", row)
        self.assertIn("\x1b[97mAssets\x1b[0m \x1b[90m(", row)
        self.assertIn("…", row)
