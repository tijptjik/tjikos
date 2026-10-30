"""Client enrollment and retry contracts; all state is synthetic."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import client


class ClientTests(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        environment = patch.dict(os.environ, {'XDG_STATE_HOME': self.root.name, 'XDG_CONFIG_HOME': self.root.name})
        environment.start()
        self.addCleanup(environment.stop)

    def test_stable_private_device_identity(self):
        self.assertEqual(client.device_identity(), client.device_identity())
        path = Path(self.root.name) / 'tasktjik/device.json'
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_endpoint_requires_private_https_without_credentials_or_redirects(self):
        for url in ['http://si.example.ts.net/tasktjik', 'https://example.org/tasktjik',
                    'https://user:password@si.example.ts.net/tasktjik',
                    'https://si.example.ts.net:8443/tasktjik', 'https://si.example.ts.net/other']:
            with self.subTest(url=url), self.assertRaises(ValueError):
                client.base_url(url)
        with self.assertRaises(ValueError):
            client.NoRedirect().redirect_request(None)

    def test_mutation_keeps_caller_retry_id(self):
        body = {'action': 'done', 'request_id': 'stable-id'}
        with patch.object(client, 'http_request', return_value={'result': True}) as http:
            self.assertTrue(client.api_request('https://si.example.ts.net/tasktjik', body))
        self.assertEqual(http.call_args.args[2]['request_id'], 'stable-id')
        self.assertEqual(body, {'action': 'done', 'request_id': 'stable-id'})

    def test_enrollment_preserves_settings_and_private_backup(self):
        root = Path(self.root.name) / 'tasktjik'
        root.mkdir()
        path = root / 'tasktjik.conf'
        original = '[tasktjik]\nvault = "Synthetic vault"\nurl = "https://old.example.ts.net/tasktjik"\n'
        path.write_text(original)
        with patch.object(client, 'http_request', side_effect=[{'login': 'synthetic@example.test'}, {}]):
            client.connect('https://si.example.ts.net/tasktjik', True)
        self.assertIn('vault = "Synthetic vault"', path.read_text())
        self.assertEqual(path.read_text().count('url ='), 1)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        backup = next(root.glob('*.pre-api-*'))
        self.assertEqual(backup.read_text(), original)
        self.assertEqual(backup.stat().st_mode & 0o777, 0o600)
