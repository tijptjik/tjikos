"""Synthetic CLI and PTY acceptance; no host credentials, timers, or remote stores."""
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
import unittest
import fcntl

BIN = Path(os.environ.get('SYNCTJIK_TEST_BINARY', Path(__file__).resolve().parents[1] / 'target/debug/synctjik'))


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.policy = self.root / 'policy.json'
        self.policy.write_text(json.dumps({'version': 2, 'master': 'leader',
            'remote': str(self.root / 'store'), 'initial_sync_required': [],
            'folders': {'assets': {'mode': 'sync', 'encrypted': False}}}))
        for name in ('leader', 'client'):
            (self.root / name).mkdir()
        (self.root / 'leader/assets').mkdir()
        (self.root / 'leader/assets/settings').write_text('synthetic configuration\n')
        self.run_cli('leader', 'backup')
        self.run_cli('client', 'enroll', '--role', 'laptop', '--from', 'leader', '--collections', 'assets')

    def args(self, machine, *args):
        return [str(BIN), '--home', str(self.root / machine), '--machine', machine,
                '--config', str(self.policy), *args]

    def run_cli(self, machine, *args, ok=True):
        result = subprocess.run(self.args(machine, *args), capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, ok, result.stderr)
        return result

    def test_readiness_blocks_until_restoration_then_schedule_can_be_previewed(self):
        self.run_cli('client', 'ready', ok=False)
        result = json.loads(self.run_cli('client', 'plan').stdout)
        self.run_cli('client', 'apply', result['plan'])
        self.run_cli('client', 'ready')
        units = json.loads(self.run_cli('client', 'schedule', '--print').stdout)
        self.assertIn('synctjik ready', units['service'])
        self.assertIn('00:15:00', units['timer'])
        self.assertFalse((self.root / 'client/.config/systemd').exists())

    def review_pty(self, finish):
        plan = json.loads(self.run_cli('client', 'plan').stdout)['plan']
        pid, fd = pty.fork()
        if pid == 0:
            os.execv(str(BIN), self.args('client', 'review', plan))
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 28, 110, 0, 0))
        output = bytearray()
        def wait_for(marker):
            deadline = time.monotonic() + 10
            while marker not in output and time.monotonic() < deadline:
                if select.select([fd], [], [], .1)[0]:
                    try:
                        output.extend(os.read(fd, 65536))
                    except OSError:
                        break
            self.assertIn(marker, output)
        try:
            wait_for(b'pending')
            wait_for(b'synthetic configuration')
            if finish:
                os.write(fd, b'A\r')
                wait_for(b'A recovery copy will be kept.')
                if finish == 'revisit':
                    os.write(fd, b'n\n')
                    wait_for(b'Return to the review?')
                    output.clear()
                    os.write(fd, b'y\n')
                    wait_for(b'accepted')
                    os.write(fd, b'\r')
                    wait_for(b'A recovery copy will be kept.')
                os.write(fd, b'y\n')
            else:
                os.kill(pid, signal.SIGTERM)
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if select.select([fd], [], [], .1)[0]:
                    try:
                        data = os.read(fd, 65536)
                    except OSError:
                        break
                    if not data:
                        break
                    output.extend(data)
            _, status = os.waitpid(pid, 0)
            self.assertEqual(os.waitstatus_to_exitcode(status), 0)
            self.assertIn(b'\x1b[?25h', output)
            self.assertIn(b'\x1b[?1049l', output)
        finally:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.close(fd)
        return output


    def test_declining_confirmation_preserves_review_decisions(self):
        self.review_pty('revisit')
        self.run_cli('client', 'ready')

    def test_unknown_device_without_assignments_is_not_ready(self):
        config = json.loads(self.policy.read_text())
        config['folders']['assets']['devices'] = ['leader']
        self.policy.write_text(json.dumps(config))
        (self.root / 'new').mkdir()
        self.run_cli('new', 'ready', ok=False)
        self.run_cli('new', 'backup', ok=False)

    def test_two_pane_review_and_apply(self):
        self.review_pty(True)
        self.assertEqual((self.root / 'client/assets/settings').read_text(), 'synthetic configuration\n')
        self.run_cli('client', 'ready')

    def test_termination_restores_terminal_and_does_not_apply(self):
        self.review_pty(False)
        self.assertFalse((self.root / 'client/assets').exists())
        self.run_cli('client', 'ready', ok=False)


if __name__ == '__main__':
    unittest.main()
