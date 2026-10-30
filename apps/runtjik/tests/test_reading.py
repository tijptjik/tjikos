"""Reading saves must be durable before browser tab closure."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
import uuid
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import runtjik as r
from todo import Store, Todo


class ReadingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(self.temp.name)
        environment = patch.dict(os.environ, {"XDG_CONFIG_HOME": self.temp.name})
        environment.start()
        self.addCleanup(environment.stop)

    def test_types_groups_legacy_reference_and_retry(self):
        old = self.store.request({'action': 'create', 'fields': {'title': 'Old plan', 'plan': 'Notes/plan.md'}})
        self.assertEqual(old['reference'], 'Notes/plan.md')
        self.assertEqual(old['kind'], 'todo')
        key = str(uuid.uuid4())
        request = {'action': 'create', 'request_id': key, 'fields': {'title': 'Read this', 'kind': 'reading',
                   'reference': 'https://example.org/read', 'group': 'Research notes'}}
        saved = self.store.request(request)
        self.assertEqual(saved['group'], 'Research notes')
        self.assertEqual(self.store.request(request)['id'], saved['id'])
        self.assertEqual(len(self.store.request({})), 2)
        for url in ('file:///tmp/private', 'javascript:alert(1)', 'https://'):
            with self.assertRaises(ValueError):
                self.store.request({'action': 'create', 'fields': {**request['fields'], 'reference': url}})

    def test_authority_transport_uses_argv_and_strict_host_keys(self):
        response = Mock(stdout=json.dumps({'result': {'reading': True}}))
        settings = {'plugin_settings': {'Todo': {'authority': 'm@example.org', 'ssh_command': ['mullvad-exclude', 'ssh']}}}
        with patch('todo.subprocess.run', return_value=response) as run:
            self.assertTrue(Todo().request({'action': 'capabilities'}, settings)['reading'])
        argv = run.call_args.args[0]
        self.assertEqual(argv[:2], ['mullvad-exclude', 'ssh'])
        self.assertIn('-oStrictHostKeyChecking=yes', argv)
        self.assertEqual(argv[-2], 'm@example.org')

    def test_browser_filing_is_deferred_without_io(self):
        settings = r.config('')
        settings['plugins']['enabled'] = ['Browser']
        with patch.object(r, 'config', return_value=settings):
            launcher = r.Launcher(demo=True)
        launcher._icons = Mock(resolve=lambda name: name)
        row = launcher.query('/tabs Firefox')['results'][0]
        with patch.object(launcher.providers['Browser'], 'tab_request') as request:
            result = launcher.activate(row['id'], 'switch')
        self.assertEqual(result['kind'], 'message')
        self.assertIn('not connected', result['message'])
        request.assert_not_called()


if __name__ == '__main__':
    unittest.main()
