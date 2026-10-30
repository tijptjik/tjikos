"""Private todo authority. One atomic file per task; all mutations serialized."""
import calendar
import datetime as dt
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import uuid
import tomllib
from urllib.parse import urlsplit

GROUPS = ['All', 'Quick', 'General', 'Work', 'Socials', 'Domestic', 'Finances', 'Tech']
OPTIONS = {'authority': 'si', 'directory': '~/.local/share/runtjik-todo', 'vault': '', 'ssh_command': ['ssh'], 'url': '', 'socket': ''}
UNITS = ['days', 'weeks', 'months', 'years', 'days-of-the-week', 'days-of-the-month']


def config():
    root = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config'))
    path = root / 'tasktjik/tasktjik.conf'
    legacy = root / 'runtjik/runtjik.conf'
    options = dict(OPTIONS)
    if legacy.is_file():
        options.update(tomllib.loads(legacy.read_text()).get('plugin_settings', {}).get('Todo', {}))
    if path.is_file():
        options.update(tomllib.loads(path.read_text()).get('tasktjik', {}))
    return {'plugin_settings': {'Todo': options}}


def lifecycle_defaults(task):
    task.setdefault('status', 'done' if task.get('completedAt') else 'ready')
    for key, value in {'source': {}, 'assignee': '', 'project': '', 'blockers': [],
                       'acceptance': [], 'verification': [], 'events': []}.items():
        task.setdefault(key, value)


def today():
    return dt.date.today()


