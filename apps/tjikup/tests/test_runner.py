"""Exercise real scripts in isolated directories, without any fleet dependencies."""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import tjikup
from tjikup.config import load_config
from tjikup.runtime import UpdateError


class RunnerTests(unittest.TestCase):
    def test_script_owned_failure_only_reports_root_cause(self):
        cases = (
            ("FAILED", "✗", False),
            ("PULL", "✗", False),
            ("WARN", "!", True),
            (None, None, True),
        )
        for verb, icon, needs_wrapper in cases:
            with self.subTest(verb=verb):
                command = [sys.executable, "-c", (
                    "from tjikup import runtime; "
                    + (f"runtime.record({verb!r}, {icon!r}, 'Repository', 'dirty checkout'); "
                       if verb else "")
                    + "raise SystemExit(1)"
                )]
                self.config.write_text(json.dumps(dict(version=1, steps=[
                    dict(id="wrapper", label="Wrapper", command=command,
                         output="stream", summary=False),
                    self.step("last"),
                ])))
                result = subprocess.run(
                    [sys.executable, "-B", "-m", "tjikup", "--config", str(self.config)],
                    env={**os.environ, "TJIKUP_NOTIFY": "0"}, capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 1)
                self.assertEqual("Wrapper" in result.stdout, needs_wrapper)
                self.assertNotIn("✗ Tjikup", result.stdout)
                self.assertNotIn("tjikup: Wrapper", result.stderr)
                self.assertNotIn("ALL SYSTEMS GO", result.stdout)
                self.assertEqual(self.events(), [])

    def test_previous_failure_does_not_hide_new_silent_failure(self):
        report = self.root / "report"
        report.write_text("PULL\t✗\tOld repo\told failure\n")
        self.config.write_text(json.dumps(dict(version=1, steps=[
            self.step("fail", output="stream", summary=False),
        ])))
        with patch.object(tjikup, "ACTIVE_REPORT_FILE", report), \
                contextlib.redirect_stdout(io.StringIO()) as output, \
                self.assertRaises(tjikup.ReportedError):
            tjikup.run_workflow(load_config(self.config))
        self.assertIn("FAILED", output.getvalue())
        self.assertIn("command failed (5)", output.getvalue())

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = self.root / "workflow.json"
        self.script = self.root / "step.py"
        self.script.write_text("""import os, pathlib, sys, time
root = pathlib.Path('.')
name = sys.argv[1]
if name == 'wait':
    (root / 'waiting').touch()
    deadline = time.monotonic() + 2
    while not (root / 'release').exists():
        if time.monotonic() > deadline: raise SystemExit(9)
        time.sleep(.01)
elif name == 'release':
    (root / 'release').touch()
elif name == 'fail':
    print('actionable failure detail'); raise SystemExit(5)
elif name == 'skip': raise SystemExit(77)
elif name == 'stop': raise SystemExit(78)
elif name == 'timeout': time.sleep(30)
elif name == 'env':
    assert os.environ['CUSTOM_FLAG'] == '1'
    from tjikup.runtime import status_msg
    status_msg('WARN', '!', 'Custom warning', 'detail')
with (root / 'events').open('a') as output: output.write(name + '\\n')
""")

    def step(self, name, **kwargs):
        return {
            "id": name,
            "command": [sys.executable, str(self.script), name],
            **kwargs,
        }

    def run_config(self, steps, *, dry_run=False):
        self.config.write_text(json.dumps(dict(version=1, steps=steps)))
        config = load_config(self.config)
        with (
            patch.object(tjikup, "ACTIVE_REPORT_FILE", self.root / "report"),
            contextlib.redirect_stdout(io.StringIO()) as output,
            contextlib.redirect_stderr(io.StringIO()) as errors,
        ):
            result = tjikup.run_workflow(config, dry_run=dry_run)
        return result, output.getvalue(), errors.getvalue()

    def events(self):
        path = self.root / "events"
        return path.read_text().splitlines() if path.exists() else []

    def test_order_and_parallel_barrier(self):
        self.run_config(
            [
                self.step("first"),
                {"parallel": [self.step("wait"), self.step("release")]},
                self.step("last"),
            ]
        )
        self.assertEqual(self.events()[0], "first")
        self.assertEqual(self.events()[-1], "last")
        self.assertEqual(set(self.events()[1:3]), {"wait", "release"})

    def test_preview_never_falls_back_to_mutating_command(self):
        _, output, _ = self.run_config(
            [
                self.step("mutate"),
                self.step(
                    "capture", preview=[sys.executable, str(self.script), "preview"]
                ),
            ],
            dry_run=True,
        )
        self.assertEqual(self.events(), ["preview"])
        self.assertIn("no preview configured", output)

    def test_failure_preserves_diagnostics_and_blocks_later_steps(self):
        self.config.write_text(
            json.dumps(dict(version=1, steps=[self.step("fail"), self.step("last")]))
        )
        with (
            contextlib.redirect_stdout(io.StringIO()),
            contextlib.redirect_stderr(io.StringIO()) as errors,
            self.assertRaisesRegex(UpdateError, "failed"),
        ):
            tjikup.run_workflow(load_config(self.config))
        self.assertIn("actionable failure detail", errors.getvalue())
        self.assertEqual(self.events(), [])

    def test_parallel_failure_waits_for_peers_and_blocks_followup(self):
        with self.assertRaises(UpdateError):
            self.run_config(
                [
                    {"parallel": [self.step("fail"), self.step("peer")]},
                    self.step("last"),
                ]
            )
        self.assertEqual(self.events(), ["peer"])

    def test_skip_continues_and_stop_suppresses_followup(self):
        _, output, _ = self.run_config(
            [
                self.step("skip", skip_codes=[77]),
                self.step("middle"),
                self.step("stop", stop_codes=[78]),
                self.step("last"),
            ]
        )
        self.assertEqual(self.events(), ["middle"])
        self.assertIn("SKIP", output)
        self.assertIn("workflow stopped", output)

    def test_timeout_fails_and_stops_followup(self):
        with self.assertRaises(UpdateError):
            self.run_config([self.step("timeout", timeout=0.05), self.step("last")])
        self.assertEqual(self.events(), [])

    def test_full_config_is_validated_before_any_command(self):
        self.config.write_text(
            json.dumps(
                dict(
                    version=1, steps=[self.step("first"), self.step("bad", timeout=-1)]
                )
            )
        )
        with self.assertRaises(UpdateError):
            load_config(self.config)
        self.assertEqual(self.events(), [])

    def test_invalid_schema_is_rejected(self):
        invalid = [
            self.step("bad", command="echo wrong"),
            self.step("bad", typo=True),
            self.step("bad", skip_codes=[True]),
            self.step("bad", stop_codes=[77], skip_codes=[77]),
            {"parallel": [self.step("bad", output="stream")]},
            self.step("bad", timeout=float("nan")),
        ]
        for step in invalid:
            with self.subTest(step=step):
                self.config.write_text(json.dumps(dict(version=1, steps=[step])))
                with self.assertRaises(UpdateError):
                    load_config(self.config)

    def test_paths_env_and_cli_flags(self):
        self.config.write_text(
            json.dumps(
                dict(
                    version=1,
                    env={"TARGET": "{config_dir}"},
                    flags={"custom": {"env": "CUSTOM_FLAG", "help": "a custom flag"}},
                    steps=[self.step("env", cwd="$TARGET", output="stream")],
                )
            )
        )
        result = subprocess.run(
            [
                sys.executable,
                "-B",
                "-m",
                "tjikup",
                "--config",
                str(self.config),
                "--custom",
            ],
            env={**os.environ, "TJIKUP_NOTIFY": "0"},
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("NEEDS ATTENTION", result.stdout)
        self.assertIn("Custom warning", result.stdout)
        self.assertEqual(self.events(), ["env"])

    def test_cli_collects_runner_rows_and_restores_report_environment(self):
        from tjikup import notifications

        self.config.write_text(json.dumps(dict(version=1, steps=[self.step("first")])))
        messages = []

        def notify(report, result, seconds, **kwargs):
            messages.append(report.read_text())

        with (
            patch.object(sys, "argv", ["tjikup", "--config", str(self.config)]),
            patch.dict(os.environ, {"TJIKUP_REPORT_FILE": str(self.root / "outer")}),
            patch.object(notifications, "notify_completion", side_effect=notify),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(tjikup.cli(), 0)
            self.assertEqual(os.environ["TJIKUP_REPORT_FILE"], str(self.root / "outer"))
        self.assertIn("RUN\t✓\tfirst", messages[0])
        self.assertFalse(tjikup.ACTIVE_REPORT_FILE.exists())

    def test_group_headings_and_script_owned_summaries(self):
        _, output, _ = self.run_config(
            [
                self.step(
                    "first",
                    section="Prepare",
                    summary=False,
                    banner={"title": "Project", "url": "https://example.org"},
                ),
                {
                    "section": "Parallel jobs",
                    "parallel": [self.step("wait"), self.step("release")],
                },
                self.step("last", section="Finish"),
            ]
        )
        self.assertIn("Project\nhttps://example.org\n", output)
        positions = [
            output.index(title + "\n")
            for title in ("Prepare", "Parallel jobs", "Finish")
        ]
        self.assertEqual(positions, sorted(positions))
        self.assertNotIn("first", output)
        self.assertIn("last", output)
        self.assertEqual(self.events()[0], "first")

    def test_disabling_summary_does_not_hide_failure(self):
        self.config.write_text(
            json.dumps(dict(version=1, steps=[self.step("fail", summary=False)]))
        )
        with (
            contextlib.redirect_stdout(io.StringIO()) as output,
            contextlib.redirect_stderr(io.StringIO()),
            self.assertRaises(UpdateError),
        ):
            tjikup.run_workflow(load_config(self.config))
        self.assertIn("FAILED", output.getvalue())

    def test_presentation_schema_is_validated(self):
        for item in (
            self.step("bad", summary="false"),
            self.step("bad", banner={"title": "Missing URL"}),
            self.step("bad", section=[]),
            {"parallel": [self.step("bad", section="Member heading")]},
        ):
            with self.subTest(item=item):
                self.config.write_text(json.dumps(dict(version=1, steps=[item])))
                with self.assertRaises(UpdateError):
                    load_config(self.config)

    def test_check_does_not_run_steps(self):
        self.config.write_text(json.dumps(dict(version=1, steps=[self.step("mutate")])))
        result = subprocess.run(
            [
                sys.executable,
                "-B",
                "-m",
                "tjikup",
                "--config",
                str(self.config),
                "--check",
            ],
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.events(), [])


if __name__ == "__main__":
    unittest.main()
