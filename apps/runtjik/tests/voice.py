"""Voice history and actions with private synthetic data and stub commands."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from rust_parity import Backend, fixture


class VoiceTests(unittest.TestCase):
    def test_history_and_actions(self):
        with tempfile.TemporaryDirectory(prefix="runtjik-voice-") as tmp:
            root = Path(tmp)
            env = fixture(root, 0)
            history = root / "state/hyprvoice/history"
            history.mkdir(parents=True)
            for name, created, text in [("old.json", "2026-11-09T09:00:00Z", "Old text"),
                                        ("new.json", "2026-11-09T10:00:00Z", "Hello 香港\nsecond line")]:
                (history / name).write_text(json.dumps(dict(created=created, text=text)))
            (history / ".pending-1").write_text('{')
            (history / "bad.json").write_text('{')
            bindir = root / "bin"
            bindir.mkdir()
            for name, body in {
                "wl-copy": 'cat > "$VOICE_COPY"',
                "hyprvoice": 'echo "$*" >> "$VOICE_CALLS"; if [ "$1" = status ]; then echo "STATUS status=${VOICE_STATUS:-idle}"; else echo OK; fi',
                "terminal": 'echo "$*" > "$VOICE_TERMINAL"',
            }.items():
                file = bindir / name
                file.write_text('#!/bin/sh\n' + body + '\n')
                file.chmod(0o700)
            env.update(PATH=str(bindir) + ':' + os.environ['PATH'], VOICE_COPY=str(root / 'copy'),
                       VOICE_CALLS=str(root / 'calls'), VOICE_TERMINAL=str(root / 'terminal'))
            Path(env['RUNTJIK_CONFIG']).write_text('[index]\npath_commands=false\n[launch]\nisolate=false\nterminal=["terminal"]\n')
            backend = Backend('rust', env)
            try:
                menu = backend.request(op='query', query='/voice')['results']
                self.assertEqual([r['name'] for r in menu], ['Voice Record', 'Voice History'])
                reply = backend.request(op='activate', id='Voice:history', mode='launch')
                self.assertEqual(reply['edit_query'], '/voice history')
                self.assertEqual([r['id'] for r in reply['results']], ['Voice:new.json', 'Voice:old.json'])
                reply = backend.request(op='activate', id='Voice:new.json', mode='launch')
                self.assertEqual(reply['kind'], 'activated')
                self.assertTrue(reply['close_launcher'])
                self.assertEqual((root / 'copy').read_text(), 'Hello 香港\nsecond line')
                reply = backend.request(op='activate', id='Voice:new.json', mode='close')
                self.assertNotEqual(reply['kind'], 'activated')
                self.assertFalse((history / 'new.json').exists())
                self.assertEqual([r['id'] for r in reply['results']], ['Voice:old.json'])
                self.assertEqual(backend.request(op='query', query='/voice history missing')['results'], [])
                backend.request(op='query', query='/voice')
                backend.request(op='activate', id='Voice:record', mode='launch')
                self.assertEqual((root / 'calls').read_text().splitlines(), ['status', 'toggle'])
                backend.request(op='activate', id='Voice:record', mode='switch')
                from private_compositor import wait_for
                wait_for(lambda: (root / 'terminal').exists())
                self.assertEqual((root / 'terminal').read_text().strip(), 'hyprvoice configure')
            finally:
                backend.close()
                backend.process.stdout.close()
            env['VOICE_STATUS'] = 'recording'
            backend = Backend('rust', env)
            try:
                backend.request(op='query', query='/voice')
                reply = backend.request(op='activate', id='Voice:record', mode='launch')
                self.assertEqual(reply['kind'], 'error')
                self.assertEqual((root / 'calls').read_text().splitlines(), ['status', 'toggle', 'status'])
            finally:
                backend.close()
                backend.process.stdout.close()
            backend = Backend('rust', env, demo=True)
            try:
                rows = backend.request(op='query', query='/voice history')['results']
                self.assertEqual(len(rows), 2)
                backend.request(op='activate', id=rows[0]['id'], mode='close')
                self.assertTrue((history / 'old.json').exists())
            finally:
                backend.close()
                backend.process.stdout.close()


if __name__ == '__main__':
    unittest.main()
