import importlib.machinery
import importlib.util
from pathlib import Path
import tempfile
import json
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / 'runtime/private_assets.py'
loader = importlib.machinery.SourceFileLoader('private_storage', str(SOURCE))
spec = importlib.util.spec_from_loader(loader.name, loader)
storage = importlib.util.module_from_spec(spec)
loader.exec_module(storage)


class PrivateStorageTests(unittest.TestCase):
    def test_hourly_run_never_initializes_missing_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'documents').mkdir()
            with patch.object(storage, 'HOME', root), patch.object(storage, 'STATE', root/'state'), \
                 patch.object(storage, 'settings', return_value={'sharing_approved': True, 'folders':['documents'], 'remote':'drive:private'}), \
                 patch.object(storage, 'run') as run:
                with self.assertRaisesRegex(RuntimeError, 'not been initialized'):
                    storage.sync()
                run.assert_not_called()

    def test_missing_or_symlinked_source_cannot_propagate_deletion(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'target').mkdir()
            (root/'documents').symlink_to(root/'target', target_is_directory=True)
            with patch.object(storage, 'HOME', root), patch.object(storage, 'STATE', root/'state'), \
                 patch.object(storage, 'settings', return_value={'sharing_approved': True, 'folders':['documents'], 'remote':'drive:private'}), \
                 patch.object(storage, 'run') as run:
                with self.assertRaisesRegex(RuntimeError, 'real local directory'):
                    storage.sync()
                run.assert_not_called()

    def test_overlapping_operations_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(storage, 'STATE', Path(directory)):
            with storage.locked():
                with self.assertRaisesRegex(RuntimeError, 'Another private'):
                    with storage.locked():
                        self.fail('Second operation acquired lock')

    def test_normal_sync_preserves_conflicts_and_never_resyncs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'documents').mkdir()
            work = root/'state/bisync/documents'
            work.mkdir(parents=True)
            (work/'initialized.json').write_text(json.dumps({'host': storage.HOST, 'remote': 'drive:private/documents'}))
            calls = []
            def run(args, **kwargs):
                calls.append(args)
                Path(args[args.index('--log-file') + 1]).write_text('Sync completed')
            with patch.object(storage, 'HOME', root), patch.object(storage, 'STATE', root/'state'), \
                 patch.object(storage, 'settings', return_value={'sharing_approved': True, 'folders':['documents'], 'remote':'drive:private'}), \
                 patch.object(storage, 'run', side_effect=run):
                storage.sync()
            self.assertEqual(len(calls), 1)
            args = calls[0]
            self.assertNotIn('--resync', args)
            self.assertNotIn('--force', args)
            self.assertIn('--check-access', args)
            self.assertIn('--filters-file', args)
            self.assertIn('--backup-dir1', args)
            self.assertIn('--backup-dir2', args)
            self.assertEqual(args[args.index('--conflict-resolve')+1], 'none')
            self.assertEqual(args[args.index('--max-delete')+1], '5')

    def test_initialization_requires_verified_layout(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(storage, 'CONFIG', Path(directory)), \
             patch.object(storage, 'settings', return_value={'sharing_approved': True, 'folders':['documents']}), \
             patch.object(storage, 'run') as run:
            with self.assertRaisesRegex(RuntimeError, 'reorganization'):
                storage.sync(initial=True)
            run.assert_not_called()

    def test_laptop_initial_merge_prefers_established_cloud_and_retains_local_recovery(self):
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'documents').mkdir()
            (root/'layout-ready').write_text('verified')
            calls = []
            def run(args, **kwargs):
                calls.append(args)
                if '--log-file' in args:
                    Path(args[args.index('--log-file')+1]).write_text('Sync completed')
                return SimpleNamespace(stdout='{"bytes": 0}')
            with patch.object(storage, 'HOME', root), patch.object(storage, 'STATE', root/'state'), \
                 patch.object(storage, 'CONFIG', root), patch.object(storage, 'HOST', 'li'), \
                 patch.object(storage, 'settings', return_value={'sharing_approved': True, 'folders':['documents'], 'remote':'drive:private'}), \
                 patch.object(storage, 'run', side_effect=run):
                storage.sync(initial=True)
            transfers = [args for args in calls if 'bisync' in args]
            self.assertEqual(len(transfers), 2)
            self.assertIn('--dry-run', transfers[0])
            for args in transfers:
                self.assertEqual(args[args.index('--resync-mode')+1], 'path2')
                self.assertIn('--backup-dir1', args)
            receipt = json.loads((root/'state/bisync/documents/initialized.json').read_text())
            self.assertEqual(receipt['host'], 'li')

    def test_unapproved_sharing_blocks_initial_and_normal_sync_before_commands(self):
        for initial in (False, True):
            for approval in (None, False, "true"):
                with self.subTest(initial=initial, approval=approval), \
                     patch.object(storage, 'settings', return_value={'sharing_approved': approval}), \
                     patch.object(storage, 'run') as run:
                    with self.assertRaisesRegex(RuntimeError, 'sharing has not been approved'):
                        storage.sync(initial=initial)
                    run.assert_not_called()


