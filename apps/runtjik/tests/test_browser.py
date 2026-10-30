import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import browser as b
import browser_host as host
import runtjik as r


class BrowserTests(unittest.TestCase):
    def test_demo_filter_and_no_private_io(self):
        with patch.object(b, 'request', side_effect=AssertionError('private IO')):
            provider = b.Browser(demo=True)
            self.assertEqual(len(provider.query('/browser', {})), 2)
            self.assertEqual(len(provider.query('/browser HYPRLAND', {})), 1)
            rows = provider.query('/tabs FIREFOX', {})
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]['data']['tab'], 11)
            self.assertEqual(provider.activate(r.Entry(**rows[0]), {}, 'launch')['kind'], 'activated')

    def test_opt_in_and_alias(self):
        settings = r.config('')
        self.assertNotIn('Browser', settings['plugins']['enabled'])
        settings['plugins']['enabled'].append('Browser')
        with patch.object(r, 'config', return_value=settings):
            launcher = r.Launcher(demo=True)
        launcher._icons = Mock(resolve=lambda name: name)
        self.assertEqual(launcher.query('/zen')['results'], launcher.query('/browser')['results'])
        rows = launcher.query('/tabs Firefox')['results']
        self.assertEqual(len(rows), 1)
        self.assertEqual(launcher.activate(rows[0]['id'])['kind'], 'activated')
        for value in ('0', '7', 'true', '"3"'):
            with self.assertRaises(ValueError):
                r.config('[plugin_settings.Browser]\ncolumns=' + value)

    def test_tab_metadata_pin_filter_and_favicon_wrapper(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'fixture.sock').touch()
            state = {'windows': [{'id': 1, 'title': 'Do not repeat window title', 'tabs': [
                {'id': 1, 'title': 'Pinned', 'pinned': True},
                {'id': 2, 'title': 'Article', 'url': 'https://example.org/article',
                 'favicon': 'moz-remote-image://?url=https%3A%2F%2Fexample.org%2Ffavicon.png', 'audible': True},
                {'id': 3, 'title': 'Unsafe icon', 'favicon': 'file:///tmp/private'}]}]}
            with patch.object(b, 'runtime_directory', return_value=path), patch.object(b, 'request', return_value=state):
                rows = b.Browser().query('/tabs', {})
            self.assertEqual(len(rows), 2)
            self.assertEqual(rows[0]['subtitle'], 'example.org · Playing audio')
            self.assertEqual(rows[0]['data']['favicon'], 'https://example.org/favicon.png')
            self.assertEqual(rows[1]['data']['favicon'], '')

    def test_general_search_carries_favicon_and_keeps_actions_visible(self):
        settings = r.config('')
        settings['plugins']['enabled'] = ['Browser']
        with patch.object(r, 'config', return_value=settings):
            launcher = r.Launcher(demo=True)
        launcher._icons = Mock(resolve=lambda name: name)
        entry = {'id': 'Browser:demo:tab:99', 'provider': 'Browser', 'name': 'Synthetic website',
                 'data': {'favicon': 'data:image/png;base64,fixture', 'keep_open': True, 'tab': 99}}
        with patch.object(launcher.providers['Browser'], 'query', return_value=[entry]):
            rows = launcher.query('Synthetic website')['results']
        self.assertEqual(rows[0]['favicon'], entry['data']['favicon'])
        self.assertTrue(rows[0]['keep_open'])
        self.assertEqual(len(launcher.providers['Browser'].query('Firefox', settings)), 1)

    def test_close_window_routes_to_native_browser(self):
        provider = b.Browser()
        entry = r.Entry('Browser:fixture.sock:window:1', 'Window', provider='Browser',
                        data={'session': 'fixture.sock', 'window': 1, 'tab': None})
        with patch.object(b, 'runtime_directory', return_value=Path('/tmp')), patch.object(b, 'request', return_value={}) as request:
            result = provider.activate(entry, {}, 'close')
        self.assertEqual(request.call_args.args[1]['op'], 'close')
        self.assertIsNone(request.call_args.args[1]['tab'])
        self.assertEqual(result['refresh_query'], '/browser')

    def test_window_close_targets_exact_marked_client(self):
        calls = []
        def exchange(message):
            calls.append(message)
            if message['op'] == 'release':
                raise RuntimeError('Window closed')
            return {}
        client = {'address': '0x123', 'title': 'Marked window'}
        with patch.object(host, 'marked_window', return_value=client), patch.object(r, 'close_windows') as close:
            result = host.browser_action({'op': 'close', 'window': 4, 'tab': None}, exchange)
        close.assert_called_once_with([client])
        self.assertEqual([call['op'] for call in calls], ['focus', 'release'])
        self.assertIn('Hyprland', result['message'])

    def test_missing_compositor_match_does_not_close_other_windows(self):
        exchange = Mock(return_value={})
        with patch.object(host, 'marked_window', side_effect=RuntimeError('No match')), patch.object(r, 'close_windows') as close:
            with self.assertRaisesRegex(RuntimeError, 'No match'):
                host.browser_action({'op': 'close', 'window': 4, 'tab': None}, exchange)
        close.assert_not_called()
        self.assertEqual(exchange.call_args.args[0]['op'], 'release')

    def test_tab_close_stays_in_browser(self):
        exchange = Mock(return_value={})
        message = {'op': 'close', 'window': 4, 'tab': 9}
        with patch.object(r, 'close_windows') as close:
            host.browser_action(message, exchange)
        exchange.assert_called_once_with(message)
        close.assert_not_called()

    def test_shift_window_opens_filterable_tabs_without_focusing_browser(self):
        settings = r.config('')
        settings['plugins']['enabled'] = ['Browser']
        with patch.object(r, 'config', return_value=settings):
            launcher = r.Launcher(demo=True)
        launcher._icons = Mock(resolve=lambda name: name)
        window = launcher.query('/browser')['results'][0]
        with patch.object(launcher.providers['Browser'], 'tab_request') as request:
            action = launcher.activate(window['id'], 'switch')
            self.assertEqual(action, {'kind': 'edit', 'text': '/tabs '})
            rows = launcher.query(action['text'])
            self.assertTrue(rows['browser_tab_scope'])
            self.assertEqual(len(rows['results']), 2)
            self.assertEqual(len(launcher.query('/tabs Firefox')['results']), 1)
            self.assertEqual(launcher.query('/tabs reading')['results'], [])
            request.assert_not_called()
        launcher.query('/browser')
        self.assertEqual(len(launcher.query('/tabs')['results']), 3)
        self.assertFalse(launcher.query('/tabs')['browser_tab_scope'])

    def test_native_framing_limits_and_truncation(self):
        data = io.BytesIO()
        host.write_message(data, {'title': '山水'})
        data.seek(0)
        self.assertEqual(host.read_message(data), {'title': '山水'})
        with self.assertRaises(ValueError):
            host.read_message(io.BytesIO(struct.pack('=I', host.MAX_MESSAGE + 1)))
        with self.assertRaises(EOFError):
            host.read_message(io.BytesIO(struct.pack('=I', 10) + b'{}'))

    def test_focus_uses_unique_marker_not_page_title(self):
        with patch.object(r, 'windows', return_value=[{'title': 'Same page', 'address': '0x1'},
                         {'title': '[marker] Same page', 'address': '0x2'}]), patch.object(r, 'focus_window') as focus:
            host.focus_marker('[marker] ')
        focus.assert_called_once_with({'title': '[marker] Same page', 'address': '0x2'})

    def test_native_host_roundtrip_and_disconnect_cleanup(self):
        with tempfile.TemporaryDirectory(prefix='runtjik-browser-test-') as directory:
            env = {**os.environ, 'XDG_RUNTIME_DIR': directory}
            process = subprocess.Popen([sys.executable, '-B', str(Path(host.__file__))],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
            try:
                deadline = time.monotonic() + 3
                paths = []
                while not paths and time.monotonic() < deadline:
                    paths = list(Path(directory).glob('runtjik-browser/*.sock'))
                    time.sleep(0.01)
                self.assertEqual(len(paths), 1)
                self.assertEqual(paths[0].stat().st_mode & 0o077, 0)
                import concurrent.futures
                with concurrent.futures.ThreadPoolExecutor() as executor:
                    future = executor.submit(b.request, paths[0], {'op': 'list'})
                    message = host.read_message(process.stdout)
                    self.assertEqual(message['op'], 'list')
                    host.write_message(process.stdin, {'id': message['id'], 'windows': []})
                    self.assertEqual(future.result(timeout=3)['windows'], [])
                process.stdin.close()
                self.assertEqual(process.wait(timeout=3), 0)
                self.assertFalse(paths[0].exists())
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                process.stdout.close()
                process.stderr.close()
                if not process.stdin.closed:
                    process.stdin.close()


if __name__ == '__main__':
    unittest.main()
