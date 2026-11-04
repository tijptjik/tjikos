#!/usr/bin/env python3
"""Incrementally record explicit WhatsApp contribution responses in Tasktjik.

The shared agreement workflow is updated without marking contributions received.
All message text, plans, checkpoints and reports stay in a private state directory.
"""
import argparse
import copy
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import uuid


def save(path, value):
    temporary = path.with_suffix('.tmp')
    with temporary.open('w') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)
    descriptor = os.open(path.parent, os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def call(cli, request):
    result = subprocess.run([cli, 'request'], input=json.dumps(request), text=True,
                            capture_output=True, timeout=90, check=True)
    value = json.loads(result.stdout)
    if isinstance(value, dict) and value.get('error'):
        raise ValueError(value['error'])
    return value


def normalize(text):
    return re.sub(r'\s+', ' ', text.lower().replace('’', "'")).strip()


def classify(text):
    """Deliberately narrow English rules; everything else goes to private review.

    Reject questions, uncertainty, mixed responses, quoted material and any
    medium-specific answer. Never infer agreement from gratitude or reactions.
    """
    text = normalize(text)
    # Planning the contents of a contribution presupposes participation. Keep
    # uncertainty about WHETHER to participate separate from choosing WHAT to send.
    planning = re.search(r"\b(?:lemme|let me|let us|(?:i'll|we'll)|(?:i|we) will) think (?:of|about) what to (?:include|contribute|send)\b", text)
    uncertainty_text = text[:planning.start()] + text[planning.end():] if planning else text
    if re.search(r'[?"“”\n>]|\b(maybe|might|perhaps|try|hopefully|if|but|however|not sure|think|can you|could you|said|says)\b', uncertainty_text):
        return None
    if re.search(r'\b(video|book|guestbook|photo|clip|entry)\b', text):
        return None
    declined = re.search(r"\b(?:i|we) (?:can't|cannot|won't|will not|(?:won't|will not) be able to|am unable to|are unable to|am not able to|are not able to) contribute\b", text)
    accepted = re.search(r"\b(?:(?:i|we) (?:will|can|would (?:absolutely |really )?love to|(?:am|are) (?:happy|keen) to)|(?:i'd|we'd) (?:absolutely |really )?love to|(?:i'll|we'll)) contribute\b", text)
    # Also accept an omitted subject in a direct, explicit reply to the invitation.
    accepted = accepted or re.search(r'(?:^|[^\w\s])\s*would (?:absolutely |really )?love to contribute\b', text) or planning
    if declined and not accepted:
        return 'Declined'
    if accepted and not declined and not re.search(r"\b(no|not|never|don't|won't|can't|cannot|unable)\b", text):
        return 'Accepted'
    return None


def identities(tasks, project):
    people, aliases = {}, {}
    for task in tasks:
        meta = task.get('metadata', {})
        if task.get('project') != project or meta.get('contributionType') not in ('book', 'video', 'combined'):
            continue
        identity = meta.get('contactId') or meta.get('contactPhoneNumber')
        if not identity:
            continue
        person = people.setdefault(identity, {'tasks': [], 'name': task['title']})
        person['tasks'].append(task)
        phone = meta.get('contactPhoneNumber', '')
        keys = [identity] if identity.endswith(('@lid', '@s.whatsapp.net')) else []
        if re.fullmatch(r'\+[1-9][0-9]{6,14}', phone):
            keys.append(phone[1:] + '@s.whatsapp.net')
        for key in keys:
            if key in aliases and aliases[key] != identity:
                raise ValueError('Ambiguous contributor identity; resolve duplicate contact mappings')
            aliases[key] = identity
    return people, aliases


def message_identity(row, aliases):
    if row['chat'].endswith('@g.us'):
        return None  # A group participant's agreement must never be assigned to the whole group.
    candidates = {aliases[row['chat']]} if row['chat'] in aliases else set()
    if not row.get('fromMe'):
        payload = json.loads(row.get('payload') or '{}')
        alt = payload.get('info', {}).get('source', {}).get('sender_alt') or {}
        if alt.get('server') == 's.whatsapp.net':
            key = alt['user'] + '@s.whatsapp.net'
            if key in aliases:
                candidates.add(aliases[key])
    if len(candidates) > 1:
        raise ValueError('Message identity disagrees with Tasktjik contact mapping')
    return next(iter(candidates), None)


def forwarded(row):
    def walk(value):
        if isinstance(value, dict):
            if value.get('is_forwarded') or value.get('forwarding_score', 0):
                return True
            return any(walk(v) for v in value.values())
        if isinstance(value, list):
            return any(walk(v) for v in value)
        return False
    return walk(json.loads(row.get('payload') or '{}').get('message', {}))


def read_messages(database, state):
    with sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
        connection.execute('BEGIN')
        maximum = connection.execute('SELECT coalesce(max(seq),0) FROM whatsover_inbox').fetchone()[0]
        cursor = state.get('lastSequence', 0)
        if maximum < cursor:
            raise ValueError('Inbox was reset; checkpoint cannot be reused')
        if cursor:
            previous = connection.execute('SELECT data FROM whatsover_inbox WHERE seq=?', (cursor,)).fetchone()
            if not previous or hashlib.sha256(previous[0].encode()).hexdigest() != state['boundaryHash']:
                raise ValueError('Inbox checkpoint identity changed; review before resetting state')
        rows = []
        boundary = state.get('boundaryHash')
        for sequence, raw in connection.execute('SELECT seq,data FROM whatsover_inbox WHERE seq>? AND seq<=? ORDER BY seq', (cursor, maximum)):
            row = json.loads(raw)
            row['sequence'] = sequence
            rows.append(row)
            boundary = hashlib.sha256(raw.encode()).hexdigest()
    return rows, maximum, boundary


def response_fields(task, metadata):
    fields = {'metadata': metadata}
    if metadata.get('contributionType') != 'combined' or not task.get('workflow'):
        return fields
    nodes = copy.deepcopy(task['subtasks'])
    agreements = [n for n in nodes if n['title'].lower() == 'contribution agreement']
    if len(agreements) != 1:
        raise ValueError('Combined contribution agreement workflow needs review')
    choices = agreements[0]['items']
    accepted = [n for n in choices if n['title'].lower() in ('agreed to a submission', 'accepted') and n['kind'] == 'task']
    declined = [n for n in choices if n['title'].lower() == 'declined' and n['kind'] == 'exit']
    if len(accepted) != 1 or len(declined) != 1:
        raise ValueError('Contribution agreement choices need review')
    accepted[0]['done'] = metadata['contributionResponse'] == 'Accepted'
    declined[0]['done'] = metadata['contributionResponse'] == 'Declined'
    outreach = metadata.get('contributionOutreach', {})
    requests = outreach.get('requestIds', [])
    if requests and outreach.get('serverAcceptedMessages', 0) >= len(requests):
        for node in nodes:
            if node['title'].lower() == 'instruction sent' and node['kind'] == 'task':
                node['done'] = True
    metadata['contributionResponseEvidence']['workflowApplied'] = True
    fields['subtasks'] = nodes
    return fields


def prepare(tasks, rows, state, project):
    people, aliases = identities(tasks, project)
    next_state = copy.deepcopy(state)
    conversations = next_state.setdefault('conversations', {})
    decisions, reviews = {}, []
    for row in rows:
        identity = message_identity(row, aliases)
        if not identity:
            continue
        context = conversations.setdefault(identity, {})
        text = row.get('text') or ''
        stamp = row['timestamp']
        if row.get('fromMe'):
            normalized = normalize(text)
            if 'demo' in normalized:
                context['demo'] = True
            if ('tom' in normalized and 'martina' in normalized
                    and ('contribut' in normalized or 'keepsake' in normalized)):
                context['invitationAt'] = max(stamp, context.get('invitationAt', 0))
            continue
        if not text:
            continue
        eligible = (not context.get('demo') and context.get('invitationAt')
                    and 0 <= stamp - context['invitationAt'] <= 14 * 86400000)
        status = classify(text) if eligible and not forwarded(row) else None
        evidence = {'status': status, 'messageId': row['id'], 'chat': row['chat'],
                    'timestamp': stamp, 'sequence': row['sequence'], 'text': text,
                    'scope': 'general contribution; medium unspecified'}
        # Server arrival order can differ from message time. Older replies cannot
        # replace a later decision, even when they arrive in a subsequent run.
        latest = max(context.get('decisionAt', 0), decisions.get(identity, {}).get('timestamp', 0))
        if status and stamp > latest:
            decisions[identity] = evidence
        else:
            reviews.append({'name': people[identity]['name'], **evidence,
                            'reason': 'Unclear, unsupported, missing invitation context, or older reply'})
        context['lastProcessedMessageId'] = row['id']
        context['lastProcessedTimestamp'] = max(stamp, context.get('lastProcessedTimestamp', 0))
    operations, changes = [], []
    for identity, evidence in decisions.items():
        person = people[identity]
        conversations[identity]['decisionAt'] = evidence['timestamp']
        for task in person['tasks']:
            metadata = copy.deepcopy(task['metadata'])
            previous = metadata.get('contributionResponseEvidence', {})
            if previous.get('timestamp', 0) >= evidence['timestamp']:
                continue
            if (metadata.get('contributionResponse')
                    and metadata.get('contributionResponse') != evidence['status']
                    and previous.get('source') != 'whatsapp-response-sync'):
                reviews.append({'name': person['name'], **evidence, 'reason': 'Preserved manually recorded response'})
                continue
            metadata['contributionResponse'] = evidence['status']
            metadata['contributionResponseEvidence'] = {
                **{k: v for k, v in evidence.items() if k not in ('text', 'status')},
                'source': 'whatsapp-response-sync', 'ruleVersion': 2}
            operations.append({'action': 'edit', 'id': task['id'], 'revision': task['revision'],
                               'request_id': str(uuid.uuid4()), 'fields': response_fields(task, metadata)})
            changes.append({'taskId': task['id'], 'name': person['name'], 'group': task.get('group'),
                            'before': task['metadata'].get('contributionResponse'), **evidence})
    # One-time repair of responses recorded before the shared workflow existed.
    # Once projected, never undo subsequent manual workflow edits on an idle run.
    changed_ids = {op['id'] for op in operations}
    for person in people.values():
        for task in person['tasks']:
            metadata = copy.deepcopy(task['metadata'])
            evidence = metadata.get('contributionResponseEvidence', {})
            if (task['id'] in changed_ids or metadata.get('contributionType') != 'combined'
                    or not task.get('workflow') or evidence.get('workflowApplied')
                    or evidence.get('source') != 'whatsapp-response-sync'
                    or metadata.get('contributionResponse') not in ('Accepted', 'Declined')):
                continue
            operations.append({'action': 'edit', 'id': task['id'], 'revision': task['revision'],
                               'request_id': str(uuid.uuid4()), 'fields': response_fields(task, metadata)})
            changes.append({'taskId': task['id'], 'name': person['name'], 'group': task.get('group'),
                            'status': metadata['contributionResponse'], 'reason': 'Apply recorded response to shared agreement workflow'})
    return next_state, operations, changes, reviews


def finish_pending(directory, cli):
    path = directory / 'pending.json'
    if not path.exists():
        return
    pending = json.loads(path.read_text())
    for operation in pending['operations']:
        # Replay the exact UUID and revision after an interrupted/lost response.
        call(cli, operation)
    current = {t['id']: t for t in call(cli, {'action': 'list'})}
    for operation in pending['operations']:
        actual = current.get(operation['id'], {}).get('metadata', {})
        expected = operation['fields']['metadata']
        for field in ('contributionResponse', 'contributionResponseEvidence'):
            if actual.get(field) != expected[field]:
                raise ValueError('Response read-back differs; pending checkpoint preserved')
        if 'subtasks' in operation['fields'] and current[operation['id']].get('subtasks') != operation['fields']['subtasks']:
            raise ValueError('Workflow read-back differs; pending checkpoint preserved')
    archive = directory / 'runs'
    archive.mkdir(mode=0o700, exist_ok=True)
    save(archive / (pending['runId'] + '.json'), pending)
    save(directory / 'state.json', pending['state'])
    save(directory / 'last-report.json', {k: v for k, v in pending.items() if k not in ('operations', 'state')})
    path.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', required=True)
    parser.add_argument('--state-dir', required=True, type=Path)
    parser.add_argument('--database', type=Path, default=Path(os.environ.get('WHATSOVER_RUST_STATE_DIR',
                        str(Path(os.environ.get('XDG_STATE_HOME', str(Path.home() / '.local/state'))) / 'whatsover-rust'))) / 'session.sqlite')
    parser.add_argument('--tasktjik', default='tasktjik')
    parser.add_argument('--whatsover', default='whatsover')
    parser.add_argument('--observe-seconds', type=int, default=0)
    parser.add_argument('--reconsider-message', help='Re-evaluate one previously processed message ID without moving the inbox checkpoint')
    parser.add_argument('--apply', action='store_true', help='Default is a private dry-run report without checkpoint advancement')
    args = parser.parse_args()
    os.umask(0o077)
    directory = args.state_dir.resolve()
    if any((parent / '.git').is_file() or (parent / '.git/HEAD').is_file()
           for parent in [directory, *directory.parents]):
        parser.error('State must be outside a Git repository')
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    if directory.stat().st_mode & 0o077:
        parser.error('State directory must have mode 0700')
    with (directory / 'sync.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state_path = directory / 'state.json'
        state = json.loads(state_path.read_text()) if state_path.exists() else {
            'version': 1, 'project': args.project, 'database': str(args.database.resolve()), 'lastSequence': 0}
        if state['project'] != args.project or state['database'] != str(args.database.resolve()):
            raise ValueError('Checkpoint belongs to another project or inbox')
        if (directory / 'pending.json').exists():
            pending_state = json.loads((directory / 'pending.json').read_text())['state']
            if pending_state['project'] != args.project or pending_state['database'] != str(args.database.resolve()):
                raise ValueError('Pending apply belongs to another project or inbox')
            if not args.apply:
                raise ValueError('Pending apply exists; resume it before planning another run')
            finish_pending(directory, args.tasktjik)
            state = json.loads(state_path.read_text())
        if args.observe_seconds:
            # This same advisory jobs lock is documented for other Whatsover jobs.
            with (args.database.parent / 'jobs.lock').open('a') as jobs:
                fcntl.flock(jobs, fcntl.LOCK_EX | fcntl.LOCK_NB)
                subprocess.run([args.whatsover, '--state-dir', str(args.database.parent),
                                'observe', '--seconds', str(args.observe_seconds)],
                               capture_output=True, check=True, timeout=args.observe_seconds + 240)
        tasks = call(args.tasktjik, {'action': 'list'})
        if not any(t.get('id') == args.project and t.get('kind') == 'project' for t in tasks):
            raise ValueError('Target project does not exist')
        rows, maximum, boundary = read_messages(args.database, state)
        if args.reconsider_message:
            maximum, boundary = state['lastSequence'], state.get('boundaryHash')
            with sqlite3.connect(args.database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
                matches = connection.execute(
                    'SELECT seq,data FROM whatsover_inbox WHERE message_id=? AND seq<=?',
                    (args.reconsider_message, maximum)).fetchall()
            if len(matches) != 1:
                raise ValueError('Reconsideration requires exactly one previously processed message')
            sequence, raw = matches[0]
            rows = [dict(json.loads(raw), sequence=sequence)]
        next_state, operations, changes, reviews = prepare(tasks, rows, state, args.project)
        next_state.update(lastSequence=maximum, boundaryHash=boundary,
                          lastRunAt=datetime.now(timezone.utc).isoformat())
        pending = {'runId': str(uuid.uuid4()), 'state': next_state, 'operations': operations,
                   'changes': changes, 'review': reviews,
                   'reconsideredMessage': args.reconsider_message,
                   'newMessages': 0 if args.reconsider_message else len(rows),
                   'lastSequence': maximum, 'at': next_state['lastRunAt']}
        if args.apply:
            save(directory / 'pending.json', pending)
            finish_pending(directory, args.tasktjik)
        else:
            save(directory / 'dry-run.json', pending)
        print(json.dumps({'applied': args.apply, 'newMessages': pending['newMessages'], 'taskChanges': len(changes),
                          'reconsideredMessage': args.reconsider_message,
                          'reviewMessages': len(reviews), 'lastSequence': maximum,
                          'report': str(directory / ('last-report.json' if args.apply else 'dry-run.json'))}))


if __name__ == '__main__':
    main()