class BackupCoordinationTests(unittest.TestCase):
    def start(self, root, host='fi', scheduled=False, cutoff=0):
        import subprocess
        import sys
        process = subprocess.Popen(
            [sys.executable, '-u', '-c', storage.BACKUP_COORDINATOR,
             str(root), host, str(cutoff), '1' if scheduled else '0'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(self.close, process)
        return process

    @staticmethod
    def close(process):
        if not process.stdin.closed:
            process.stdin.close()
        try:
            process.wait(timeout=3)
        except Exception:
            process.kill()
            process.wait()
        process.stdout.close()
        process.stderr.close()

    def test_laptop_skips_running_desktop_then_runs_after_failed_desktop(self):
        with tempfile.TemporaryDirectory() as root:
            fi = self.start(root)
            self.assertEqual(storage.coordinator_reply(fi), 'READY')
            li = self.start(root, 'li', True)
            self.assertIn('another client', storage.coordinator_reply(li))
            self.close(fi)
            self.assertFalse((Path(root)/'.coordination/fi.json').exists())
            retry = self.start(root, 'li', True)
            self.assertEqual(storage.coordinator_reply(retry), 'READY')
            self.close(retry)

    def test_successful_desktop_skips_scheduled_laptop_but_not_manual(self):
        with tempfile.TemporaryDirectory() as root:
            fi = self.start(root)
            self.assertEqual(storage.coordinator_reply(fi), 'READY')
            fi.stdin.write('complete\n')
            fi.stdin.flush()
            self.assertEqual(storage.coordinator_reply(fi), 'SAVED')
            fi.wait(timeout=3)
            li = self.start(root, 'li', True)
            self.assertIn('fi already completed', storage.coordinator_reply(li))
            li.wait(timeout=3)
            manual = self.start(root, 'li', False)
            self.assertEqual(storage.coordinator_reply(manual), 'READY')
            self.close(manual)

    def test_yesterdays_completion_does_not_skip_laptop(self):
        import time
        with tempfile.TemporaryDirectory() as root:
            state = Path(root)/'.coordination'
            state.mkdir()
            (state/'fi.json').write_text(json.dumps({'host':'fi', 'completed_epoch':time.time()-86400}))
            li = self.start(root, 'li', True, time.time()-3600)
            self.assertEqual(storage.coordinator_reply(li), 'READY')
            self.close(li)

    def test_invalid_receipt_fails_closed(self):
        with tempfile.TemporaryDirectory() as root:
            state = Path(root)/'.coordination'
            state.mkdir()
            (state/'fi.json').write_text('not valid json')
            li = self.start(root, 'li', True)
            with self.assertRaisesRegex(RuntimeError, 'Lost backup coordination'):
                storage.coordinator_reply(li)

    def test_lost_lock_stops_running_backup_process(self):
        import subprocess
        import sys
        from types import SimpleNamespace
        processes = []
        real_popen = subprocess.Popen
        def launch(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            processes.append(process)
            return process
        with patch.object(storage.subprocess, 'Popen', side_effect=launch):
            with self.assertRaisesRegex(RuntimeError, 'Lost backup coordination lock'):
                storage.coordinated_run([sys.executable, '-c', 'import time; time.sleep(60)'],
                                        SimpleNamespace(poll=lambda: 1))
        self.assertIsNotNone(processes[0].poll())
