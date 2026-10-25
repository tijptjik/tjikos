"""Generated audio only: these tests never access a microphone or Telegram."""
import json
import os
from pathlib import Path
import queue
import threading
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import telegram_recording as recording


class RecordingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = patch.dict(os.environ, {"XDG_RUNTIME_DIR":str(self.root)})
        self.env.start(); self.addCleanup(self.env.stop)

    def start(self, bad_input=False):
        source = ['-re','-f','lavfi','-i','anullsrc=r=48000:cl=mono'] if not bad_input else ['-f','lavfi','-i','not-a-real-filter']
        process = subprocess.Popen([sys.executable, '-B', '-c',
            'import telegram_recording as r; raise SystemExit(r.run('+repr(source)+'))'],
            cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        process.events = queue.Queue()
        threading.Thread(target=lambda: [process.events.put(json.loads(line)) for line in process.stdout], daemon=True).start()
        def cleanup():
            if process.poll() is None:
                process.kill(); process.wait(timeout=5)
            for stream in (process.stdin, process.stdout, process.stderr): stream.close()
        self.addCleanup(cleanup)
        return process

    def event(self, process, state, seconds=10):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                result = process.events.get(timeout=.1)
                if result.get('state') == state:
                    return result
            except queue.Empty:
                pass
        self.fail('Timed out waiting for '+state)

    def draft(self):
        path = recording.draft_path('a'*32)
        subprocess.run(['ffmpeg','-v','error','-f','lavfi','-i','anullsrc=r=48000:cl=mono',
                        '-t','1','-c:a','libopus',str(path)], check=True)
        path.chmod(0o600)
        return path

    def test_stop_review_discard_and_private_permissions(self):
        process = self.start()
        self.event(process,'recording')
        time.sleep(.7)
        process.stdin.write('{"action":"stop"}\n'); process.stdin.flush()
        result = self.event(process,'ready')
        path = recording.draft_path(result['token'])
        self.assertTrue(path.exists())
        self.assertGreaterEqual(result['duration'], 250)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)
        process.stdin.write('{"action":"discard"}\n'); process.stdin.flush()
        process.wait(timeout=5)
        self.assertFalse(path.exists())

    def test_ui_exit_stops_capture_and_deletes_draft(self):
        process = self.start()
        self.event(process,'recording')
        time.sleep(.3)
        process.stdin.close()
        process.wait(timeout=5)
        self.assertFalse(list(recording.draft_dir().glob('*.ogg')))

    def test_device_failure_is_actionable(self):
        process = self.start(bad_input=True)
        result = self.event(process,'error')
        process.wait(timeout=5)
        self.assertIn('Microphone unavailable',result['error'])
        self.assertFalse(list(recording.draft_dir().glob('*.ogg')))

    def test_upload_copy_survives_draft_discard(self):
        path = self.draft()
        copy, duration = recording.copy_for_upload('a'*32,self.root/'outbox')
        path.unlink()
        self.assertTrue(copy.exists())
        self.assertGreater(duration,.9)
        self.assertEqual(copy.stat().st_mode & 0o777,0o600)

    def test_token_traversal_symlinks_permissions_and_non_audio_rejected(self):
        for token in ('../other',str(self.root/'secret'),'a'*33,None):
            with self.assertRaises(ValueError): recording.copy_for_upload(token,self.root/'outbox')
        path=recording.draft_path('a'*32)
        target=self.root/'unrelated'; target.write_text('private')
        path.symlink_to(target)
        with self.assertRaises(OSError): recording.copy_for_upload('a'*32,self.root/'outbox')
        path.unlink(); path.write_text('not audio'); path.chmod(0o600)
        with self.assertRaises(ValueError): recording.copy_for_upload('a'*32,self.root/'outbox')
        self.assertFalse(list((self.root/'outbox').glob('*.ogg')))
        path.chmod(0o644)
        with self.assertRaises(ValueError): recording.copy_for_upload('a'*32,self.root/'outbox')
        self.assertEqual(target.read_text(),'private')


if __name__ == '__main__': unittest.main()
