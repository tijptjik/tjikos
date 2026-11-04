"""Native scheduled backup decisions with synthetic data and a real flock coordinator."""
import datetime
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/synctjik'
FAKE = r'''#!/usr/bin/python3
import datetime,fcntl,json,os,subprocess,sys
from pathlib import Path
root=Path(os.environ['FIXTURE']);name=Path(sys.argv[0]).name;a=sys.argv[1:]
if name=='hostname':print('li');sys.exit(0)
if name=='ssh':
 if a[-1]=='hostname':print('fixture-server');sys.exit(0)
 os.execvp('sh',['sh','-c',a[-1]])
with (root/'calls').open('a') as f:f.write(json.dumps(a)+'\n')
with (root/'remote/.coordination/backup.lock').open('a') as f:
 try:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
 except BlockingIOError:pass
 else:raise AssertionError('remote coordination lock must remain held')
peer=a[a.index('-r')+1].endswith('/fi')
if 'snapshots' in a:
 stamp=datetime.datetime.now(datetime.timezone.utc).isoformat()
 snapshot={'id':('a' if peer else 'b')*64,'time':stamp,'hostname':'li','tags':['private-storage'],'paths':[str(root/'documents')]}
 if peer:snapshot=json.loads((root/'snapshot.json').read_text())
 print(json.dumps([snapshot]));sys.exit(0)
if 'dump' in a:
 if os.environ.get('FAIL_DUMP'):sys.exit(7)
 sys.stdout.buffer.write((root/'peer.tar').read_bytes());sys.stdout.buffer.flush()
 if os.environ.get('CHANGE_DURING_DUMP'):(root/'documents/changed-after-check').write_text('new laptop content')
 sys.exit(0)
if 'backup' in a and os.environ.get('FAIL_BACKUP'):sys.exit(8)
'''


