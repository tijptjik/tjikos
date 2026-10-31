"""Synthetic authority restart, offline outbox, and history-exclusion acceptance."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[3]
API = ROOT / 'apps/notjik-service/target/release/notjik-api'
CLIENT = ROOT / 'packages/notjik-sync/target/release/notjik-sync'
TJIKUP = ROOT / 'apps/tjikup/target/release/tjikup'

class Delivery(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='n-sync-')
        self.root = Path(self.temp.name)
        self.socket = self.root / 'api.sock'
        self.cfg = self.root / 'api.toml'
        self.cfg.write_text(f'''[server]
socket = "{self.socket}"
directory = "{self.root / 'authority'}"
origin = "https://si.test"
allowed_users = ["test@example.test"]
[server.local_users]
"{os.getuid()}" = "test@example.test"
''')
        self.log = (self.root / 'server.log').open('w')
        self.process = None
        self.start()
        self.envs = {}
        for name, kind in [('source', 'server'), ('receiver', 'desktop')]:
            home = self.root / name
            (home / '.local/bin').mkdir(parents=True)
            fake = home / '.local/bin/notjikctl'
            fake.write_text('''#!/usr/bin/env python3
import json,pathlib,os,sys
n=json.load(sys.stdin)
p=pathlib.Path(os.environ['HOME'])/'shown.json'
p.write_text(json.dumps(n))
print('synthetic-local-handle')
''')
            fake.chmod(0o755)
            env = dict(os.environ, HOME=str(home), NOTJIK_SYNC_CONFIG=str(home / 'sync.json'), NOTJIK_SYNC_STATE=str(home / 'state'))
            self.envs[name] = env
            self.run_cli(name, 'connect', 'http://localhost', name, kind, str(self.socket))
        self.request('receiver', {'op': 'presence', 'active': True, 'ready': True, 'idle_seconds': 0})

    def start(self):
        self.process = subprocess.Popen([str(API), '--config', str(self.cfg)], stdout=self.log, stderr=self.log)
        for _ in range(100):
            if self.socket.exists():
                time.sleep(0.05)
                return
            self.assertIsNone(self.process.poll())
            time.sleep(0.02)
        self.fail('Authority did not bind its socket')

    def stop(self):
        self.process.terminate()
        self.process.wait(timeout=10)
        self.socket.unlink(missing_ok=True)

    def tearDown(self):
        if self.process and self.process.poll() is None:
            self.stop()
        self.log.close()
        self.temp.cleanup()

    def run_cli(self, who, *args, payload=None, env=None):
        r = subprocess.run([str(CLIENT), *args], input=None if payload is None else json.dumps(payload), text=True, capture_output=True, env=env or self.envs[who], timeout=20)
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def request(self, who, payload):
        return json.loads(self.run_cli(who, 'request', payload=payload))

    def test_offline_fallback_survives_restart_without_popup_replay(self):
        self.stop()
        env = dict(self.envs['source'], WAYLAND_DISPLAY='synthetic')
        n = {'app': 'Synthetic script', 'run': 'offline-test', 'title': 'Fixture result', 'body': 'No private data', 'actions': []}
        key = self.run_cli('source', 'publish', payload=n, env=env)
        self.assertTrue((self.root / 'source/shown.json').exists())
        with sqlite3.connect(self.root / 'source/state/client.sqlite3') as db:
            self.assertEqual(db.execute('SELECT count(*) FROM outbox').fetchone()[0], 1)
        self.start()
        self.run_cli('source', 'flush')
        history = self.request('receiver', {'op': 'history'})['notices']
        self.assertEqual(len(history), 1)
        self.assertEqual(history[0]['id'], key)
        self.assertTrue(history[0]['shown'])
        self.assertEqual(self.request('receiver', {'op': 'poll'})['notices'], [])
        with sqlite3.connect(self.root / 'source/state/client.sqlite3') as db:
            self.assertEqual(db.execute('SELECT count(*) FROM outbox').fetchone()[0], 0)

    def test_telegram_excluded_but_managed_results_are_retained(self):
        for app in ['Telegram via Notjik', 'Telegram Desktop', 'Synthetic script']:
            self.run_cli('source', 'publish', payload={'app': app, 'title': 'Fixture', 'body': '', 'actions': []})
        self.stop()
        self.start()
        history = self.request('receiver', {'op': 'history'})['notices']
        self.assertEqual([n['app'] for n in history], ['Synthetic script'])
        self.assertEqual(len(self.request('receiver', {'op': 'poll'})['notices']), 3)

    def test_large_history_page_is_not_truncated_by_transport(self):
        for i in range(40):
            self.run_cli('source', 'publish', payload={
                'app': 'Synthetic script', 'title': f'Result {i}', 'body': '',
                'data': {'detail': 'x' * 60000}, 'actions': [],
            })
        self.assertEqual(len(self.request('receiver', {'op': 'history'})['notices']), 40)

    def test_legacy_report_entry_uses_notjik_without_telegram_credentials(self):
        env = dict(self.envs['source'], XDG_STATE_HOME=str(self.root / 'scheduled'),
                   TJIKUP_TELEGRAM_CONFIG=str(self.root / 'no-telegram.json'))
        payload = json.dumps({'host': 'fixture', 'text': 'Synthetic report', 'run_id': 'run-1'})
        for _ in range(2):
            result = subprocess.run([str(TJIKUP), 'scheduled', '--report-stdin'],
                                    input=payload, text=True, capture_output=True, env=env, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
        notices = self.request('receiver', {'op': 'history'})['notices']
        self.assertEqual(len(notices), 1)
        self.assertEqual(notices[0]['run'], 'fixture:run-1')

if __name__ == '__main__':
    unittest.main(verbosity=2)
