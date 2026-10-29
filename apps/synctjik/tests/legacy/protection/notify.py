#!/usr/bin/python3
"""SnapRAID failure/recovery alerts; no parity or application writes."""
import datetime as dt
import fcntl
import html
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.error
import urllib.request

CONFIG = Path('/etc/si-notifications/telegram.json')
STATE = Path('/var/lib/si-snapraid-notifications')
JOBS = {
    'si-snapraid-sync.service': ('SnapRAID sync', 'sync', 36 * 3600),
    'si-snapraid-scrub.service': ('SnapRAID scrub', 'scrub', 9 * 86400),
    'si-snapraid-resume.service': ('SnapRAID writer recovery', None, None),
    'si-snapraid-notification-test.service': ('TEST: SnapRAID notification', None, None),
}
RECEIPTS = Path('/var/lib/si-snapraid-maintenance')


def utcnow():
    return dt.datetime.now(dt.timezone.utc)


def read_json(path):
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return {}


def write_json(path, value):
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(value, indent=2))
    tmp.replace(path)


def unit_status(unit):
    result = subprocess.run(
        ['systemctl', 'show', unit, '-p', 'ActiveState', '-p', 'Result',
         '-p', 'ExecMainStatus', '-p', 'ExecMainStartTimestampMonotonic'],
        text=True, capture_output=True, check=True, timeout=10)
    return dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)


def send(text):
    if CONFIG.stat().st_mode & 0o077:
        raise RuntimeError('Telegram configuration must be root-only')
    config = read_json(CONFIG)
    payload = {'chat_id': config['chat_id'], 'text': text,
               'disable_notification': False, 'parse_mode': 'HTML'}
    if config.get('topic_id'):
        payload['message_thread_id'] = config['topic_id']
    request = urllib.request.Request(
        'https://api.telegram.org/bot' + config['bot_token'] + '/sendMessage',
        data=json.dumps(payload).encode(), headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            result = json.load(response)
        if not result.get('ok'):
            raise RuntimeError('Telegram rejected the notification')
    except urllib.error.HTTPError as exc:
        # Never print the request URL: it contains the bot token.
        raise RuntimeError(f'Telegram delivery failed: HTTP {exc.code}') from None
    except (OSError, ValueError) as exc:
        raise RuntimeError('Telegram delivery failed: ' + type(exc).__name__) from None
    return result['result']['message_id']



def format_message(unit, problem=None):
    test = unit == 'si-snapraid-notification-test.service'
    if test:
        title = 'Test alert' if problem is not None else 'Test passed'
        body = ('This is a notification test. No storage job failed.'
                if problem is not None else
                'Failure and recovery notifications are working.')
        return (f'🧪 <b>SnapRAID · {title}</b>\n'
                f'<code>si</code> · Notification check\n\n{body}')
    task = JOBS[unit][0].removeprefix('SnapRAID ').capitalize()
    if problem is None:
        return (f'✅ <b>SnapRAID · Recovered</b>\n'
                f'<code>si</code> · {html.escape(task)}\n\n'
                'Maintenance completed successfully.')
    command = 'journalctl -u ' + unit.removesuffix('.service') + ' -n 50'
    return (f'⚠️ <b>SnapRAID · Action needed</b>\n'
            f'<code>si</code> · {html.escape(task)}\n\n'
            f'{html.escape(problem)}\n\n'
            f'<b>Review logs</b>\n<pre>{html.escape(command)}</pre>')


def incident(unit, problem, current=None):
    current = current or utcnow()
    path = STATE / (unit + '.json')
    state = read_json(path)
    last = state.get('last_sent_at')
    if (state.get('active') and state.get('problem') == problem and last
            and (current - dt.datetime.fromisoformat(last)).total_seconds() < 86400):
        return
    # Keep an undelivered incident pending if Telegram is temporarily unavailable.
    state.update(active=True, problem=problem, last_sent_at=None)
    write_json(path, state)
    message_id = send(format_message(unit, problem))
    state.update(last_sent_at=current.isoformat(), message_id=message_id)
    write_json(path, state)
    print(f'Alert delivered for {unit}; Telegram message {message_id}', flush=True)


def recover(unit):
    path = STATE / (unit + '.json')
    state = read_json(path)
    if not state.get('active'):
        return
    message_id = send(format_message(unit))
    write_json(path, {'active': False, 'recovered_at': utcnow().isoformat(),
                      'message_id': message_id})
    print(f'Recovery delivered for {unit}; Telegram message {message_id}', flush=True)


def failure_reason(unit, status):
    mode = JOBS[unit][1]
    if mode:
        failure = read_json(RECEIPTS / (mode + '-failure.json'))
        # Prefer the current failure record, never an earlier run's stale error.
        try:
            stamp = dt.datetime.fromisoformat(failure['failed_at'])
            if 0 <= (utcnow() - stamp).total_seconds() <= 600:
                return str(failure['error'])[:450]
        except (KeyError, TypeError, ValueError):
            pass
    return ('Job failed or a safety check blocked it. '
            f"Result={status.get('Result', 'unknown')}; exit={status.get('ExecMainStatus', 'unknown')}.")


def health():
    for unit, (_, mode, max_age) in JOBS.items():
        if mode is None:
            continue
        status = unit_status(unit)
        if status.get('ActiveState') == 'failed':
            incident(unit, failure_reason(unit, status))
            continue
        try:
            receipt = read_json(RECEIPTS / ('last-' + mode + '-success.json'))
            completed = dt.datetime.fromisoformat(receipt['completed_at'])
            age = (utcnow() - completed).total_seconds()
            fresh = 0 <= age <= max_age
        except (KeyError, TypeError, ValueError):
            fresh = False
        running = status.get('ActiveState') in ('active', 'activating')
        if running:
            started = int(status.get('ExecMainStartTimestampMonotonic') or 0) / 1e6
            if started and time.monotonic() - started > max_age:
                incident(unit, f'Job has been running longer than {max_age // 3600} hours; inspect it.')
            # Do not mistake a currently running job for a missed schedule.
            continue
        if not fresh:
            incident(unit, f'No verified successful {mode} within {max_age // 3600} hours. '
                     'Check its timer, dependencies and logs.')
        else:
            recover(unit)
    # The boot-resume unit stays active (RemainAfterExit), so OnSuccess cannot
    # announce its recovery until shutdown. Verify it here instead.
    unit = 'si-snapraid-resume.service'
    if read_json(STATE / (unit + '.json')).get('active'):
        status = unit_status(unit)
        if status.get('Result') == 'success' and status.get('ActiveState') == 'active':
            recover(unit)


def main():
    os.umask(0o077)
    STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (STATE / 'notify.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        action = sys.argv[1]
        if action == 'health':
            health()
            return
        unit = sys.argv[2]
        if unit not in JOBS:
            raise RuntimeError('Unknown maintenance unit')
        if action == 'failure':
            status = unit_status(unit)
            if status.get('Result') == 'success' and status.get('ActiveState') != 'failed':
                recover(unit)  # A queued alert can run after the job already recovered.
            else:
                incident(unit, failure_reason(unit, status))
        elif action == 'recovery':
            status = unit_status(unit)
            if status.get('Result') == 'success' and status.get('ActiveState') != 'failed':
                recover(unit)
        else:
            raise RuntimeError('Unknown notification action')


if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        raise SystemExit(1)