def stamp():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def occurrence(task, day):
    """Latest anchored period at day; month ends clamp without drifting."""
    if not task.get('due'):
        return ''
    start = dt.date.fromisoformat(task['due'])
    rule = task.get('repeat') or {}
    unit, value = rule.get('unit'), rule.get('value', 1)
    if not unit or (day < start and not unit.startswith('days-of-')):
        return start.isoformat()
    if unit in ('days', 'weeks'):
        span = int(value) * (7 if unit == 'weeks' else 1)
        return (start + dt.timedelta(days=((day - start).days // span) * span)).isoformat()
    if unit in ('months', 'years'):
        span = int(value) * (12 if unit == 'years' else 1)
        months = (day.year - start.year) * 12 + day.month - start.month
        n = months // span
        def date_at(index):
            y, m = divmod(start.year * 12 + start.month - 1 + index * span, 12)
            return dt.date(y, m + 1, min(start.day, calendar.monthrange(y, m + 1)[1]))
        result = date_at(n)
        return date_at(max(0, n - 1)).isoformat() if result > day else result.isoformat()
    allowed = {int(v) for v in str(value).split(',')}
    cursor = day
    while cursor >= start:
        if (cursor.isoweekday() if unit == 'days-of-the-week' else cursor.day) in allowed:
            return cursor.isoformat()
        cursor -= dt.timedelta(days=1)
    cursor = start
    while (cursor.isoweekday() if unit == 'days-of-the-week' else cursor.day) not in allowed:
        cursor += dt.timedelta(days=1)
    return cursor.isoformat()


def next_occurrence(task, period):
    current = dt.date.fromisoformat(period)
    rule = task['repeat']
    unit, value = rule['unit'], rule['value']
    if unit in ('days', 'weeks'):
        return (current + dt.timedelta(days=int(value) * (7 if unit == 'weeks' else 1))).isoformat()
    if unit in ('months', 'years'):
        anchor = dt.date.fromisoformat(task['due'])
        span = int(value) * (12 if unit == 'years' else 1)
        y, m = divmod(current.year * 12 + current.month - 1 + span, 12)
        return dt.date(y, m + 1, min(anchor.day, calendar.monthrange(y, m + 1)[1])).isoformat()
    allowed = {int(v) for v in str(value).split(',')}
    current += dt.timedelta(days=1)
    while (current.isoweekday() if unit == 'days-of-the-week' else current.day) not in allowed:
        current += dt.timedelta(days=1)
    return current.isoformat()


def validate(task):
    task['title'] = str(task.get('title', '')).strip()
    if not task['title']:
        raise ValueError('Title is required')
    task['group'] = str(task.get('group') or 'All').strip() or 'All'
    if len(task['group']) > 100 or any(ord(c) < 32 for c in task['group']):
        raise ValueError('Group must be a short single-line name')
    if task.get('kind', 'todo') not in ('todo', 'reading'):
        raise ValueError('Unknown item type')
    if task.get('kind') == 'reading':
        reference = task.get('reference', '')
        if not isinstance(reference, str) or len(reference) > 16384:
            raise ValueError('Invalid reading URL')
        parsed = urlsplit(reference)
        if parsed.scheme not in ('http', 'https') or not parsed.hostname:
            raise ValueError('Reading items need an HTTP or HTTPS reference')
    if not isinstance(task.get('source', {}), dict) or not isinstance(task.get('acceptance', []), list):
        raise ValueError('Source must be an object and acceptance a list')
    if task.get('due'):
        dt.date.fromisoformat(task['due'])
    rule = task.get('repeat') or {}
    if rule.get('unit'):
        unit, value = rule['unit'], rule.get('value', 1)
        if unit not in UNITS or not task.get('due'):
            raise ValueError('Repeating tasks need a due date and valid unit')
        if unit.startswith('days-of-'):
            maximum = 7 if unit.endswith('week') else 31
            if not all(1 <= int(v) <= maximum for v in str(value).split(',')):
                raise ValueError('Repeat days are out of range')
        elif not 1 <= int(value) <= 10000:
            raise ValueError('Repeat interval must be positive')
    if task.get('notifyBeforeDueLeadUnit') not in ('hours', 'days', 'weeks'):
        raise ValueError('Notification lead unit must be hours, days or weeks')
    if not 0 <= int(task.get('notifyBeforeDueLeadValue', 1)) <= 10000:
        raise ValueError('Invalid notification lead')


class Store:
    def __init__(self, directory):
        self.root = Path(directory).expanduser().resolve()
        repos = [p for p in Path(__file__).resolve().parents if (p / 'apps/runtjik/todo.py').is_file()]
        if any(self.root == repo or repo in self.root.parents for repo in repos):
            raise ValueError('Todo data must be outside the repository')
        self.root.mkdir(parents=True, exist_ok=True, mode=0o700)

    def request(self, request):
        with (self.root / '.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            return self.apply(request)

    def write(self, task):
        fd, name = tempfile.mkstemp(dir=self.root, prefix='.task-')
        try:
            with os.fdopen(fd, 'w') as handle:
                json.dump(task, handle, indent=2)
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(name, self.root / (task['id'] + '.json'))
            directory = os.open(self.root, os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        finally:
            if os.path.exists(name):
                os.unlink(name)

    def read(self):
        return [json.loads(p.read_text()) for p in sorted(self.root.glob('*.json'))]

    def apply(self, request):
        tasks = self.read()
        for task in tasks:
            lifecycle_defaults(task)
            task.setdefault('kind', 'todo')
            task.setdefault('reference', task.get('plan', ''))
        op = request.get('action', 'list')
        if op == 'capabilities':
            return {'reading': True, 'idempotent_create': True, 'agent_tasks': True, 'protocol': 1}
        if op == 'list':
            return tasks
        if op == 'create':
            key = request.get('request_id')
            if key:
                uuid.UUID(key)
                existing = next((t for t in tasks if t.get('createKey') == key), None)
                if existing:
                    return existing
            task = {'id': str(uuid.uuid4()), 'title': '', 'kind': 'todo', 'reference': '', 'createKey': key, 'group': 'All', 'tags': [], 'due': '',
                    'repeat': {}, 'plan': '', 'owner': 'tijptjik', 'notifyBeforeDue': True,
                    'notifyBeforeDueLeadValue': 1, 'notifyBeforeDueLeadUnit': 'days',
                    'snoozedUntil': None, 'createdAt': stamp(), 'updatedAt': stamp(),
                    'completedAt': None, 'completions': {}, 'notifications': {}, 'revision': 0}
        else:
            task = next((t for t in tasks if t['id'] == request.get('id')), None)
            if task is None:
                raise ValueError('Task no longer exists')
        lifecycle_defaults(task)
        period = request.get('period') or occurrence(task, today()) or 'once'
        if op in ('create', 'edit'):
            if op == 'edit' and request.get('revision') != task['revision']:
                raise ValueError('Task changed on another device; reopen the editor')
            allowed = {'kind', 'reference', 'title', 'group', 'tags', 'due', 'repeat', 'plan', 'notifyBeforeDue', 'notifyBeforeDueLeadValue', 'notifyBeforeDueLeadUnit', 'source', 'assignee', 'project', 'acceptance'}
            fields = {k: v for k, v in request.get('fields', {}).items() if k in allowed}
            if 'plan' in fields and 'reference' not in fields:
                fields['reference'] = fields['plan']
            task.update(fields)
            task['plan'] = task.get('reference', '')  # Compatibility with existing Todo clients.
            if task['group'] == 'Quick':
                task['due'] = today().isoformat()
            validate(task)
        elif op in ('status', 'verify'):
            if request.get('revision') != task['revision']:
                raise ValueError('Task changed; refresh before updating')
            actor = request.get('actor', '')
            if not isinstance(actor, str) or not actor.strip():
                raise ValueError('Actor is required')
            if op == 'status':
                status = request.get('status')
                if status not in ('inbox', 'ready', 'in_progress', 'blocked', 'done', 'cancelled'):
                    raise ValueError('Unknown task status')
                blockers = request.get('blockers', [])
                if not isinstance(blockers, list) or any(not isinstance(b, dict) or not b.get('reason') for b in blockers):
                    raise ValueError('Blockers must contain a reason and optional task_id')
                if status == 'blocked' and not blockers:
                    raise ValueError('Blocked tasks need a reason')
                for blocker in blockers:
                    if blocker.get('task_id') and (blocker['task_id'] == task['id'] or not any(t['id'] == blocker['task_id'] for t in tasks)):
                        raise ValueError('Invalid blocker task')
                task['status'], task['blockers'] = status, blockers
                if status == 'done':
                    task['completions'].setdefault(period, stamp())
                else:
                    task['completions'].pop(period, None)
                task['completedAt'] = max(task['completions'].values(), default=None)
            else:
                evidence = request.get('evidence')
                artifact = request.get('artifact')
                outcome = request.get('outcome')
                if not isinstance(evidence, str) or not evidence.strip() or not isinstance(artifact, str) or not artifact.strip():
                    raise ValueError('Verification needs evidence and an artifact or revision')
                if outcome not in ('passed', 'failed', 'pending'):
                    raise ValueError('Unknown verification outcome')
                task['verification'].append({'actor': actor, 'at': stamp(), 'outcome': outcome,
                    'evidence': evidence, 'artifact': artifact, 'taskRevision': task['revision'], 'period': period})
            task['events'].append({'action': op, 'actor': actor, 'at': stamp(),
                'status': task['status'], 'revision': task['revision'] + 1})
        elif op in ('toggle', 'done'):
            completions = task['completions']
            if op == 'toggle' and completions.get(period):
                del completions[period]
            else:
                completions.setdefault(period, stamp())
            task['completedAt'] = max(completions.values(), default=None)
            task['status'] = 'done' if completions.get(period) else 'ready'
        elif op == 'snooze':
            until = dt.datetime.fromisoformat(request['until'])
            if until.tzinfo is None or until <= dt.datetime.now(dt.timezone.utc):
                raise ValueError('Snooze must be a future date with timezone')
            task['snoozedUntil'] = until.isoformat()
            task['notifications'].pop(period + ':' + today().isoformat(), None)
        elif op in ('claim', 'dismiss', 'ack', 'release'):
            key = request.get('notification_key') if op in ('ack', 'release') else None
            key = key or period + ':' + today().isoformat()
            now = dt.datetime.now(dt.timezone.utc)
            previous = task['notifications'].get(key)
            if op == 'claim':
                if task['completions'].get(period) or task.get('status') == 'cancelled':
                    return False
                if previous:
                    if previous.get('state') != 'pending' or dt.datetime.fromisoformat(previous['expires']) > now:
                        return False
                if task.get('snoozedUntil') and dt.datetime.fromisoformat(task['snoozedUntil']) > now:
                    return False
                task['notifications'][key] = {'state': 'pending', 'at': stamp(),
                    'device': request.get('device', ''), 'token': str(uuid.uuid4()),
                    'expires': (now + dt.timedelta(minutes=2)).isoformat()}
            elif op in ('ack', 'release'):
                if not previous or previous.get('token') != request.get('token') or previous.get('device') != request.get('device', ''):
                    raise ValueError('Notification claim no longer belongs to this device')
                if op == 'ack':
                    previous['state'] = 'delivered'
                elif previous.get('state') == 'pending':
                    del task['notifications'][key]
            else:
                task['notifications'][key] = {'state': 'dismiss', 'at': stamp(), 'device': request.get('device', '')}
        else:
            raise ValueError('Unknown todo action')
        task['updatedAt'] = stamp()
        task['revision'] += 1
        self.write(task)
        return task


class Todo:
    name = 'Todo'
    commands = {'/todo': 'Todo app', '/reading': 'Reading list'}
    mode = 'app'
    icon = 'view-task'

    def __init__(self, demo=False):
        self.demo = demo
        self.temporary = tempfile.TemporaryDirectory(prefix='runtjik-todo-demo-') if demo else None

    def request(self, request, settings):
        options = {**config()['plugin_settings']['Todo'], **settings.get('plugin_settings', {}).get('Todo', {})}
        path = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'tasktjik/tasktjik.conf'
        if path.is_file():
            options.update(tomllib.loads(path.read_text()).get('tasktjik', {}))
        if self.demo:
            return Store(self.temporary.name).request(request)
        if options.get('url') or options.get('socket'):
            from client import api_request
            return api_request(options['url'], request, options.get('socket'))
        if options['authority']:
            host = options['authority']
            if not all(c.isalnum() or c in '.-@_' for c in host) or host.startswith('-'):
                raise ValueError('Invalid authority SSH host')
            transport = options['ssh_command']
            if not isinstance(transport, list) or not transport or not all(isinstance(arg, str) and arg for arg in transport):
                raise ValueError('Todo.ssh_command must be a nonempty argv array')
            result = subprocess.run([*transport, '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=4', host,
                                     'python3 .local/lib/tasktjik/tasktjik.py serve'],
                                    input=json.dumps({'directory': options['directory'], **request}), text=True,
                                    capture_output=True, timeout=10, check=True)
            response = json.loads(result.stdout)
            if 'error' in response:
                raise ValueError(response['error'])
            return response['result']
        return Store(options['directory']).request(request)

    def state(self, request, settings):
        if request.get('action') == 'open-reference':
            task = next((t for t in self.request({'action': 'list'}, settings) if t['id'] == request.get('id')), None)
            if not task or task.get('kind') != 'reading':
                raise ValueError('Select a reading item')
            validate(task)
            if not self.demo:
                subprocess.Popen(['xdg-open', task['reference']], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            return {'message': 'Opened reading reference'}
        if request.get('action', 'list') == 'plans':
            if self.demo:
                return {'plans': ['Projects/Synthetic plan.md', 'Home/Synthetic chores.md']}
            vault = config()['plugin_settings']['Todo'].get('vault', '') or settings.get('plugin_settings', {}).get('Todo', {}).get('vault', '')
            if not vault:
                return {'plans': [], 'message': 'Configure tasktjik.vault'}
            base = Path(vault).expanduser().resolve()
            from itertools import islice
            paths = list(islice((str(p.relative_to(base)) for p in base.rglob('*.md')
                                 if p.is_file() and base in p.resolve().parents), 10000))
            return {'plans': sorted(paths)}
        if request.get('action', 'list') != 'list':
            self.request(request, settings)
        rows = self.request({'action': 'list'}, settings)
        for task in rows:
            task['period'] = occurrence(task, today()) or 'once'
            task['done'] = bool(task['completions'].get(task['period']))
        groups = list(dict.fromkeys([*GROUPS, *sorted({task['group'] for task in rows}, key=str.casefold)]))
        return {'tasks': rows, 'groups': groups, 'today': today().isoformat()}

    def query(self, query, settings):
        return []


def main():
    if len(sys.argv) > 1 and sys.argv[1] in ('api', 'connect', 'devices', 'import-json', 'snapshot'):
        from api import command
        return command(sys.argv[1:])
    if len(sys.argv) > 1 and sys.argv[1] == 'serve':
        try:
            request = json.load(sys.stdin)
            result = Store(request.pop('directory', OPTIONS['directory'])).request(request)
            print(json.dumps({'result': result}))
        except Exception as exc:
            print(json.dumps({'error': str(exc)}))
        return
    provider, settings = Todo(), config()
    command = sys.argv[1] if len(sys.argv) > 1 else 'notify'
    if command in ('request', 'state'):
        request = json.load(sys.stdin)
        result = provider.state(request, settings) if command == 'state' else provider.request(request, settings)
        print(json.dumps(result))
        return
    if command in ('done', 'dismiss', 'snooze'):
        request = {'action': command, 'id': sys.argv[2], 'period': sys.argv[3]}
        if command == 'snooze':
            request['until'] = sys.argv[4]
        provider.request(request, settings)
        return
    if command != 'notify':
        raise ValueError('Expected notify, done, dismiss or snooze')
    from client import device_identity
    device = device_identity()['id']
    for task in provider.state({}, settings)['tasks']:
        if not task['due'] or task.get('status') == 'cancelled':
            continue
        if task['done']:
            if not task.get('repeat', {}).get('unit'):
                continue
            task['period'] = next_occurrence(task, task['period'])
        due = dt.datetime.combine(dt.date.fromisoformat(task['period']), dt.time(), dt.datetime.now().astimezone().tzinfo)
        lead = dt.timedelta(**{task['notifyBeforeDueLeadUnit']: int(task['notifyBeforeDueLeadValue'])}) if task['notifyBeforeDue'] else dt.timedelta()
        if dt.datetime.now().astimezone() < due - lead:
            continue
        claim = provider.request({'action': 'claim', 'id': task['id'], 'period': task['period'], 'device': device}, settings)
        if not claim:
            continue
        key, reservation = max(((k, v) for k, v in claim['notifications'].items()
            if v.get('state') == 'pending' and v.get('device') == device
            and k.startswith(task['period'] + ':')), key=lambda item: item[0])
        token = reservation['token']
        data = {'group': task['group'], 'due': task['period'], 'taskId': task['id'], 'period': task['period']}
        try:
            subprocess.run(['notjikctl', 'send', task['title'], '--app', 'tasktjik', '--type', 'todo', '--id', task['id'], '--timeout', 'never', '--data', json.dumps(data), '--actions', '[]'], check=True, timeout=10)
        except (OSError, subprocess.SubprocessError):
            provider.request({'action': 'release', 'id': task['id'], 'period': task['period'], 'device': device, 'token': token, 'notification_key': key}, settings)
            raise
        provider.request({'action': 'ack', 'id': task['id'], 'period': task['period'], 'device': device, 'token': token, 'notification_key': key}, settings)



if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
