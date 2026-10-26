import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("email_archive_run", ROOT / "backend/email_archive_run.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class PipelineTests(unittest.TestCase):
    def test_json_write_does_not_reuse_readable_or_symlinked_temporary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "connection.json"
            old_temp = root / "connection.json.tmp"
            old_temp.write_text("unchanged")
            old_temp.chmod(0o644)
            previous_umask = os.umask(0)
            try:
                runner.save_json(path, {"token": "synthetic"})
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertEqual(old_temp.read_text(), "unchanged")
                old_temp.unlink()
                victim = root / "victim"
                victim.write_text("unchanged")
                old_temp.symlink_to(victim)
                runner.save_json(path, {"token": "synthetic-refresh"})
                self.assertEqual(victim.read_text(), "unchanged")
                self.assertEqual(json.loads(path.read_text())["token"], "synthetic-refresh")
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            finally:
                os.umask(previous_umask)

    def test_failed_json_write_preserves_destination_and_cleans_temporary(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "connection.json"
            path.write_text("original")
            with self.assertRaises(TypeError):
                runner.save_json(path, {"invalid": object()})
            self.assertEqual(path.read_text(), "original")
            self.assertEqual(list(Path(directory).iterdir()), [path])

    def test_failed_index_never_starts_export_or_records_success(self):
        with tempfile.TemporaryDirectory(prefix=".email-pipeline-test-", dir=ROOT) as temp:
            work = Path(temp)
            config = work / "config.json"
            config.write_text(json.dumps({"source_paths": []}))
            failure = subprocess.CalledProcessError(1, ["index"])
            with patch.object(runner.subprocess, "run", side_effect=failure) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    runner.run(work, config)
            self.assertEqual(run.call_count, 1)
            self.assertIn("email_archive_index.py", run.call_args.args[0][1])
            self.assertFalse((work / "index.complete.json").exists())
            self.assertFalse((work / "upload.complete.json").exists())

    def test_export_failure_keeps_completed_index_for_retry(self):
        with tempfile.TemporaryDirectory(prefix=".email-pipeline-test-", dir=ROOT) as temp:
            work = Path(temp)
            config = work / "config.json"
            config.write_text(json.dumps({"source_paths": []}))
            counts = {"messages": 2, "memberships": 3, "attachments": 1}
            calls = [subprocess.CompletedProcess([], 0), subprocess.CalledProcessError(1, ["export"])]
            with patch.object(runner.subprocess, "run", side_effect=calls) as run, \
                    patch.object(runner, "validate_index", return_value=counts), \
                    patch.object(runner, "require_drive_mount"):
                with self.assertRaises(subprocess.CalledProcessError):
                    runner.run(work, config)
            self.assertEqual(run.call_count, 2)
            self.assertTrue((work / "index.complete.json").exists())
            self.assertFalse((work / "upload.complete.json").exists())

    def test_wrong_mount_is_rejected(self):
        mounts = {"filesystems": [{"source": "/dev/disk", "fstype": "btrfs", "options": "rw"}]}
        result = subprocess.CompletedProcess([], 0, stdout=json.dumps(mounts))
        with patch.object(runner.subprocess, "run", return_value=result):
            with self.assertRaises(RuntimeError):
                runner.require_drive_mount(Path("/example"))


if __name__ == "__main__":
    unittest.main()
