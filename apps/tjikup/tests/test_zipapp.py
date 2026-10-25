"""Acceptance checks against the distributed executable, with no source imports."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ZipappTests(unittest.TestCase):
    def test_standalone_exit_codes_and_child_runtime_import(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / "tjikup"
            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/build.py"),
                    "--output",
                    str(executable),
                ],
                check=True,
            )
            env = {**os.environ, "HOME": str(root), "TJIKUP_NOTIFY": "0"}
            env.pop("PYTHONPATH", None)
            env.pop("TJIKUP_CONFIG", None)
            config = root / "config.json"

            def run(*args):
                return subprocess.run(
                    [str(executable), *args],
                    env=env,
                    capture_output=True,
                    text=True,
                    timeout=5,
                )

            self.assertEqual(run("--help").returncode, 0)
            self.assertEqual(run("--config", str(config), "--check").returncode, 1)
            config.write_text(
                json.dumps(
                    {
                        "version": 1,
                        "steps": [
                            {
                                "id": "example",
                                "output": "stream",
                                "command": [
                                    sys.executable,
                                    "-c",
                                    "from tjikup.runtime import status_msg; status_msg('CHECK','✓','Standalone child')",
                                ],
                            }
                        ],
                    }
                )
            )
            result = run("--config", str(config))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Standalone child", result.stdout)
            config.write_text(
                json.dumps(
                    {
                        "version": 1,
                        "steps": [
                            {
                                "id": "failure",
                                "command": [
                                    sys.executable,
                                    "-c",
                                    "raise SystemExit(8)",
                                ],
                            }
                        ],
                    }
                )
            )
            result = run("--config", str(config))
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("command failed (8)", result.stderr)
