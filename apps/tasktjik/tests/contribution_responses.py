#!/usr/bin/env python3
"""Synthetic regression tests; never use a linked account or real tasks."""
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('responses', Path(__file__).parents[1] / 'scripts/sync_contribution_responses.py')
sync = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sync)


def task():
    return {'id': 'task', 'project': 'project', 'title': 'Synthetic Guest', 'revision': 2,
            'metadata': {'contactId': 'contact', 'contactPhoneNumber': '+15555550100',
                         'contributionType': 'combined', 'unrelated': 'preserve'}}


def message(sequence, text, mine=False, stamp=None):
    return {'sequence': sequence, 'id': str(sequence), 'timestamp': stamp or sequence * 1000,
            'chat': '15555550100@s.whatsapp.net' if mine else '123@lid', 'fromMe': mine,
            'text': text, 'payload': json.dumps({'info': {'source': {
                'sender_alt': {'user': '15555550100', 'server': 's.whatsapp.net'}}}})}


class Responses(unittest.TestCase):
    def test_existing_response_updates_visible_shared_workflow_once(self):
        t = task()
        t['workflow'] = True
        t['subtasks'] = [
            {'id': 'sent', 'title': 'Instruction Sent', 'kind': 'task', 'done': False, 'items': []},
            {'id': 'agreement', 'title': 'Contribution agreement', 'kind': 'flow', 'done': False, 'items': [
                {'id': 'yes', 'title': 'Agreed to a submission', 'kind': 'task', 'done': False, 'items': []},
                {'id': 'no', 'title': 'Declined', 'kind': 'exit', 'done': False, 'items': []}]},
            {'id': 'received', 'title': 'Contribution received', 'kind': 'task', 'done': False, 'items': []}]
        t['metadata'].update(contributionResponse='Accepted',
                             contributionResponseEvidence={'source': 'whatsapp-response-sync'},
                             contributionOutreach={'requestIds': ['one'], 'serverAcceptedMessages': 1})
        operations = sync.prepare([t], [], {}, 'project')[1]
        self.assertEqual(len(operations), 1)
        fields = operations[0]['fields']
        self.assertTrue(fields['subtasks'][0]['done'])
        self.assertTrue(fields['subtasks'][1]['items'][0]['done'])
        self.assertFalse(fields['subtasks'][1]['items'][1]['done'])
        self.assertFalse(fields['subtasks'][2]['done'])
        t.update(fields)
        self.assertFalse(sync.prepare([t], [], {}, 'project')[1])
        t['metadata']['contributionResponse'] = 'Declined'
        changed = sync.response_fields(t, t['metadata'])['subtasks']
        self.assertFalse(changed[1]['items'][0]['done'])
        self.assertTrue(changed[1]['items'][1]['done'])

    def test_explicit_rules_and_abstentions(self):
        for text in ['I can contribute', "I'd absolutely love to contribute", 'Would absolutely love to contribute to this. I have noted the deadline', 'Lovely to hear from you 🫶🏽 would absolutely love to contribute to this']:
            self.assertEqual(sync.classify(text), 'Accepted', text)
        self.assertEqual(sync.classify("Sorry, I can't contribute this time"), 'Declined')
        self.assertEqual(sync.classify("I won't be able to contribute"), 'Declined')
        for text in ['Thanks!', '👍', 'Best gift ever',
                     'I can contribute?', 'Maybe I can contribute', "I can't contribute a video but I can contribute a book",
                     'She said I can contribute', 'She would love to contribute', 'I can contribute if I have time', 'I can contribute a video',
                     'I would not love to contribute', 'I can contribute but not before the deadline']:
            self.assertIsNone(sync.classify(text), text)

    def test_planning_content_is_acceptance_but_deciding_participation_is_not(self):
        for text in ["Let me think about what to include", "I'll think of what to send", 'Lemme think of what to contribute']:
            self.assertEqual(sync.classify(text), 'Accepted')
            rows = [message(1, 'Tom and Martina keepsakes', True), message(2, text)]
            self.assertEqual(sync.prepare([task()], rows, {}, 'project')[2][0]['status'], 'Accepted')
        for text in ['Let me think about whether to contribute', 'I will think about it',
                     'Maybe let me think about what to include',
                     "Let me think about what to include, but I can't promise anything",
                     'Let me think about what to include in the video']:
            self.assertIsNone(sync.classify(text), text)

    def test_identity_context_and_preservation(self):
        invitation = message(1, "Keepsakes for Tom and Martina; please contribute", True)
        reply = message(2, 'I can contribute')
        state, operations, changes, reviews = sync.prepare([task()], [invitation, reply], {}, 'project')
        self.assertEqual(len(operations), 1)
        self.assertEqual(operations[0]['fields']['metadata']['unrelated'], 'preserve')
        self.assertEqual(set(operations[0]['fields']), {'metadata'})
        self.assertEqual(changes[0]['status'], 'Accepted')
        self.assertFalse(reviews)
        self.assertEqual(state['conversations']['contact']['lastProcessedMessageId'], '2')
        self.assertFalse(sync.prepare([task()], [reply], {}, 'project')[1])
        reply['chat'] = '123@g.us'
        self.assertFalse(sync.prepare([task()], [invitation, reply], {}, 'project')[1])

    def test_late_arrival_does_not_reverse_newer_decision(self):
        rows = [message(1, 'Tom and Martina keepsakes', True),
                message(2, 'I can contribute', stamp=5000)]
        state, *_ = sync.prepare([task()], rows, {}, 'project')
        self.assertFalse(sync.prepare([task()], [message(3, "I can't contribute", stamp=4000)], state, 'project')[1])
        self.assertEqual(sync.prepare([task()], [message(4, "I can't contribute", stamp=6000)], state, 'project')[2][0]['status'], 'Declined')

    def test_manual_response_and_forward_are_preserved(self):
        t = task()
        t['metadata']['contributionResponse'] = 'Declined'
        rows = [message(1, 'Tom and Martina keepsakes', True), message(2, 'I can contribute')]
        self.assertFalse(sync.prepare([t], rows, {}, 'project')[1])
        payload = json.loads(rows[1]['payload'])
        payload['message'] = {'extended_text_message': {'context_info': {'is_forwarded': True}}}
        rows[1]['payload'] = json.dumps(payload)
        self.assertFalse(sync.prepare([task()], rows, {}, 'project')[1])

    def test_sequence_checkpoint_and_reset_detection(self):
        with tempfile.TemporaryDirectory() as temporary:
            db = Path(temporary) / 'inbox.sqlite'
            with sqlite3.connect(db) as conn:
                conn.execute('CREATE TABLE whatsover_inbox(seq INTEGER PRIMARY KEY, data TEXT)')
                conn.execute('INSERT INTO whatsover_inbox VALUES(1,?)', (json.dumps(message(1, 'test')),))
            rows, sequence, boundary = sync.read_messages(db, {})
            self.assertEqual(len(rows), 1)
            state = {'lastSequence': sequence, 'boundaryHash': boundary}
            self.assertEqual(sync.read_messages(db, state)[0], [])
            with sqlite3.connect(db) as conn:
                conn.execute('UPDATE whatsover_inbox SET data=?', (json.dumps(message(1, 'replacement')),))
            with self.assertRaises(ValueError):
                sync.read_messages(db, state)

    def test_lost_reply_replays_identical_request_before_checkpoint(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            operation = {'id': 'task', 'revision': 2, 'request_id': 'fixed-id', 'fields': {'metadata': {
                'contributionResponse': 'Accepted', 'contributionResponseEvidence': {'messageId': 'reply'}}}}
            pending = {'runId': 'run', 'operations': [operation], 'state': {'lastSequence': 9}}
            sync.save(root / 'pending.json', pending)
            with patch.object(sync, 'call', side_effect=TimeoutError):
                with self.assertRaises(TimeoutError):
                    sync.finish_pending(root, 'fake')
            self.assertFalse((root / 'state.json').exists())
            with patch.object(sync, 'call', side_effect=[{}, [{'id': 'task', **operation['fields']}]]) as request:
                sync.finish_pending(root, 'fake')
                self.assertEqual(request.call_args_list[0].args[1], operation)
            self.assertEqual(json.loads((root / 'state.json').read_text())['lastSequence'], 9)
            self.assertFalse((root / 'pending.json').exists())


if __name__ == '__main__':
    unittest.main()