class PeerCoverage(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='synctjik-peer-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.docs = self.root / 'documents'
        self.docs.mkdir()
        (self.docs / 'file').write_text('original')
        (self.docs / 'empty').mkdir()
        (self.docs / 'archive-link').symlink_to('/unavailable/drive/archive')
        (self.root / 'bin').mkdir()
        for name in ['restic', 'ssh', 'hostname']:
            p = self.root / 'bin' / name
            p.write_text(FAKE)
            p.chmod(0o700)
        config = self.root / '.config/private-storage'
        config.mkdir(parents=True)
        (config / 'restic-password').write_text('synthetic')
        (config / 'config.json').write_text(json.dumps(dict(
            sharing_approved=True, folders=['documents'], remote='fixture:private',
            backup_root=str(self.root / 'remote'), recovery_days=31,
            crypt_secret_id='fixture', ssh_command=[str(self.root / 'bin/ssh')],
            ssh_hosts=['fixture'], ssh_hostname='fixture-server',
            drive_remote='fixture:', server_mount_root='/fixture')))
        coordination = self.root / 'remote/.coordination'
        coordination.mkdir(parents=True)
        (self.root / 'remote/li').mkdir()
        (self.root / 'remote/li/config').touch()
        now = datetime.datetime.now(datetime.timezone.utc)
        (coordination / 'fi.json').write_text(json.dumps(dict(host='fi', completed_epoch=int(now.timestamp()))))
        self.snapshot = dict(id='a' * 64, time=(now - datetime.timedelta(seconds=10)).isoformat(),
                             hostname='fi', tags=['private-storage'], paths=[str(self.docs)])
        self.save_snapshot()
        with tarfile.open(self.root / 'peer.tar', 'w') as archive:
            archive.add(self.docs, arcname='documents', recursive=True)
        self.env = {**os.environ, 'HOME': str(self.root), 'FIXTURE': str(self.root),
                    'PATH': str(self.root / 'bin') + ':/usr/bin:/bin'}
        self.state = self.root / '.local/state/private-storage'

    def save_snapshot(self):
        (self.root / 'snapshot.json').write_text(json.dumps(self.snapshot))

    def run_backup(self, scheduled=True, extra=None, success=True):
        result = subprocess.run([str(BINARY), 'private-assets', 'backup'] +
                                (['--scheduled'] if scheduled else []),
                                env={**self.env, **(extra or {})}, capture_output=True, timeout=20)
        self.assertEqual(result.returncode == 0, success, result.stderr.decode())
        calls = self.root / 'calls'
        return [json.loads(line) for line in calls.read_text().splitlines()] if calls.exists() else []

    def assert_own_backup(self, **kwargs):
        calls = self.run_backup(**kwargs)
        self.assertTrue(any('backup' in c for c in calls))
        self.assertTrue((self.state / 'last-backup.json').exists())
        self.assertFalse((self.state / 'last-backup-skip.json').exists())
        self.assertTrue((self.root / 'remote/.coordination/li.json').exists())

    def test_exact_content_links_and_empty_directories_skip_with_pinned_receipt(self):
        calls = self.run_backup()
        self.assertTrue(any('dump' in c and 'a' * 64 in c for c in calls))
        self.assertFalse(any('backup' in c for c in calls))
        proof = json.loads((self.state / 'last-backup-skip.json').read_text())
        self.assertEqual(proof['snapshot'], 'a' * 64)
        self.assertEqual(proof['entries'], 4)
        self.assertFalse((self.state / 'last-backup.json').exists())
        self.assertFalse((self.root / 'remote/.coordination/li.json').exists())

    def test_same_size_and_timestamp_but_changed_content_requires_backup(self):
        p = self.docs / 'file'
        before = p.stat()
        p.write_text('modified')
        os.utime(p, ns=(before.st_atime_ns, before.st_mtime_ns))
        self.assert_own_backup()

    def test_laptop_only_file_requires_backup(self):
        (self.docs / 'laptop-only').write_text('new')
        self.assert_own_backup()

    def test_deletion_requires_backup(self):
        (self.docs / 'file').unlink()
        self.assert_own_backup()

    def test_changed_link_requires_backup(self):
        p = self.docs / 'archive-link'
        p.unlink()
        p.symlink_to('/different/archive')
        self.assert_own_backup()

    def test_changed_permissions_require_backup(self):
        (self.docs / 'file').chmod(0o600)
        self.assert_own_backup()

    def test_unreadable_peer_or_changed_source_falls_back(self):
        self.assert_own_backup(extra={'FAIL_DUMP': '1'})

    def test_change_during_comparison_prevents_skip(self):
        self.assert_own_backup(extra={'CHANGE_DURING_DUMP': '1'})

    def test_snapshot_after_completion_receipt_is_not_trusted(self):
        self.snapshot['time'] = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=20)).isoformat()
        self.save_snapshot()
        self.assert_own_backup()

    def test_wrong_scope_is_not_trusted(self):
        self.snapshot['paths'] = ['/different/source']
        self.save_snapshot()
        self.assert_own_backup()

    def test_stale_peer_completion_does_not_skip(self):
        (self.root / 'remote/.coordination/fi.json').write_text(json.dumps(dict(host='fi', completed_epoch=0)))
        self.assert_own_backup()

    def test_malformed_peer_receipt_does_not_block_own_backup(self):
        (self.root / 'remote/.coordination/fi.json').write_text('invalid')
        self.assert_own_backup()

    def test_manual_backup_does_not_consult_peer_contents(self):
        calls = self.run_backup(scheduled=False)
        self.assertTrue(any('backup' in c for c in calls))
        self.assertFalse(any('dump' in c for c in calls))

    def test_own_backup_failure_never_records_skip_or_completion(self):
        (self.docs / 'laptop-only').write_text('new')
        self.run_backup(extra={'FAIL_BACKUP': '1'}, success=False)
        self.assertFalse((self.state / 'last-backup-skip.json').exists())
        self.assertFalse((self.root / 'remote/.coordination/li.json').exists())

    def test_excluded_cache_does_not_force_backup(self):
        (self.docs / 'node_modules').mkdir()
        (self.docs / 'node_modules/cache').write_text('not protected')
        self.assertFalse(any('backup' in c for c in self.run_backup()))


if __name__ == '__main__':
    unittest.main()
