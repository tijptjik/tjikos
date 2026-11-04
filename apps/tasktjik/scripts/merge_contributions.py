#!/usr/bin/env python3
"""Prepare a revision-checked contribution merge from a private Tasktjik snapshot.

Never stores task records in the repository. Output is a private reviewable plan;
application requires an authority advertising conditional_pathways and --apply.
"""
import argparse
import copy
import json
import os
from pathlib import Path
import subprocess
import uuid


def node(title, kind='task', items=None):
    return dict(id=str(uuid.uuid4()), title=title, kind=kind, items=items or [], due='', done=False)


def merge(book, video):
    b, v = copy.deepcopy(book['subtasks']), copy.deepcopy(video['subtasks'])
    if len(b) != len(v) or len(b) not in (3, 6):
        raise ValueError('Contribution workflows have incompatible shapes')
    short = len(b) == 3
    offset = 0 if short else 2
    for items in (b, v):
        if [n['title'].lower() for n in items[offset:offset + 3]] != ['contribution received', 'contribution review', 'contribution compiled']:
            raise ValueError('Contribution workflow was customized; review manually')
    shared = []
    if not short:
        sent, agreement = b[:2]
        sent['done'] = sent['done'] or v[0]['done']
        sent['due'] = min(filter(None, [sent['due'], v[0]['due']]), default='')
        if [n['kind'] for n in agreement['items']] != ['task', 'exit', 'reminder'] or [n['kind'] for n in v[1]['items']] != ['task', 'exit', 'reminder']:
            raise ValueError('Unexpected agreement choices')
        agreed = agreement['items'][0]
        agreed['title'] = 'Agreed to a submission'
        agreed['done'] = agreed['done'] or v[1]['items'][0]['done']
        agreement['items'][1]['done'] = agreement['items'][1]['done'] and v[1]['items'][1]['done'] and not agreed['done']
        agreement['items'][2]['done'] = agreement['items'][2]['done'] or v[1]['items'][2]['done']
        agreement['due'] = min(filter(None, [agreement['due'], v[1]['due']]), default='')
        shared = [sent, agreement]
    received, reviews, compiled = [], [], []
    for source, items, title in [(book, b, 'Books of Memories'), (video, v, 'To a Lifetime Together')]:
        receipt, review, compilation = items[offset:offset + 3]
        receipt['title'] = title
        if source.get('status') == 'cancelled' or not short and source['subtasks'][1]['items'][1]['done']:
            receipt['cancelled'] = True
        review.update(title=title + ' - Contribution Review', requires=[receipt['id']])
        compilation.update(title=title + ' - Contribution Compiled', requires=[review['id']])
        received.append(receipt)
        reviews.append(review)
        compiled.append(compilation)
    thanks = node('Thank you sent') if short else b[5]
    thanks.update(requires=[n['id'] for n in compiled], acceptCancelled=True)
    if not short:
        thanks['done'] = b[5]['done'] and v[5]['done']
    metadata = copy.deepcopy(book.get('metadata', {}))
    metadata.update(contributionType='combined', contributionTypes=['book', 'video'],
                    contributionSources={kind:dict(id=task['id'], metadata=task.get('metadata', {}),
                                                   reference=task.get('reference', ''), due=task.get('due', ''))
                                         for kind, task in [('book', book), ('video', video)]})
    return dict(group='Contributions', workflow=True,
                subtasks=shared + [node('Contribution received', 'group', received)] + reviews + compiled + [thanks],
                metadata=metadata, tags=sorted(set(book.get('tags', []) + video.get('tags', []))))


