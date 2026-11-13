#!/usr/bin/env python3
"""Real-terminal native conflict review against private synthetic homes only."""
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import tempfile
import time
import termios
import unittest

BINARY = os.environ['TJIKUP_BINARY']


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        (self.base / 'root').mkdir()
        self.source = self.base / 'source'
        self.source.write_text('managed-secret\n')
        self.target = self.base / 'root/config'
        self.target.write_text('local-secret\n')
        self.target.chmod(0o600)
        self.manifest = self.base / 'manifest.json'
        self.spec = {'version': 1, 'id': 'review', 'entries': [
            {'path': '/config', 'source': 'source', 'kind': 'file', 'mode': '0600', 'sensitive': True}]}
        self.save()
        self.log = self.base / 'stdout.log'

    def save(self):
        self.manifest.write_text(json.dumps(self.spec))

    def args(self, action='apply'):
        return [BINARY, 'files', action, '--manifest', str(self.manifest),
                '--root', str(self.base / 'root'), '--state-dir', str(self.base / 'state')]

    def plain(self, action='apply'):
        return subprocess.run(self.args(action), capture_output=True, text=True)

    def review(self, choices, mutate=None, scheduled=True, guarded=False, environment=None):
        command = self.args()
        if guarded:
            supervisor = self.base / 'supervisor.py'
            supervisor.write_text('import os, subprocess, sys\n'
                'group = os.tcgetpgrp(0)\n'
                'os.environ["TJIKUP_REVIEW_FOREGROUND_GROUP"] = str(group)\n'
                'child = subprocess.Popen(sys.argv[1:], process_group=0)\n'
                'code = child.wait()\n'
                'assert os.tcgetpgrp(0) == group, "foreground not restored"\n'
                'sys.exit(code)\n')
            command = ['python3', str(supervisor), *command]
        if scheduled:
            config = self.base / 'workflow.json'
            config.write_text(json.dumps({'version': 2, 'recipes': [
                {'id': 'files', 'interactive': True, 'apply': command}]}))
            command = [BINARY, '--config', str(config)]
        pid, fd = pty.fork()
        if pid == 0:
            if environment is not None:
                os.environ.update(environment)
            out = os.open(self.log, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            os.dup2(out, 1)
            os.close(out)
            os.execv(BINARY, command)
        self.addCleanup(lambda: self.kill_if_running(pid))
        terminal_settings = termios.tcgetattr(fd)
        data = b''
        deadline = time.monotonic() + 15
        sent = 0
        prompts = 0
        while time.monotonic() < deadline:
            ready, _, _ = select.select([fd], [], [], .05)
            if ready:
                try:
                    chunk = os.read(fd, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                data += chunk
                prompts = data.count(b'Cancel (default): ')
                if prompts > sent and sent < len(choices):
                    if mutate is not None and sent == 0:
                        mutate()
                    os.write(fd, choices[sent].encode())
                    sent += 1
            done, status = os.waitpid(pid, os.WNOHANG)
            if done:
                self.assertEqual(termios.tcgetattr(fd), terminal_settings)
                os.close(fd)
                return os.waitstatus_to_exitcode(status), data.decode(errors='replace')
        self.assertEqual(termios.tcgetattr(fd), terminal_settings)
        os.close(fd)
        done, status = os.waitpid(pid, os.WNOHANG)
        # PTY closure can precede process exit; allow the child to be reaped.
        while not done and time.monotonic() < deadline:
            time.sleep(.01)
            done, status = os.waitpid(pid, os.WNOHANG)
        if not done:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
            self.fail('review did not finish: ' + data.decode(errors='replace'))
        return os.waitstatus_to_exitcode(status), data.decode(errors='replace')

    @staticmethod
    def kill_if_running(pid):
        try:
            done, _ = os.waitpid(pid, os.WNOHANG)
            if not done:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
        except ChildProcessError:
            pass

    def test_keep_local_is_durable_until_managed_version_changes(self):
        code, terminal = self.review(['k'])
        self.assertEqual(code, 0, terminal)
        self.assertEqual(self.target.read_text(), 'local-secret\n')
        self.target.write_text('application-updated-secret\n')
        self.assertEqual(self.plain().returncode, 0)
        self.assertEqual(self.target.read_text(), 'application-updated-secret\n')
        check = self.plain('check')
        self.assertEqual(check.returncode, 0, check.stderr)
        self.assertIn('KEPT-LOCAL', check.stdout)
        self.source.write_text('new-managed-policy\n')
        self.assertEqual(self.plain().returncode, 2)
        code, terminal = self.review(['m'])
        self.assertEqual(code, 0, terminal)
        self.assertEqual(self.target.read_text(), 'new-managed-policy\n')

    def test_managed_has_restorable_backup_and_preview_stays_off_logs(self):
        code, terminal = self.review(['R', 'm'], guarded=True)
        self.assertEqual(code, 0, terminal)
        self.assertIn('local-secret', terminal)
        self.assertIn('managed-secret', terminal)
        self.assertNotIn('local-secret', self.log.read_text())
        self.assertNotIn('managed-secret', self.log.read_text())
        self.assertEqual(self.target.read_text(), 'managed-secret\n')
        receipt = next((self.base / 'state').glob('receipt-*/receipt.json'))
        result = subprocess.run([BINARY, 'files', 'restore', '--receipt', str(receipt),
                                 '--state-dir', str(self.base / 'state'), '--root', str(self.base / 'root')],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.target.read_text(), 'local-secret\n')
        self.assertEqual(self.target.stat().st_mode & 0o777, 0o600)

    def test_colored_fallback_escapes_terminal_commands(self):
        self.target.write_text('local-secret\x1b]0;injected\x07\n')
        # A failed renderer must preserve a useful preview and decision prompt.
        tools = self.base / 'bin'
        tools.mkdir()
        delta = tools / 'delta'
        delta.write_text('#!/bin/sh\nexit 1\n')
        delta.chmod(0o700)
        code, terminal = self.review(['r', 'q'], environment={
            'PATH': str(tools) + ':' + os.environ['PATH'],
            'TERM': 'xterm-256color', 'NO_COLOR': ''})
        self.assertNotEqual(code, 0)
        self.assertIn('\x1b[31m-local-secret', terminal)
        self.assertIn('\x1b[32m+managed-secret', terminal)
        self.assertNotIn('\x1b]0;injected', terminal)
        self.assertIn(r'\u{1b}]0;injected\u{7}', terminal)
        self.assertNotIn('local-secret', self.log.read_text())

    def test_delta_receives_sanitized_diff_and_terminal_output(self):
        tools = self.base / 'bin'
        tools.mkdir()
        delta = tools / 'delta'
        delta.write_text('#!/bin/sh\nprintf "DELTA %s\\n" "$*"\ncat\n')
        delta.chmod(0o700)
        code, terminal = self.review(['r', 'q'], environment={
            'PATH': str(tools) + ':' + os.environ['PATH'],
            'TERM': 'xterm-256color', 'NO_COLOR': ''})
        self.assertNotEqual(code, 0)
        self.assertIn('DELTA --no-gitconfig --paging=never --line-numbers', terminal)
        self.assertIn('-local-secret', terminal)
        self.assertNotIn('DELTA', self.log.read_text())
        self.assertEqual(self.target.read_text(), 'local-secret\n')

    def test_no_color_preview(self):
        code, terminal = self.review(['r', 'q'], scheduled=False,
                                     environment={'NO_COLOR': '1'})
        self.assertNotEqual(code, 0)
        self.assertIn('-local-secret', terminal)
        self.assertNotIn('\x1b[', terminal)

    def test_control_keys_cancel_and_restore_terminal(self):
        for key in ['\n', '\x03', '\x04', '\x1b']:
            with self.subTest(key=repr(key)):
                code, terminal = self.review([key], scheduled=False)
                self.assertNotEqual(code, 0, terminal)
                self.assertEqual(self.target.read_text(), 'local-secret\n')
                self.assertFalse((self.base / 'state/review.json').exists())

    def test_invalid_key_reprompts_without_enter(self):
        code, terminal = self.review(['x', 'K'])
        self.assertEqual(code, 0, terminal)
        self.assertIn('Choose r, k, m, or q.', terminal)
        self.assertEqual(self.target.read_text(), 'local-secret\n')

    def test_cancel_does_not_write_any_file_or_decision(self):
        (self.base / 'root/second').write_text('second-local\n')
        self.spec['entries'].append({**self.spec['entries'][0], 'path': '/second'})
        self.save()
        code, _ = self.review(['m', 'q'])
        self.assertNotEqual(code, 0)
        self.assertEqual(self.target.read_text(), 'local-secret\n')
        self.assertEqual((self.base / 'root/second').read_text(), 'second-local\n')
        self.assertFalse((self.base / 'state/review.json').exists())
        self.assertFalse(list((self.base / 'state').glob('receipt-*')))

    def test_rejects_edits_made_during_review(self):
        code, _ = self.review(['m'], lambda: self.target.write_text('newer-local\n'))
        self.assertNotEqual(code, 0)
        self.assertEqual(self.target.read_text(), 'newer-local\n')

    def test_noninteractive_fails_with_recovery_guidance(self):
        result = self.plain()
        self.assertEqual(result.returncode, 2)
        self.assertIn('interactive terminal', result.stderr)
        self.assertNotIn('local-secret', result.stderr)
        self.assertEqual(self.target.read_text(), 'local-secret\n')

    def test_keep_local_applies_unrelated_files(self):
        self.spec['entries'].append({**self.spec['entries'][0], 'path': '/new'})
        self.save()
        code, terminal = self.review(['k'])
        self.assertEqual(code, 0, terminal)
        self.assertEqual((self.base / 'root/new').read_text(), 'managed-secret\n')
        self.assertEqual(self.target.read_text(), 'local-secret\n')


if __name__ == '__main__':
    unittest.main()
