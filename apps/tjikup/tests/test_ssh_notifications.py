"""Verify SSH notifications use only the originating terminal transport."""
import base64
import contextlib
import io
import os
from pathlib import Path
import re
import sys
import tempfile
import unittest
from unittest.mock import patch

# Works in the current dotfiles layout and the extracted tooltjik application.
ROOT = Path(__file__).resolve().parents[1]
for source in (ROOT / 'src', ROOT / 'home/dot_local/lib', Path.home() / 'code/tooltjik/apps/tjikup/src'):
    if (source / 'tjikup/notifications.py').is_file():
        sys.path.insert(0, str(source))
        break
from tjikup import notifications


class SSHNotificationTests(unittest.TestCase):
    def capture(self, *, tty=True, term='xterm-kitty', disabled=False, result=0, ssh=True):
        output = io.StringIO()
        environment = {'TERM': term, 'DISPLAY': ':9'}
        if ssh:
            environment['SSH_CONNECTION'] = '192.0.2.1 4000 192.0.2.2 22'
        if disabled:
            environment['TJIKUP_NOTIFY'] = '0'
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'report.tsv'
            report.write_text('WARN\t!\tA & <B>\tprivate details\n')
            with patch.dict(os.environ, environment, clear=True), \
                 patch.object(notifications.socket, 'gethostname', return_value='si.example'), \
                 contextlib.redirect_stdout(output), patch.object(output, 'isatty', return_value=tty), \
                 patch.object(notifications.subprocess, 'run') as desktop:
                notifications.notify_completion(report, result, 83)
                desktop.assert_not_called()
        return output.getvalue()

    def test_remote_result_is_encoded_and_host_labelled(self):
        output = self.capture()
        frames = re.findall(r'\x1b\]99;([^;]*);([A-Za-z0-9+/=]*)\x1b\\', output)
        self.assertEqual(len(frames), 2)
        metadata = [dict(field.split('=', 1) for field in row[0].split(':')) for row in frames]
        self.assertEqual(metadata[0]['i'], metadata[1]['i'])
        self.assertEqual(metadata[0]['d'], '0')
        self.assertEqual(metadata[1]['d'], '1')
        self.assertEqual(metadata[0]['a'], '-report')
        self.assertEqual(metadata[0]['u'], '1')
        self.assertEqual(base64.b64decode(metadata[0]['n']), b'tjikup-notification')
        self.assertEqual(base64.b64decode(metadata[0]['f']), b'Tjikup')
        self.assertEqual(base64.b64decode(frames[0][1]).decode(), 'Tjikup · si needs attention')
        body = base64.b64decode(frames[1][1]).decode()
        self.assertIn('A & <B>', body)
        self.assertNotIn('&amp;', body)
        self.assertNotIn('private details', body)

    def test_failure_is_critical(self):
        self.assertIn(':u=2:', self.capture(result=1))

    def test_no_escape_sequences_in_logs_or_unsupported_terminals(self):
        for kwargs in ({'tty': False}, {'term': 'xterm-256color'}, {'disabled': True}):
            with self.subTest(kwargs=kwargs):
                self.assertEqual(self.capture(**kwargs), '')

    def test_closed_terminal_does_not_fail_the_command(self):
        with patch.dict(os.environ, {'SSH_TTY': '/dev/pts/1'}, clear=True), \
             patch.object(notifications, 'notify_ssh_terminal', side_effect=BrokenPipeError):
            notifications.notify_completion(None, 1, 2)

    def test_each_run_has_an_independent_identifier(self):
        first = self.capture().split(':', 1)[0]
        second = self.capture().split(':', 1)[0]
        self.assertNotEqual(first, second)


if __name__ == '__main__':
    unittest.main()
