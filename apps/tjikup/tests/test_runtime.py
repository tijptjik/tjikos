import contextlib
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from tjikup import runtime


class RuntimeTests(unittest.TestCase):
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
