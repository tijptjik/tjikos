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

GROUPS = ['Quick', 'General', 'Work', 'Socials', 'Domestic', 'Finances', 'Tech']
OPTIONS = {'authority': 'si', 'directory': '~/.local/share/runtjik-todo', 'vault': ''}
UNITS = ['days', 'weeks', 'months', 'years', 'days-of-the-week', 'days-of-the-month']


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
    if task.get('group') not in GROUPS:
        raise ValueError('Unknown group')
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

    def apply(self, request):
        tasks = [json.loads(p.read_text()) for p in sorted(self.root.glob('*.json'))]
        op = request.get('action', 'list')
        if op == 'list':
            return tasks
        if op == 'create':
            task = {'id': str(uuid.uuid4()), 'title': '', 'group': 'General', 'tags': [], 'due': '',
                    'repeat': {}, 'plan': '', 'owner': 'tijptjik', 'notifyBeforeDue': True,
                    'notifyBeforeDueLeadValue': 1, 'notifyBeforeDueLeadUnit': 'days',
                    'snoozedUntil': None, 'createdAt': stamp(), 'updatedAt': stamp(),
                    'completedAt': None, 'completions': {}, 'notifications': {}, 'revision': 0}
        else:
            task = next((t for t in tasks if t['id'] == request.get('id')), None)
            if task is None:
                raise ValueError('Task no longer exists')
        period = request.get('period') or occurrence(task, today()) or 'once'
        if op in ('create', 'edit'):
            if op == 'edit' and request.get('revision') != task['revision']:
                raise ValueError('Task changed on another device; reopen the editor')
            allowed = {'title', 'group', 'tags', 'due', 'repeat', 'plan', 'notifyBeforeDue', 'notifyBeforeDueLeadValue', 'notifyBeforeDueLeadUnit'}
            task.update({k: v for k, v in request.get('fields', {}).items() if k in allowed})
            if task['group'] == 'Quick':
                task['due'] = today().isoformat()
            validate(task)
        elif op in ('toggle', 'done'):
            completions = task['completions']
            if op == 'toggle' and completions.get(period):
                del completions[period]
            else:
                completions.setdefault(period, stamp())
            task['completedAt'] = max(completions.values(), default=None)
        elif op == 'snooze':
            until = dt.datetime.fromisoformat(request['until'])
            if until.tzinfo is None or until <= dt.datetime.now(dt.timezone.utc):
                raise ValueError('Snooze must be a future date with timezone')
            task['snoozedUntil'] = until.isoformat()
            task['notifications'].pop(period + ':' + today().isoformat(), None)
        elif op in ('claim', 'dismiss'):
            key = period + ':' + today().isoformat()
            if op == 'claim':
                if task['notifications'].get(key) or task['completions'].get(period):
                    return False
                if task.get('snoozedUntil') and dt.datetime.fromisoformat(task['snoozedUntil']) > dt.datetime.now(dt.timezone.utc):
                    return False
            task['notifications'][key] = {'state': op, 'at': stamp(), 'device': request.get('device', '')}
        else:
            raise ValueError('Unknown todo action')
        task['updatedAt'] = stamp()
        task['revision'] += 1
        self.write(task)
        return task


class Todo:
    name = 'Todo'
    commands = {'/todo': 'Todo app'}
    mode = 'app'
    icon = 'view-task'

    def __init__(self, demo=False):
        self.demo = demo
        self.temporary = tempfile.TemporaryDirectory(prefix='runtjik-todo-demo-') if demo else None

    def request(self, request, settings):
        options = {**OPTIONS, **settings.get('plugin_settings', {}).get('Todo', {})}
        if self.demo:
            return Store(self.temporary.name).request(request)
        if options['authority']:
            host = options['authority']
            if not all(c.isalnum() or c in '.-@_' for c in host) or host.startswith('-'):
                raise ValueError('Invalid authority SSH host')
            result = subprocess.run(['ssh', '-oBatchMode=yes', '-oConnectTimeout=4', host,
                                     'python3 .local/lib/runtjik/todo.py serve'],
                                    input=json.dumps({'directory': options['directory'], **request}), text=True,
                                    capture_output=True, timeout=10, check=True)
            response = json.loads(result.stdout)
            if 'error' in response:
                raise ValueError(response['error'])
            return response['result']
        return Store(options['directory']).request(request)

    def state(self, request, settings):
        if request.get('action', 'list') == 'plans':
            if self.demo:
                return {'plans': ['Projects/Synthetic plan.md', 'Home/Synthetic chores.md']}
            vault = settings['plugin_settings']['Todo'].get('vault', '')
            if not vault:
                return {'plans': [], 'message': 'Configure plugin_settings.Todo.vault'}
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
        return {'tasks': rows, 'groups': GROUPS, 'today': today().isoformat()}

    def query(self, query, settings):
        return []


def main():
    if len(sys.argv) > 1 and sys.argv[1] == 'serve':
        try:
            request = json.load(sys.stdin)
            result = Store(request.pop('directory', OPTIONS['directory'])).request(request)
            print(json.dumps({'result': result}))
        except Exception as exc:
            print(json.dumps({'error': str(exc)}))
        return
    from runtjik import config
    provider, settings = Todo(), config()
    command = sys.argv[1] if len(sys.argv) > 1 else 'notify'
    if command in ('done', 'dismiss', 'snooze'):
        request = {'action': command, 'id': sys.argv[2], 'period': sys.argv[3]}
        if command == 'snooze':
            request['until'] = sys.argv[4]
        provider.request(request, settings)
        return
    if command != 'notify':
        raise ValueError('Expected notify, done, dismiss or snooze')
    import socket
    for task in provider.state({}, settings)['tasks']:
        if not task['due']:
            continue
        if task['done']:
            if not task.get('repeat', {}).get('unit'):
                continue
            task['period'] = next_occurrence(task, task['period'])
        due = dt.datetime.combine(dt.date.fromisoformat(task['period']), dt.time(), dt.datetime.now().astimezone().tzinfo)
        lead = dt.timedelta(**{task['notifyBeforeDueLeadUnit']: int(task['notifyBeforeDueLeadValue'])}) if task['notifyBeforeDue'] else dt.timedelta()
        if dt.datetime.now().astimezone() < due - lead:
            continue
        if not provider.request({'action': 'claim', 'id': task['id'], 'period': task['period'], 'device': socket.gethostname()}, settings):
            continue
        argv = [sys.executable, str(Path(__file__).resolve())]
        data = {'group': task['group'], 'due': task['period'], 'todoCommand': argv, 'taskId': task['id'], 'period': task['period']}
        subprocess.run(['notjikctl', 'send', task['title'], '--app', 'runtjik-todo', '--type', 'todo', '--id', task['id'], '--timeout', 'never', '--data', json.dumps(data), '--actions', '[]'], check=True, timeout=10)


if __name__ == '__main__':
    main()
