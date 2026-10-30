import sys
import tempfile
import unittest
import os
from unittest.mock import patch
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tasktjik import Store, Todo


class AgentTasks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(self.temp.name)
        self.task = self.store.request({'action': 'create', 'fields': {'title': 'Synthetic agent task', 'source': {'service': 'agent'}, 'acceptance': ['Pass checks']}})

    def update(self, action, **fields):
        self.task = self.store.request({'action': action, 'id': self.task['id'], 'revision': self.task['revision'], 'actor': 'agent:test', **fields})
        return self.task

    def test_blocker_and_stale_revision(self):
        with self.assertRaises(ValueError):
            self.update('status', status='blocked')
        self.update('status', status='blocked', blockers=[{'reason': 'Awaiting input'}])
        with self.assertRaises(ValueError):
            self.store.request({'action': 'status', 'id': self.task['id'], 'revision': 1, 'actor': 'other', 'status': 'done'})
        self.assertEqual(self.store.request({})[0]['status'], 'blocked')

    def test_verification_is_separate_and_durable(self):
        self.update('status', status='done')
        self.assertEqual(self.task['verification'], [])
        self.update('verify', outcome='pending', evidence='Browser unavailable', artifact='commit:example')
        self.assertEqual(self.task['status'], 'done')
        self.assertEqual(Store(self.temp.name).request({})[0]['verification'][0]['actor'], 'agent:test')
        self.update('status', status='in_progress')
        self.assertFalse(self.task['completions'])
        self.assertEqual(len(self.task['verification']), 1)

    def test_failed_verification_never_completes(self):
        self.update('verify', outcome='failed', evidence='Check failed', artifact='build:example')
        self.assertEqual(self.task['status'], 'ready')
        self.assertFalse(self.task['completedAt'])

    def test_invalid_dependency_and_missing_evidence(self):
        with self.assertRaises(ValueError):
            self.update('status', status='blocked', blockers=[{'reason': 'Self', 'task_id': self.task['id']}])
        with self.assertRaises(ValueError):
            self.update('verify', outcome='passed', evidence='', artifact='build:example')

    def test_tasktjik_configuration_overrides_launcher_defaults(self):
        config = Path(self.temp.name) / 'config/tasktjik'
        config.mkdir(parents=True)
        directory = Path(self.temp.name) / 'authority'
        (config / 'tasktjik.conf').write_text('[tasktjik]\nauthority=""\ndirectory="'+str(directory)+'"\n')
        with patch.dict(os.environ, {'XDG_CONFIG_HOME': str(config.parent)}):
            result = Todo().request({'action': 'create', 'fields': {'title': 'Config precedence'}},
                {'plugin_settings': {'Todo': {'authority': 'unreachable.invalid'}}})
        self.assertEqual(Store(directory).request({})[0]['id'], result['id'])
