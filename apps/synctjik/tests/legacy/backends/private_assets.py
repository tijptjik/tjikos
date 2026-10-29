#!/usr/bin/env python3
"""Local-first Drive sync, incremental si backups, and explicit mounts."""
import argparse
import contextlib
import datetime as dt
import fcntl
import json
import os
import re
import select
import signal
from pathlib import Path
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile

HOME = Path.home()
CONFIG = HOME / '.config/private-storage'
STATE = HOME / '.local/state/private-storage'
HOST = socket.gethostname().split('.')[0]



class Busy(RuntimeError):
    pass


def run(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)


def settings():
    return json.loads((CONFIG / 'config.json').read_text())


def note(stage, message):
    icons = {'SYNC': ('↻', '12'), 'DONE': ('✓', '10'), 'SKIP': ('–', '8'),
             'FAILED': ('✗', '9'), 'PLAN': ('·', '13'), 'BACKUP': ('⇧', '13'),
             'MOUNT': ('↗', '12')}
    icon, color = icons.get(stage, ('·', '8'))
    line = f'{icon} {message}'
    gum = shutil.which('gum')
    if gum and sys.stdout.isatty() and 'NO_COLOR' not in os.environ:
        result = subprocess.run([gum, 'style', '--foreground', color,
                                 '--bold' if stage in ('FAILED', 'DONE') else '--faint',
                                 line])
        if result.returncode == 0:
            return
    print(line, flush=True)


def failure_detail(log):
    try:
        lines = log.read_text(errors='replace').splitlines()
    except OSError:
        return 'rclone failed'
    for line in reversed(lines):
        marker = 'Safety abort: '
        if marker in line:
            return line.split(marker, 1)[1].strip()
    for line in reversed(lines):
        marker = 'Failed to bisync: '
        if marker in line:
            return line.split(marker, 1)[1].strip()
    return 'rclone failed'


