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
