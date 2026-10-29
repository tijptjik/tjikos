import os
from pathlib import Path
import signal
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import runtjik as r


class CloseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.env = patch.dict(os.environ, {"XDG_CONFIG_HOME": str(root/'config'),
                                          "XDG_STATE_HOME": str(root/'state'),
                                          "RUNTJIK_CONFIG": str(root/'absent.conf')})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.launcher = r.Launcher(demo=True)
        self.launcher.demo = False
        self.launcher.store = r.Store(root/'state.sqlite3')
        self.addCleanup(self.launcher.store.db.close)
        self.entry = next(e for e in self.launcher.entries if e.id == 'apps:firefox.desktop')
        self.provider = Mock()
        self.launcher.providers['apps'] = self.provider
        self.clients = [{'address': '0xabc', 'class': 'firefox', 'pid': 10001},
                        {'address': '0xdef', 'class': 'firefox', 'pid': 10001}]
        self.identities = {10001: 'boot:100'}
        for name, options in [('windows', {'return_value': self.clients}),
                              ('process_identity', {'side_effect': lambda pid: self.identities.get(pid)}),
                              ('close_windows', {}), ('signal_process', {'return_value': True})]:
            target = patch.object(r, name, **options)
            setattr(self, name, target.start())
            self.addCleanup(target.stop)

    def action(self, mode='close', address=''):
        return self.launcher.activate(self.entry.id, mode, address)

    def test_closing_feedback_tracks_requested_windows_until_gone(self):
        self.launcher._icons = Mock()
        self.launcher._icons.resolve.side_effect = lambda icon: icon
        self.action(address='0xabc')
        self.assertEqual(self.launcher.query('/processes')['closing'], [self.entry.id])
        self.clients[:] = [self.clients[1]]
        result = self.launcher.query('/processes')
        self.assertEqual(result['closing'], [])
        self.assertEqual(result['results'][0]['windows'], 1)
        self.action()
        self.clients.clear()
        result = self.launcher.query('/processes')
        self.assertEqual(result['closing'], [])
        self.assertEqual(result['results'], [])

    def test_first_request_is_graceful_for_every_matching_window(self):
        result = self.action()
        self.close_windows.assert_called_once_with(self.clients)
        self.signal_process.assert_not_called()
        self.assertEqual(result['action'], 'close')
        self.assertEqual(self.launcher.store.pending_closes(self.entry.id), {10001: ('boot:100', 0)})
        self.assertNotIn('launch:'+self.entry.id, self.launcher.store.counts())

    def test_plain_cli_does_not_escalate_an_old_false_gui_match(self):
        entry = r.Entry('commands:flatpak', 'flatpak', provider='commands', data={'argv':['/usr/bin/flatpak']})
        self.launcher.entries = [entry]
        self.launcher.store.remember_close(entry.id, 10001, 'boot:100', 0)
        with patch.object(r.Commands, 'activate') as launch:
            self.launcher.activate(entry.id, 'launch')
            launch.assert_called_once()
        self.signal_process.assert_not_called()
        self.assertEqual(self.launcher.activate(entry.id, 'close')['kind'], 'message')
        self.signal_process.assert_not_called()

    def test_ordinary_launch_retries_term_then_kill_without_launching(self):
        self.action()
        self.action('launch')
        self.signal_process.assert_called_once_with(10001, 'boot:100', signal.SIGTERM)
        self.action('launch')
        self.assertEqual(self.signal_process.call_args.args, (10001, 'boot:100', signal.SIGKILL))
        self.provider.activate.assert_not_called()

    def test_repeated_ctrl_enter_escalates_and_stops_at_sigkill(self):
        for _ in range(4):
            self.action()
        self.assertEqual([c.args[2] for c in self.signal_process.call_args_list],
                         [signal.SIGTERM, signal.SIGKILL, signal.SIGKILL])
        self.assertEqual(self.close_windows.call_count, 1)

    def test_pending_close_survives_reopen_even_when_windows_disappear(self):
        self.action()
        path = self.launcher.store.db.execute('PRAGMA database_list').fetchone()[2]
        reopened = r.Store(path)
        self.addCleanup(reopened.db.close)
        self.launcher.store = reopened
        self.windows.return_value = []
        self.action('launch')
        self.signal_process.assert_called_once_with(10001, 'boot:100', signal.SIGTERM)

    def test_exited_process_allows_normal_launch(self):
        self.action()
        self.identities.clear()
        self.action('launch')
        self.provider.activate.assert_called_once()
        self.signal_process.assert_not_called()
        self.assertEqual(self.launcher.store.pending_closes(self.entry.id), {})

    def test_reused_pid_starts_with_graceful_close(self):
        self.action()
        self.identities[10001] = 'different-boot:100'
        self.action()
        self.assertEqual(self.close_windows.call_count, 2)
        self.signal_process.assert_not_called()
        self.assertEqual(self.launcher.store.pending_closes(self.entry.id)[10001], ('different-boot:100', 0))

    def test_new_process_is_not_escalated_with_survivor(self):
        self.action()
        other = {'address': '0x123', 'class': 'firefox', 'pid': 10002}
        self.clients.append(other)
        self.identities[10002] = 'boot:200'
        self.action()
        self.close_windows.assert_called_with([other])
        self.signal_process.assert_called_once_with(10001, 'boot:100', signal.SIGTERM)

    def test_selected_instance_limits_initial_graceful_request(self):
        self.action(address='0xabc')
        self.close_windows.assert_called_once_with([self.clients[0]])

    def test_history_disabled_still_remembers_close_progress(self):
        self.launcher.settings['history']['enabled'] = False
        self.action()
        self.action('launch')
        self.signal_process.assert_called_once()
        self.assertEqual(self.launcher.store.counts(), {})

    def test_failed_graceful_request_does_not_advance(self):
        self.close_windows.side_effect = RuntimeError('close failed')
        with self.assertRaisesRegex(RuntimeError, 'close failed'):
            self.action()
        self.assertEqual(self.launcher.store.pending_closes(self.entry.id), {})

    def test_failed_signal_does_not_advance(self):
        self.action()
        self.signal_process.side_effect = PermissionError('denied')
        with self.assertRaises(PermissionError):
            self.action('launch')
        self.assertEqual(self.launcher.store.pending_closes(self.entry.id)[10001][1], 0)

    def test_no_windows_no_close_and_no_launch(self):
        self.windows.return_value = []
        self.assertEqual(self.action()['kind'], 'message')
        self.close_windows.assert_not_called()
        self.provider.activate.assert_not_called()

    def test_internal_close_does_not_execute_command(self):
        with patch.object(self.launcher, 'refresh') as refresh:
            self.assertEqual(self.launcher.activate('/refresh', 'close')['kind'], 'message')
            refresh.assert_not_called()

    def test_demo_close_never_touches_processes_or_store(self):
        self.launcher.demo = True
        self.launcher.store = None
        self.assertEqual(self.action()['action'], 'close')
        self.windows.assert_not_called()
        self.close_windows.assert_not_called()
        self.signal_process.assert_not_called()


