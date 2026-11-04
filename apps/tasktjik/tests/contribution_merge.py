"""Synthetic coverage for contribution pairing and preservation."""
import copy
import importlib.util
from pathlib import Path
import unittest
import json
import os
import subprocess
import tempfile

spec = importlib.util.spec_from_file_location('merge_contributions', Path(__file__).parents[1] / 'scripts/merge_contributions.py')
merge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(merge)


def contribution(kind, short=False):
    receipt = merge.node('Contribution received')
    review = merge.node('Contribution review', 'flow', [merge.node('Verified'), merge.node('Needs revision')])
    compiled = merge.node('Contribution compiled')
    stages = [receipt, review, compiled]
    if not short:
        agreement = merge.node('Contribution agreement', 'flow', [merge.node('Agreed'), merge.node('Declined', 'exit'), merge.node('Follow up', 'reminder')])
        agreement['due'] = '2099-11-20'
        agreement['items'][2]['offset'] = 'one week before'
        stages = [merge.node('Instruction Sent'), agreement] + stages + [merge.node('Thank you sent')]
    return dict(id=kind, title='Synthetic guest', revision=3, project='wedding', kind='todo',
                group='Book of Memories' if kind == 'book' else 'Wedding Videos',
                metadata={'contactId': 'synthetic-contact', 'contributionType': kind}, subtasks=stages)


class MergeTests(unittest.TestCase):
    def test_receipts_reviews_and_join_preserve_each_path(self):
        book, video = contribution('book'), contribution('video')
        book['subtasks'][0]['done'] = True
        book['subtasks'][1]['items'][1]['done'] = True
        video['subtasks'][1]['items'][0]['done'] = True
        video['subtasks'][2]['done'] = True
        result = merge.merge(book, video)
        self.assertTrue(result['subtasks'][0]['done'])
        self.assertEqual(result['subtasks'][1]['items'][0]['title'], 'Agreed to a submission')
        self.assertFalse(result['subtasks'][1]['items'][1]['done'])
        receipt = result['subtasks'][2]
        self.assertTrue(receipt['items'][0]['cancelled'])
        self.assertTrue(receipt['items'][1]['done'])
        self.assertEqual(result['subtasks'][3]['requires'], [receipt['items'][0]['id']])
        self.assertEqual(result['subtasks'][4]['requires'], [receipt['items'][1]['id']])
        self.assertEqual(result['subtasks'][-1]['requires'], [result['subtasks'][5]['id'], result['subtasks'][6]['id']])
        self.assertTrue(result['subtasks'][-1]['acceptCancelled'])
        self.assertTrue(book['subtasks'][1]['items'][1]['done'])

    def test_short_flow_does_not_add_outreach(self):
        result = merge.merge(contribution('book', True), contribution('video', True))
        self.assertEqual(result['subtasks'][0]['title'], 'Contribution received')
        self.assertEqual(len(result['subtasks']), 6)

    def test_plan_edits_before_purging_and_retains_originals(self):
        snapshot = {'tasks': [contribution('book'), contribution('video')]}
        original = copy.deepcopy(snapshot)
        result = merge.plan(snapshot, 'wedding')
        self.assertEqual([r['action'] for r in result['requests']], ['edit', 'purge'])
        self.assertEqual(result['originals'], snapshot['tasks'])
        self.assertEqual(snapshot, original)
        with self.assertRaises(ValueError):
            merge.plan({'tasks': snapshot['tasks'][:1]}, 'wedding')
        with self.assertRaises(ValueError):
            merge.plan({'tasks': snapshot['tasks'] + snapshot['tasks'][:1]}, 'wedding')

    def test_apply_is_resumable_and_removes_only_empty_old_groups(self):
        cli = str(Path(__file__).parents[1] / 'target/release/tasktjik-api')
        with tempfile.TemporaryDirectory(prefix='tasktjik-merge-test-') as directory:
            root = Path(directory)
            (root / 'config/tasktjik').mkdir(parents=True)
            (root / 'config/tasktjik/tasktjik.conf').write_text('[tasktjik]\nauthority=""\ndirectory=' + json.dumps(str(root / 'tasks')) + '\n')
            env = {**os.environ, 'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'), 'XDG_STATE_HOME': str(root / 'state')}
            def call(value, command='request'):
                return json.loads(subprocess.check_output([cli, command], input=json.dumps(value), text=True, env=env))
            project = call({'action': 'create', 'fields': {'title': 'Synthetic wedding', 'kind': 'project'}})
            for index, kind in enumerate(['book', 'video']):
                source = contribution(kind)
                call({'action': 'group-add', 'target': source['group'], 'groupsRevision': index})
                fields = {k: source[k] for k in ['title', 'group', 'metadata', 'subtasks']}
                fields.update(project=project['id'], workflow=True)
                call({'action': 'create', 'fields': fields})
            snapshot = call({'action': 'list'}, 'state')
            prepared = merge.plan(snapshot, project['id'])
            output = root / 'plan.json'
            output.write_text(json.dumps(prepared))
            for _ in range(2):
                subprocess.run(['python3', '-B', str(Path(merge.__file__)), '--output', str(output), '--apply', '--cli', cli], env=env, check=True, capture_output=True)
            after = call({'action': 'list'}, 'state')
            self.assertEqual(len(after['tasks']), 2)
            guest = next(t for t in after['tasks'] if t['kind'] == 'todo')
            self.assertEqual(guest['group'], 'Contributions')
            self.assertNotIn('Book of Memories', after['groups'])
            self.assertNotIn('Wedding Videos', after['groups'])


if __name__ == '__main__':
    unittest.main()
