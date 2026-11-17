import importlib.util
import io
import json
from pathlib import Path
import struct
import subprocess
import unittest
from unittest.mock import patch

path = Path(__file__).resolve().parents[1] / 'host/readtjik.py'
spec = importlib.util.spec_from_file_location('readtjik', path)
host = importlib.util.module_from_spec(spec)
spec.loader.exec_module(host)


class HostTests(unittest.TestCase):
    def test_rejects_nonreading_operations(self):
        for message in ({'op': 'delete'}, {'op': 'save', 'fields': {'kind': 'todo'}},
                        {'op': 'save', 'fields': {'kind': 'reading', 'reference': 'file:///etc/passwd'}}):
            with self.assertRaises(ValueError):
                host.request(message)

    def test_framing_errors(self):
        with self.assertRaises(ValueError):
            host.serve(io.BytesIO(struct.pack('@I', host.MAX_MESSAGE + 1)), io.BytesIO())
        body = b'{"op":"delete"}'
        output = io.BytesIO()
        host.serve(io.BytesIO(struct.pack('@I', len(body)) + body), output)
        response = output.getvalue()
        self.assertEqual(struct.unpack('@I', response[:4])[0], len(response[4:]))
        self.assertIn('error', json.loads(response[4:]))

    @patch.object(Path, 'is_file', return_value=True)
    @patch.object(subprocess, 'run')
    def test_options_exclude_unrelated_tasks(self, run, _):
        run.return_value = subprocess.CompletedProcess([], 0, json.dumps({'groups': ['All'], 'tasks': [
            {'kind': 'todo', 'title': 'private task'},
            {'id': 't', 'kind': 'template', 'title': 'Reading', 'subtasks': [], 'metadata': {'private': True}}
        ]}), '')
        result = host.request({'op': 'options'})
        self.assertEqual(len(result['items']), 1)
        self.assertNotIn('metadata', result['items'][0])
        self.assertEqual(run.call_args.args[0][-1], 'state')

    @patch.object(Path, 'is_file', return_value=True)
    @patch.object(subprocess, 'run')
    def test_save_preserves_id_and_requires_ack(self, run, _):
        run.return_value = subprocess.CompletedProcess([], 0, '{"id":"saved"}', '')
        message = {'op': 'save', 'request_id': 'fdebd655-f41d-4c3f-98d5-a086a9c886f1',
                   'fields': {'kind': 'reading', 'title': 'Test', 'reference': 'https://example.test'}}
        self.assertTrue(host.request(message)['saved'])
        request = json.loads(run.call_args.kwargs['input'])
        self.assertEqual(request['request_id'], message['request_id'])
        self.assertEqual(request['action'], 'create')
        run.return_value = subprocess.CompletedProcess([], 0, '{}', '')
        with self.assertRaisesRegex(ValueError, 'acknowledge'):
            host.request(message)


class ModelIntegration(unittest.TestCase):
    @unittest.skipUnless((Path.home() / '.local/lib/tasktjik/tasktjik-api').is_file(), 'Tasktjik is not installed')
    def test_real_model_with_synthetic_fields_only(self):
        actual_run = subprocess.run
        calls = []

        def synthetic(argv, **kwargs):
            calls.append(argv)
            return actual_run(argv if '--demo' in argv else [*argv, '--demo'], **kwargs)

        message = {'op': 'save', 'request_id': 'fdebd655-f41d-4c3f-98d5-a086a9c886f1', 'fields': {
            'kind': 'reading', 'title': 'Synthetic Readtjik fixture', 'reference': 'https://example.test/read',
            'group': 'All', 'tags': ['fixture'], 'due': '2026-12-01', 'repeat': {'unit': 'weeks', 'value': 2},
            'notifyBeforeDue': True, 'notifyBeforeDueLeadValue': 2, 'notifyBeforeDueLeadUnit': 'days',
            'plan': 'synthetic.md', 'assignee': 'fixture', 'acceptance': ['Read the page'],
            'workflow': True, 'onlyInProject': True, 'subtasks': [
                {'id': 'synthetic-step', 'kind': 'task', 'title': 'Take notes', 'due': '', 'done': False, 'items': []}
            ]}}
        with patch.object(subprocess, 'run', side_effect=synthetic):
            self.assertTrue(host.request(message)['saved'])
            self.assertEqual(len(calls), 2)
            calls.clear()
            message['fields']['title'] = ''
            with self.assertRaises(host.Rejected):
                host.request(message)
            self.assertEqual(len(calls), 1, 'Invalid fields must never reach the authority')


unittest.main()