class SignalTests(unittest.TestCase):
    def test_runtime_without_pidfds_uses_verified_system_python_helper(self):
        with patch.object(r, 'process_identity', return_value='boot:100'), \
                patch.object(r, 'hasattr', return_value=False, create=True), \
                patch.object(r.os.path, 'samefile', return_value=False), \
                patch.object(r.subprocess, 'run', return_value=Mock(returncode=0, stdout='true', stderr='')) as run:
            self.assertTrue(r.signal_process(10001, 'boot:100', signal.SIGTERM))
            argv = run.call_args.args[0]
            self.assertEqual(argv[0], '/usr/bin/python3')
            self.assertEqual(argv[-4:], ['_signal-close', '10001', 'boot:100', str(int(signal.SIGTERM))])

    def test_pid_reuse_after_open_does_not_signal(self):
        with patch.object(r, 'process_identity', side_effect=['boot:100', 'boot:200']), \
                patch.object(r.os, 'pidfd_open', return_value=42, create=True), \
                patch.object(r.os, 'close') as close, \
                patch.object(r.signal, 'pidfd_send_signal', create=True) as send:
            self.assertFalse(r.signal_process(10001, 'boot:100', signal.SIGTERM))
            send.assert_not_called()
            close.assert_called_once_with(42)

    def test_pidfd_signals_verified_process_and_closes_handle(self):
        with patch.object(r, 'process_identity', return_value='boot:100'), \
                patch.object(r.os, 'pidfd_open', return_value=42, create=True) as opened, \
                patch.object(r.os, 'close') as close, \
                patch.object(r.signal, 'pidfd_send_signal', create=True) as send:
            self.assertTrue(r.signal_process(10001, 'boot:100', signal.SIGTERM))
            opened.assert_called_once_with(10001)
            send.assert_called_once_with(42, signal.SIGTERM)
            close.assert_called_once_with(42)

    def test_stale_identity_never_opens_process_handle(self):
        with patch.object(r, 'process_identity', return_value=None), \
                patch.object(r.os, 'pidfd_open', create=True) as opened:
            self.assertFalse(r.signal_process(10001, 'boot:100', signal.SIGTERM))
            opened.assert_not_called()


class GracefulCloseTests(unittest.TestCase):
    def test_lua_and_legacy_send_window_close_requests(self):
        for lua in (True, False):
            with self.subTest(lua=lua):
                probe = Mock(returncode=0 if lua else 1, stdout='ok' if lua else '', stderr='')
                done = Mock(returncode=0, stdout='ok', stderr='')
                with patch.object(r.subprocess, 'run', side_effect=[probe, done]) as run:
                    r.close_windows([{'address': '0xabc'}])
                    command = run.call_args.args[0]
                    self.assertIn('address:0xabc', command[-1])
                    self.assertIn('window.close' if lua else 'closewindow', ' '.join(command))

    def test_invalid_address_is_rejected_before_dispatch(self):
        with patch.object(r.subprocess, 'run') as run:
            with self.assertRaises(ValueError):
                r.close_windows([{'address': 'invalid'}])
            run.assert_not_called()