def plan(snapshot, project_id):
    tasks = [t for t in snapshot['tasks'] if t.get('project') == project_id and t.get('group') in ('Book of Memories', 'Wedding Videos')]
    pairs = {}
    for task in tasks:
        key = task.get('metadata', {}).get('contactId') or task['title']
        group = task['group']
        if group in pairs.setdefault(key, {}):
            raise ValueError('Duplicate contributor identity in one category')
        pairs[key][group] = task
    edits, purges = [], []
    for pair in pairs.values():
        if set(pair) != {'Book of Memories', 'Wedding Videos'}:
            raise ValueError('Unpaired contribution; review manually')
        book, video = pair['Book of Memories'], pair['Wedding Videos']
        if book['title'] != video['title']:
            raise ValueError('Contributor identity has different names')
        if any(t.get('images') for t in (book, video)):
            raise ValueError('Attachments need manual preservation before merging')
        edits.append(dict(action='edit', id=book['id'], revision=book['revision'], fields=merge(book, video), request_id=str(uuid.uuid4())))
        purges.append(dict(action='purge', id=video['id'], revision=video['revision'], request_id=str(uuid.uuid4())))
    templates = {t['title']: t for t in snapshot['tasks'] if t.get('kind') == 'template'}
    if 'Book Submission' in templates and 'Video Submission' in templates:
        book, video = templates['Book Submission'], templates['Video Submission']
        fields = merge(book, video)
        edits.append(dict(action='edit', id=book['id'], revision=book['revision'],
                          fields=dict(title='Contribution Submission', subtasks=fields['subtasks'], workflow=True), request_id=str(uuid.uuid4())))
        purges.append(dict(action='purge', id=video['id'], revision=video['revision'], request_id=str(uuid.uuid4())))
    if 'Non-English Contribution (Sara)' in templates:
        template = templates['Non-English Contribution (Sara)']
        video = copy.deepcopy(template)
        def reidentify(nodes):
            for item in nodes:
                item['id'] = str(uuid.uuid4())
                reidentify(item['items'])
        reidentify(video['subtasks'])
        fields = merge(template, video)
        edits.append(dict(action='edit', id=template['id'], revision=template['revision'],
                          fields=dict(subtasks=fields['subtasks'], workflow=True), request_id=str(uuid.uuid4())))
    return dict(project=project_id, contributors=len(pairs), removeEmptyGroups=[dict(source=name, request_id=str(uuid.uuid4())) for name in ('Book of Memories', 'Wedding Videos')], originals=[t for t in snapshot['tasks'] if t['id'] in {r['id'] for r in edits + purges}], requests=edits + purges)


def request(value, cli):
    return json.loads(subprocess.check_output([cli, 'request'], input=json.dumps(value), text=True))


def apply(path, cli):
    data = json.loads(path.read_text())
    if not request({'action': 'capabilities'}, cli).get('conditional_pathways'):
        raise ValueError('Update the authority and clients before applying conditional pathways')
    # Persist acknowledged operations and reuse UUIDs after interrupted/lost responses.
    receipt = path.with_suffix('.receipts.json')
    completed = json.loads(receipt.read_text()) if receipt.exists() else []
    if not completed:
        current = request({'action': 'list'}, cli)
        current = {t['id']: t for t in current}
        for original in data['originals']:
            if current.get(original['id'], {}).get('revision') != original['revision']:
                raise ValueError('Tasks changed after planning; regenerate the plan')
    for operation in data['requests']:
        if operation['request_id'] in completed:
            continue
        request(operation, cli)
        completed.append(operation['request_id'])
        temporary = receipt.with_suffix('.tmp')
        temporary.write_text(json.dumps(completed))
        temporary.replace(receipt)
    for cleanup in data.get('removeEmptyGroups', []):
        current = request({'action': 'list'}, cli)
        if any(t.get('group') == cleanup['source'] for t in current):
            raise ValueError('Old category gained members; review before removing it')
        settings = next((t for t in current if t.get('kind') == 'settings'), {})
        if cleanup['source'] in settings.get('groups', []):
            request(dict(action='group-remove', source=cleanup['source'], members=[],
                         groupsRevision=settings['revision'], request_id=cleanup['request_id']), cli)
    print(f"Merged {data['contributors']} contribution pairs")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--snapshot', type=Path)
    parser.add_argument('--project')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--cli', default='tasktjik')
    args = parser.parse_args()
    os.umask(0o077)
    repo = Path(__file__).resolve().parents[3]
    if args.output.resolve().is_relative_to(repo):
        parser.error('Private plans must be stored outside the repository')
    if args.apply:
        apply(args.output, args.cli)
    else:
        if not args.snapshot or not args.project:
            parser.error('--snapshot and --project are required to prepare a plan')
        result = plan(json.loads(args.snapshot.read_text()), args.project)
        with args.output.open('x') as output:
            json.dump(result, output, indent=2)
        print(f"Prepared {result['contributors']} contributor merges; {len(result['requests'])} revision-checked operations")


if __name__ == '__main__':
    main()