def notify(message, critical=False):
    if shutil.which('notify-send'):
        subprocess.run(['notify-send', '-a', 'Private storage', '-u',
                        'critical' if critical else 'normal', 'Private storage', message],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


@contextlib.contextmanager
def locked(wait=False):
    STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (STATE / 'operation.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
        except BlockingIOError:
            raise Busy('Another private sync/backup is running; retry later.')
        yield


def si_ssh():
    conf = settings()
    ssh = conf['ssh_command']
    for host in conf['ssh_hosts']:
        probe = subprocess.run(ssh + [host, 'hostname'], capture_output=True, text=True)
        if probe.returncode == 0 and probe.stdout.strip() == conf['ssh_hostname']:
            return ssh + [host]
    raise RuntimeError('si is unreachable with its verified SSH host key')


def rclone():
    return ['rclone', '--config', str(CONFIG / 'drive.conf')]


def stamp():
    return dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')


def sync(initial=False, selected=None, progress=False, max_delete_percent=5):
    conf = settings()
    if conf.get('sharing_approved') is not True:
        raise RuntimeError('Drive folder sharing has not been approved; synchronization is blocked')
    if initial and not (CONFIG/'layout-ready').exists():
        raise RuntimeError('Drive reorganization is not verified yet; initialization is blocked')
    folders = selected or conf['folders']
    if not set(folders).issubset(conf['folders']):
        raise RuntimeError('Unknown sync folder')
    if max_delete_percent > 5 and (initial or selected is None or len(selected) != 1):
        raise RuntimeError('A reviewed delete-limit override requires exactly one folder')
    for folder in folders:
        local = HOME / folder
        remote = conf['remote'] + '/' + folder
        work = STATE / 'bisync' / folder
        receipt = work / 'initialized.json'
        if not local.is_dir() or local.is_symlink():
            raise RuntimeError(f'Expected a real local directory: {local}')
        if not initial and not receipt.exists():
            raise RuntimeError(f'{folder} has not been initialized; run private-storage init {folder}')
        if receipt.exists():
            baseline = json.loads(receipt.read_text())
            if baseline.get('host') != HOST or baseline.get('remote') != remote:
                raise RuntimeError(f'{folder}: baseline belongs to another client or destination; review required')
        if initial and receipt.exists():
            note('SKIP', f'{folder}: already initialized; use normal sync')
            continue
        work.mkdir(parents=True, exist_ok=True, mode=0o700)
        logdir = STATE / 'logs'
        logdir.mkdir(exist_ok=True)
        when = stamp()
        log = logdir / f'{when}-{folder}.log'
        recovery = STATE / 'recovery' / folder / when
        recovery.mkdir(parents=True)
        args = rclone() + ['bisync', str(local), remote, '--workdir', str(work),
            '--filters-file', str(CONFIG / 'sync-filter.txt'), '--drive-skip-gdocs', '--skip-links',
            '--check-access', '--check-filename', '.private-sync-access',
            '--max-delete', str(max_delete_percent), '--max-delete-size', '1Gi',
            '--conflict-resolve', 'none', '--conflict-loser', 'num',
            '--conflict-suffix', f'conflict-{HOST}', '--recover', '--resilient',
            '--backup-dir1', str(recovery), '--backup-dir2',
            conf['remote'] + f'/backups/sync-recovery/{HOST}/{folder}/{when}',
            '--create-empty-src-dirs', '--log-level', 'INFO', '--log-file', str(log)]
        if initial:
            # fi establishes the cloud baseline. Other clients prefer that
            # established copy; displaced versions remain in recovery.
            marker = local / '.private-sync-access'
            if not marker.exists():
                marker.write_text('Private storage availability check. Do not delete.\n')
            run(rclone() + ['copyto', str(marker), remote + '/.private-sync-access'])
            priority = 'path1' if HOST == 'fi' else 'path2'
            args += ['--resync', '--resync-mode', priority]
            winner = 'desktop local' if HOST == 'fi' else 'established cloud'
            note('PLAN', f'{folder}: initial union; {winner} wins, displaced files retained')
            capacity = json.loads(run(rclone() + ['size', remote, '--json', '--drive-skip-gdocs',
                '--filter-from', str(CONFIG/'sync-filter.txt')], capture_output=True, text=True).stdout)
            if shutil.disk_usage(local).free < capacity['bytes'] + 5 * 1024**3:
                raise RuntimeError(f'Insufficient local space for initial {folder} sync plus 5 GiB reserve')
            run(args + ['--dry-run'])
            preview = log.read_text(errors='replace').lower()
            if 'duplicate object' in preview or 'duplicate directory' in preview:
                raise RuntimeError(f'Duplicate Drive paths require review before initial sync: {log}')
        note('SYNC', folder)
        try:
            run(args + (['--progress', '--stats-one-line', '--stats', '1s']
                        if progress else []))
        except subprocess.CalledProcessError as error:
            raise RuntimeError(
                f'{folder}: {failure_detail(log)}; details: {log}'
            ) from error
        receipt.write_text(json.dumps({'completed': when, 'host': HOST, 'remote': remote}) + '\n')
        text = log.read_text(errors='replace')
        if 'conflict' in text.lower() and ('renam' in text.lower() or 'WARNING' in text):
            notify(f'{folder}: conflict copies retained. Review {log}', critical=True)
        prune_recovery(folder)
        note('DONE', folder)
    (STATE / 'last-sync.json').write_text(json.dumps({'completed': stamp(), 'folders': folders}) + '\n')


def restic_command(source=HOST):
    if source not in ('fi', 'li'):
        raise RuntimeError('Choose backup source fi or li')
    ssh = si_ssh()
    root = settings()['backup_root'] + '/' + source
    command = shlex.join(ssh[:-1] + ['-s', ssh[-1], 'sftp'])
    return ['restic', '-r', 'sftp:si:' + root, '-p', str(CONFIG / 'restic-password'),
            '-o', 'sftp.command=' + command, '--retry-lock', '5m'], ssh, root


def private_retention(snapshot_list, now=None):
    """Keep one snapshot in each requested age band, then one per older year."""
    now = now or dt.datetime.now(dt.timezone.utc)
    bands = ((0, 1), (1, 3), (3, 7), (7, 14), (14, 31), (31, 92), (92, 365))
    parsed = []
    for snapshot in snapshot_list:
        timestamp = dt.datetime.fromisoformat(snapshot['time'].replace('Z', '+00:00'))
        if timestamp.tzinfo is None:
            timestamp = timestamp.replace(tzinfo=dt.timezone.utc)
        parsed.append((timestamp.astimezone(dt.timezone.utc), snapshot['id']))
    keep = set()
    ages = [(max(0.0, (now - timestamp).total_seconds() / 86400), timestamp, ident)
            for timestamp, ident in parsed]
    for start, end in bands:
        candidates = [item for item in ages if start <= item[0] < end]
        if candidates:
            keep.add(max(candidates, key=lambda item: item[1])[2])
    annual = {}
    for age, timestamp, ident in ages:
        if age >= 365:
            if timestamp.year not in annual or timestamp > annual[timestamp.year][0]:
                annual[timestamp.year] = (timestamp, ident)
    keep.update(item[1] for item in annual.values())
    if not keep and parsed:
        keep.add(max(parsed)[1])
    return keep


def prune_recovery(folder):
    # Timestamp directory names define age; source-file mtimes do not.
    if not (STATE / 'last-backup.json').exists():
        return
    days = settings().get('recovery_days', 31)
    cutoff = dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=days)
    def expired(name):
        if not re.fullmatch(r'\d{8}T\d{6}Z', name):
            return False
        return dt.datetime.strptime(name, '%Y%m%dT%H%M%SZ').replace(tzinfo=dt.timezone.utc) < cutoff
    local = STATE / 'recovery' / folder
    if local.exists():
        for path in local.iterdir():
            if path.is_dir() and not path.is_symlink() and expired(path.name):
                shutil.rmtree(path)
    remote = settings()['remote'] + f'/backups/sync-recovery/{HOST}/{folder}'
    listing = subprocess.run(rclone() + ['lsf', remote, '--dirs-only'], capture_output=True, text=True)
    if listing.returncode == 0:
        for name in listing.stdout.splitlines():
            name = name.rstrip('/')
            if expired(name):
                run(rclone() + ['purge', remote + '/' + name])


# This small coordinator runs on si, holding flock over one SSH connection.
# EOF releases the lock without recording success; only a verified backup sends
# "complete". The receipt is outside either device's Restic repository.
BACKUP_COORDINATOR = r"""
import fcntl, json, os, sys, time
from pathlib import Path
root, host, cutoff, scheduled = sys.argv[1:]
assert host in ('fi', 'li')
os.umask(0o077)
state = Path(root) / '.coordination'
state.mkdir(parents=True, exist_ok=True)
with (state / 'backup.lock').open('a') as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print('SKIP another client backup is running', flush=True)
        sys.exit(0)
    receipt = state / 'fi.json'
    if host == 'li' and scheduled == '1' and receipt.exists():
        saved = json.loads(receipt.read_text())
        completed = float(saved['completed_epoch'])
        if saved['host'] != 'fi' or completed > time.time() + 300:
            raise RuntimeError('Invalid desktop backup receipt')
        if completed >= float(cutoff):
            print('SKIP fi already completed a verified backup today', flush=True)
            sys.exit(0)
    print('READY', flush=True)
    if sys.stdin.readline().strip() == 'complete':
        staged = state / (host + '.json.tmp')
        staged.write_text(json.dumps({'host': host, 'completed_epoch': time.time()}))
        staged.replace(state / (host + '.json'))
        print('SAVED', flush=True)
"""


def coordinator_reply(process):
    if not select.select([process.stdout], [], [], 40)[0]:
        raise RuntimeError('Timed out waiting for backup coordination on si')
    reply = process.stdout.readline().strip()
    if not reply:
        raise RuntimeError('Lost backup coordination connection to si')
    return reply


@contextlib.contextmanager
def backup_coordination(ssh, root, scheduled=False):
    # Calendar-day policy follows the client timer's local timezone.
    midnight = dt.datetime.now().replace(hour=0, minute=0, second=0, microsecond=0).timestamp()
    command = shlex.join(['python3', '-u', '-c', BACKUP_COORDINATOR,
                          root, HOST, str(midnight), '1' if scheduled else '0'])
    process = subprocess.Popen(ssh + [command], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, text=True)
    try:
        reply = coordinator_reply(process)
        if reply.startswith('SKIP '):
            raise Busy(reply[5:])
        if reply != 'READY':
            raise RuntimeError('Unexpected backup coordinator response')
        yield process
        process.stdin.write('complete\n')
        process.stdin.flush()
        if coordinator_reply(process) != 'SAVED':
            raise RuntimeError('Backup completion was not acknowledged by si')
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        process.stdout.close()


def coordinated_run(args, coordinator):
    # Stop the transfer if the remote lock connection disappears. Do not let a
    # detached Restic/SFTP child continue after this client loses its lock.
    process = subprocess.Popen([str(x) for x in args], start_new_session=True)
    try:
        while True:
            if coordinator.poll() is not None:
                raise RuntimeError('Lost backup coordination lock on si; stopping backup')
            try:
                result = process.wait(timeout=1)
                break
            except subprocess.TimeoutExpired:
                pass
        if result:
            raise subprocess.CalledProcessError(result, args)
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()


def backup(scheduled=False):
    conf = settings()
    common, ssh, root = restic_command()
    with backup_coordination(ssh, conf['backup_root'], scheduled) as coordinator:
        # Per-device repositories isolate retention while preserving cross-device restore.
        run(ssh + ['mkdir -p ' + shlex.quote(root)])
        probe = run(ssh + ['test -f ' + shlex.quote(root + '/config') + ' && echo exists || echo missing'],
                    capture_output=True, text=True)
        if probe.stdout.strip() == 'missing':
            coordinated_run(common + ['init'], coordinator)
        paths = [HOME / name for name in conf['folders']]
        if any(not p.is_dir() or p.is_symlink() for p in paths):
            raise RuntimeError('All private folders must exist as real local directories before backup')
        note('BACKUP', f'{HOST} → si:{root}')
        coordinated_run(common + ['backup', '--host', HOST, '--tag', 'private-storage',
                      '--exclude', '**/.git', '--exclude', '**/node_modules',
                      '--exclude', '**/.venv', '--exclude', '**/__pycache__',
                      '--exclude', '**/.Trash-*',
                      '--exclude', '**/.private-sync-access', *map(str, paths)], coordinator)
        coordinated_run(common + ['check'], coordinator)
        snapshots = json.loads(run(common + ['snapshots', '--json', '--host', HOST,
                            '--tag', 'private-storage'], capture_output=True, text=True).stdout)
        keep = private_retention(snapshots)
        remove = [snapshot['id'] for snapshot in snapshots if snapshot['id'] not in keep]
        if remove:
            coordinated_run(common + ['forget', *remove, '--prune'], coordinator)
    (STATE / 'last-backup.json').write_text(json.dumps({'completed': stamp(), 'repository': root}) + '\n')
    note('DONE', 'Incremental backup verified; daily/3-day/weekly/2-week/monthly/quarterly/annual retention applied')


def crypt_password():
    # The recovery secret remains in Bitwarden; only a passphrase-encrypted copy
    # is stored locally. age prompts on the user's terminal, never in chat.
    dest = CONFIG / 'crypt.conf.age'
    with tempfile.TemporaryDirectory(prefix='crypt-password-', dir=STATE) as directory:
        temp = Path(directory)
        secret = run([HOME / '.local/share/dotjik/home/.bws-wrapper.fish', 'secret', 'get',
                      settings()['crypt_secret_id']], capture_output=True, text=True)
        plain = temp / 'config'
        plain.write_text(json.loads(secret.stdout)['value'])
        encrypted = temp / 'config.age'
        run(['age', '-p', '-o', encrypted, plain])
        encrypted.replace(dest)
    note('DONE', 'Crypt mount password set. Run mntcrypt to unlock and mount.')


def mount(which):
    mountpoints = {'crypt': HOME/'.mnt/rclone/crypt', 'giprivate': HOME/'.mnt/google/private',
                  'gipublic': HOME/'.mnt/google/public', 'siprivate': HOME/'.mnt/si/private',
                  'sipublic': HOME/'.mnt/si/public'}
    point = mountpoints[which]
    if os.path.ismount(point):
        note('SKIP', f'Already mounted: {point}')
        return
    point.mkdir(parents=True, exist_ok=True)
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid()))) / 'private-storage'
    runtime.mkdir(mode=0o700, exist_ok=True)
    if which == 'crypt':
        if not (CONFIG/'crypt.conf.age').exists():
            raise RuntimeError('Set your password first: mntcrypt --set-password')
        plain = runtime/'crypt.conf'
        staged = runtime/'crypt.conf.new'
        staged.unlink(missing_ok=True)
        try:
            run(['age', '-d', '-o', staged, CONFIG/'crypt.conf.age'])
            staged.replace(plain)
        finally:
            staged.unlink(missing_ok=True)
        args = ['rclone', '--config', str(plain), 'mount', 'crypt:', str(point)]
        device_name = 'crypt'
    elif which.startswith('gi'):
        args = rclone() + ['mount', settings()['drive_remote'] + which[2:], str(point), '--drive-skip-gdocs']
        device_name = 'gi-' + which[2:]
    else:
        args = ['rclone', 'mount', ':sftp:' + settings()['server_mount_root'].rstrip('/') + '/' + which[2:], str(point),
                '--sftp-ssh', shlex.join(si_ssh()), '--sftp-known-hosts-file', 'none',
                '--sftp-shell-type', 'unix', '--sftp-disable-hashcheck']
        device_name = 'si-' + which[2:]
    # A transient service survives the terminal and restarts mount failures.
    unit = 'private-mount-' + which
    run(['systemd-run', '--user', '--collect', '--unit', unit, '--property=Type=notify',
         '--property=Restart=on-failure', '--property=RestartSec=15',
         '--property=TimeoutStopSec=120',
         '--property=ExecStopPost=-/usr/bin/fusermount3 -uz ' + str(point),
         *args, '--devname', device_name, '--vfs-cache-mode', 'writes'])
    note('MOUNT', str(point))


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    sync_parser = sub.add_parser('sync')
    sync_parser.add_argument('folders', nargs='*')
    sync_parser.add_argument('--progress', action='store_true',
                             help='show rclone transfer progress in the terminal')
    sync_parser.add_argument('--max-delete-percent', type=int, choices=range(5, 101), default=5,
                             help='temporarily raise the delete guard for one reviewed folder')
    sub.add_parser('init').add_argument('folders', nargs='*')
    sub.add_parser('backup').add_argument('--scheduled', action='store_true',
        help='on li, skip if fi has completed a verified backup today')
    sub.add_parser('snapshots').add_argument('--from', dest='source', choices=['fi','li'], default=HOST)
    restore = sub.add_parser('restore')
    restore.add_argument('--from', dest='source', choices=['fi','li'], required=True)
    restore.add_argument('--snapshot', default='latest')
    restore.add_argument('--to', type=Path, required=True)
    sub.add_parser('status')
    sub.add_parser('ready')
    sub.add_parser('crypt-password')
    sub.add_parser('mount').add_argument('which', choices=['crypt','giprivate','gipublic','siprivate','sipublic'])
    args = parser.parse_args()
    STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        if args.action == 'ready':
            conf = settings()
            return 0 if conf.get('sharing_approved') is True and all(
                (STATE/'bisync'/name/'initialized.json').exists() for name in conf['folders']) else 1
        if args.action == 'mount': mount(args.which)
        elif args.action == 'crypt-password': crypt_password()
        elif args.action == 'snapshots':
            common, _, _ = restic_command(args.source)
            run(common + ['snapshots'])
        elif args.action == 'restore':
            if args.to.exists():
                raise RuntimeError('Restore destination must be a new directory')
            common, _, _ = restic_command(args.source)
            run(common + ['restore', args.snapshot, '--target', args.to, '--verify'])
        elif args.action == 'status':
            for name in ('last-sync.json', 'last-backup.json', 'last-error.json'):
                path = STATE/name
                print(name, path.read_text() if path.exists() else 'not completed')
        else:
            with locked(wait=args.action == 'backup'):
                if args.action == 'backup':
                    archive_state = HOME / '.local/state/device-archives'
                    archive_state.mkdir(parents=True, exist_ok=True, mode=0o700)
                    with (archive_state/'lock').open('a') as archive_lock:
                        fcntl.flock(archive_lock, fcntl.LOCK_EX)
                        backup(args.scheduled)
                else:
                    sync(args.action == 'init', args.folders,
                         getattr(args, 'progress', False),
                         getattr(args, 'max_delete_percent', 5))
                    if args.action == 'init': notify('Initial sync completed. Hourly synchronization is ready.')
    except Busy as error:
        note('SKIP', str(error))
        return 0
    except (RuntimeError, subprocess.CalledProcessError, OSError, ValueError) as error:
        (STATE/'last-error.json').write_text(json.dumps({'time': stamp(), 'operation': args.action, 'error': str(error)}) + '\n')
        note('FAILED', str(error))
        notify(str(error), critical=True)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
