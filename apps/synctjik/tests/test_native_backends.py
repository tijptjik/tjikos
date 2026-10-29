"""Exercise the native binary with synthetic executables, policies and receipts.
No live credentials, networks, mounts or backup repositories are used.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

APP = Path(__file__).resolve().parents[1]
BINARY = APP / 'target/debug/synctjik'
FAKE = r'''#!/usr/bin/python3
import json, os, sys, subprocess
from pathlib import Path
name = Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ['CALLS'], 'a') as f:
    f.write(json.dumps([name, *args]) + '\n')
if name == 'hostname': print(os.environ.get('HOST_FIXTURE', 'fi'))
elif name == 'ssh':
    if args[-1] == 'hostname': print('si')
    else: sys.exit(subprocess.call(['sh', '-c', args[-1]]))
elif name == 'rclone':
    if 'size' in args: print('{"bytes":0,"count":0}')
    if '--log-file' in args:
        Path(args[args.index('--log-file') + 1]).write_text('duplicate object' if os.environ.get('DUPLICATE') else 'Sync completed')
    if os.environ.get('FAIL_SYNC') and 'bisync' in args: sys.exit(9)
elif name == 'restic':
    if 'check' in args and os.environ.get('FAIL_CHECK'): sys.exit(7)
    if 'snapshots' in args:
        from datetime import datetime, timezone
        print(json.dumps([{'id':'a'*64, 'time':datetime.now(timezone.utc).isoformat(), 'hostname':os.environ.get('HOST_FIXTURE','fi'), 'tags':['private-storage']}]))
elif name == 'bws':
    assert os.environ['BWS_ACCESS_TOKEN'] == 'synthetic-token'
    if os.environ.get('FAIL_BWS'):
        print('synthetic-private-diagnostic', file=sys.stderr)
        sys.exit(1)
    print(json.dumps({'id':'wrong' if os.environ.get('WRONG_SECRET') else args[2], 'value':'synthetic-crypt-config'}))
elif name == 'age':
    assert Path(args[-1]).read_text() == 'synthetic-crypt-config'
    Path(args[args.index('-o') + 1]).write_text('synthetic-encrypted-config')
'''


@unittest.skipUnless(BINARY.exists(), 'build native synctjik first')
class NativeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.config = self.root / '.config/private-storage'
        self.config.mkdir(parents=True)
        self.state = self.root / '.local/state/private-storage'
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        for name in ('hostname', 'ssh', 'rclone', 'restic', 'notify-send', 'bws', 'age'):
            f = self.bin / name
            f.write_text(FAKE)
            f.chmod(0o700)
        self.calls = self.root / 'calls'
        self.env = {**os.environ, 'HOME':str(self.root), 'PATH':str(self.bin)+':/usr/bin:/bin', 'CALLS':str(self.calls)}
        self.env.pop('BWS_ACCESS_TOKEN', None)
        self.policy = dict(sharing_approved=True, folders=['documents'], remote='fixture:private', backup_root=str(self.root/'server'), crypt_secret_id='fixture', ssh_command=['ssh'], ssh_hosts=['si'], ssh_hostname='si', drive_remote='fixture:', server_mount_root='/fixture')
        self.save_policy()
        (self.root/'documents').mkdir()

    def save_policy(self):
        (self.config/'config.json').write_text(json.dumps(self.policy))

    def run_native(self, *args, ok=True):
        result = subprocess.run([str(BINARY), 'private-assets', *args], env=self.env, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode == 0, ok, result.stderr)
        return result

    def commands(self, name):
        return [c for c in map(json.loads,self.calls.read_text().splitlines()) if c[0] == name]

    def baseline(self, host='fi'):
        p = self.state/'bisync/documents/initialized.json'
        p.parent.mkdir(parents=True)
        p.write_text(json.dumps({'host':host,'remote':'fixture:private/documents'}))

    def test_crypt_password_native_secret_provider_and_atomic_failure(self):
        self.policy['crypt_secret_id'] = '00000000-0000-0000-0000-000000000001'
        self.save_policy()
        token = self.root / '.config/bws/environment'
        token.parent.mkdir()
        token.write_text('BWS_ACCESS_TOKEN=synthetic-token\n')
        token.chmod(0o600)
        encrypted = self.config / 'crypt.conf.age'
        encrypted.write_text('previous')
        for flag in ('FAIL_BWS', 'WRONG_SECRET'):
            self.env[flag] = '1'
            result = self.run_native('crypt-password', ok=False)
            self.assertNotIn('synthetic-private-diagnostic', result.stderr)
            self.assertEqual(encrypted.read_text(), 'previous')
            del self.env[flag]
        token.chmod(0o644)
        self.run_native('crypt-password', ok=False)
        self.assertEqual(encrypted.read_text(), 'previous')
        token.chmod(0o600)
        self.run_native('crypt-password')
        self.assertEqual(encrypted.read_text(), 'synthetic-encrypted-config')
        self.assertEqual(encrypted.stat().st_mode & 0o777, 0o600)
        self.assertFalse(list(self.state.glob('crypt-password-*')))
        token.unlink()
        self.env['BWS_ACCESS_TOKEN'] = 'synthetic-token'
        self.run_native('crypt-password')

    def test_sync_guards_and_success_receipts(self):
        self.run_native('sync',ok=False)
        self.assertEqual(self.commands('rclone'),[])
        self.baseline('li')
        self.run_native('sync',ok=False)
        self.assertEqual(self.commands('rclone'),[])
        p=self.state/'bisync/documents/initialized.json'
        p.write_text(json.dumps({'host':'fi','remote':'fixture:private/documents'}))
        self.run_native('sync')
        cmd=self.commands('rclone')[-1]
        for flag in ('--check-access','--filters-file','--backup-dir1','--backup-dir2','--recover','--resilient'):
            self.assertIn(flag,cmd)
        self.assertNotIn('--resync',cmd)
        self.assertEqual(cmd[cmd.index('--max-delete')+1],'5')
        self.assertTrue((self.state/'last-sync.json').exists())

    def test_symlink_and_sharing_guards(self):
        self.baseline()
        (self.root/'documents').rmdir()
        (self.root/'documents').symlink_to(self.root/'bin',target_is_directory=True)
        self.run_native('sync',ok=False)
        self.assertEqual(self.commands('rclone'),[])
        self.policy['sharing_approved']=False
        self.save_policy()
        self.run_native('sync',ok=False)
        self.assertEqual(self.commands('rclone'),[])

    def test_initialization_preview_then_union_and_duplicate_abort(self):
        self.run_native('init',ok=False)
        (self.config/'layout-ready').touch()
        self.env['DUPLICATE']='1'
        self.run_native('init',ok=False)
        self.assertFalse((self.state/'bisync/documents/initialized.json').exists())
        self.assertTrue(all('--dry-run' in c for c in self.commands('rclone') if 'bisync' in c))
        del self.env['DUPLICATE']
        self.env['HOST_FIXTURE']='li'
        self.run_native('init')
        cmd=self.commands('rclone')[-1]
        self.assertNotIn('--dry-run',cmd)
        self.assertEqual(cmd[cmd.index('--resync-mode')+1],'path2')

    def test_failed_sync_does_not_claim_success(self):
        self.baseline()
        self.env['FAIL_SYNC']='1'
        result=self.run_native('sync',ok=False)
        self.assertFalse((self.state/'last-sync.json').exists())
        self.assertNotIn('duplicate',result.stderr)

    def test_backup_failure_no_receipts_then_verified_success(self):
        self.env['FAIL_CHECK']='1'
        self.run_native('backup',ok=False)
        self.assertFalse((self.state/'last-backup.json').exists())
        self.assertFalse((self.root/'server/.coordination/fi.json').exists())
        self.assertFalse(any('forget' in c for c in self.commands('restic')))
        del self.env['FAIL_CHECK']
        self.run_native('backup')
        self.assertTrue((self.state/'last-backup.json').exists())
        self.assertTrue((self.root/'server/.coordination/fi.json').exists())

    def test_scheduled_laptop_skips_verified_desktop_but_manual_runs(self):
        self.run_native('backup')
        self.env['HOST_FIXTURE']='li'
        count=len(self.commands('restic'))
        result=self.run_native('backup','--scheduled')
        self.assertIn('already completed',result.stdout)
        self.assertEqual(len(self.commands('restic')),count)
        self.run_native('backup')
        self.assertGreater(len(self.commands('restic')),count)

    def test_restore_refuses_existing_or_dangling_symlink(self):
        self.run_native('restore','--from','fi','--to',str(self.root/'documents'),ok=False)
        dest=self.root/'dangling'
        dest.symlink_to(self.root/'absent')
        self.run_native('restore','--from','fi','--to',str(dest),ok=False)
        self.assertEqual(self.commands('restic'),[])
        self.run_native('restore','--from','fi','--to',str(self.root/'restored'))
        self.assertIn('--verify',self.commands('restic')[-1])


if __name__ == '__main__':
    unittest.main()
