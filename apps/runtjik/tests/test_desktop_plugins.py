import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch, Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import runtjik as r
import desktop_plugins as d
from capture_worker import recorder_argv


class DesktopPluginTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        env = patch.dict(os.environ, RUNTJIK_CONFIG=str(self.root / 'config'), XDG_RUNTIME_DIR=str(self.root), XDG_CONFIG_HOME=str(self.root / 'config-home'), XDG_CACHE_HOME=str(self.root / 'cache'))
        env.start()
        self.addCleanup(env.stop)
        self.settings = r.config()
        for name in ('Recorder', 'Screenshots'):
            self.settings['plugin_settings'][name]['directory'] = str(self.root / name)

    def test_aliases_are_deduplicated_and_route(self):
        launcher = r.Launcher(demo=True)
        launcher._icons = Mock(resolve=lambda name: name)
        menu = launcher.query('/')['results']
        self.assertEqual(len(menu), 14)
        self.assertNotIn('/shot', {row['id'] for row in menu})
        for alias, canonical in (('/shot', '/grab'), ('/screenshot', '/grab'), ('/screenrecord', '/rec')):
            self.assertEqual(launcher.query(alias)['results'], launcher.query(canonical)['results'])
            self.assertIn(canonical, {row['id'] for row in launcher.query(alias[:4])['results']})
        row = next(row for row in launcher.query('/grab')['results'] if row['name'] == 'Screen')
        self.assertEqual(launcher.activate(row['id']), {'kind': 'edit', 'text': '/grab screen'})
        row = launcher.query('/grab screen')['results'][0]
        self.assertTrue(row['hide_launcher'])

    def test_demo_does_not_read_private_history_or_capture(self):
        with patch.object(d, 'run', side_effect=AssertionError('private read')), patch.object(d, 'recorder_request', side_effect=AssertionError('private recorder')):
            for provider, command in ((d.Clipboard(True), '/cliphist'), (d.Recorder(True), '/rec'), (d.Screenshots(True), '/grab')):
                rows = provider.query(command, self.settings)
                self.assertTrue(rows)
                for row in rows:
                    provider.activate(r.Entry(**row), self.settings, 'launch')
                if command != '/cliphist':
                    self.assertTrue(provider.query(command + ' history', self.settings))

    def test_clipboard_filter_and_binary_copy(self):
        provider = d.Clipboard()
        with patch.object(d, 'run', return_value=b'42\t[[ binary data png ]]\n43\ta note\n'):
            rows = provider.query('/cliphist binary', self.settings)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]['data']['clip_id'], '42')
        with patch.object(d, 'run', side_effect=[b'\x89PNG\x00', b'']) as run:
            result = provider.activate(r.Entry(**rows[0]), self.settings, 'launch')
        self.assertEqual(result['kind'], 'activated')
        self.assertEqual(run.call_args_list[0].kwargs['input'], b'42')
        self.assertEqual(run.call_args_list[1].kwargs['input'], b'\x89PNG\x00')

    def test_history_newest_first_and_no_symlinks(self):
        folder = Path(self.settings['plugin_settings']['Screenshots']['directory'])
        folder.mkdir()
        old, new = folder / 'old.png', folder / 'new.png'
        old.write_bytes(b'old')
        new.write_bytes(b'new')
        os.utime(old, (1, 1))
        (folder / 'external.png').symlink_to(old)
        rows = d.Screenshots().query('/screenshot history', self.settings)
        self.assertEqual([row['name'] for row in rows], ['new.png', 'old.png'])
        self.assertEqual(d.Screenshots().query('/shot history new', self.settings)[0]['name'], 'new.png')

    def test_window_and_workspace_targets_revalidate(self):
        provider = d.Screenshots()
        monitor = {'name': 'DP-1', 'activeWorkspace': {'id': 2, 'name': '2'}}
        with patch.object(d, 'hypr', return_value=[monitor]), patch.object(d.shutil, 'which', return_value='/tool'), patch.object(d, 'recorder_request', return_value=None):
            self.assertEqual(provider.query('/grab workspace', self.settings), [])
            recorder = d.Recorder()
            rows = recorder.query('/rec workspace', self.settings)
            self.assertEqual(rows[0]['data']['workspace'], 2)
            monitor['activeWorkspace']['id'] = 3
            with self.assertRaisesRegex(ValueError, 'Workspace changed'):
                recorder.activate(r.Entry(**rows[0]), self.settings, 'launch')
        with patch.object(d, 'hypr', return_value=[]), patch.object(d.shutil, 'which', return_value='/tool'):
            with self.assertRaisesRegex(ValueError, 'closed'):
                provider.capture({'capture': 'window', 'target': '0x123'}, self.settings['plugin_settings']['Screenshots'])

    def test_region_cancel_does_not_create_file(self):
        with patch.object(d, 'selection', return_value=None), patch.object(d.shutil, 'which', return_value='/tool'):
            result = d.Screenshots().capture({'capture': 'region'}, self.settings['plugin_settings']['Screenshots'])
        self.assertEqual(result['message'], 'Capture cancelled')
        self.assertFalse((self.root / 'Screenshots').exists())

    def test_screenshot_args_clipboard_and_forced_close(self):
        calls = []
        def run(args, **kwargs):
            calls.append((args, kwargs))
            if args[0] == 'grim':
                Path(args[-1]).write_bytes(b'PNG fixture')
            return b''
        with patch.object(d, 'selection', return_value='-100,20 200x100'), patch.object(d.shutil, 'which', return_value='/tool'), patch.object(d, 'run', side_effect=run):
            result = d.Screenshots().capture({'capture': 'region'}, self.settings['plugin_settings']['Screenshots'])
        self.assertTrue(result['close_launcher'])
        self.assertEqual(calls[0][0][:3], ['grim', '-g', '-100,20 200x100'])
        self.assertEqual(calls[1][1]['input'], b'PNG fixture')

    def test_recording_commands_and_stop_are_scoped(self):
        spec = {'mode': 'region', 'area': '0,0 100x100', 'output': None, 'fps': 30, 'audio': False, 'file': '/tmp/demo.mp4'}
        args = recorder_argv(spec)
        self.assertEqual(args[0], 'wf-recorder')
        self.assertNotIn('-a', args)
        self.assertIn('0,0 100x100', args)
        with patch('capture_worker.subprocess.check_output', return_value='test-output\n'):
            self.assertIn('--audio=test-output.monitor', recorder_argv({**spec, 'audio': True}))
        args = recorder_argv({**spec, 'mode': 'window', 'audio': True})
        self.assertEqual(args[0], 'gpu-screen-recorder')
        self.assertIn('portal', args)
        self.assertIn('default_output', args)
        with patch.object(d, 'recorder_request', return_value={'file': '/tmp/demo.mp4'}) as request:
            result = d.Recorder().activate(r.Entry(**d.Recorder().row('stop', 'Stop', stop=True)), self.settings, 'launch')
            request.assert_called_once_with('stop')
            self.assertIn('Stopping', result['message'])

    def test_capture_config_validation_and_xdg_resolution(self):
        for text in ('[plugin_settings.Recorder]\nfps=0', '[plugin_settings.Screenshots]\ndirectory="relative"', '[plugin_settings.Clipboard]\nmax_results=0'):
            with self.assertRaises(ValueError):
                r.config(text)
        with self.assertRaises(ValueError):
            d.capture_directory(str(Path(__file__).resolve().parents[3] / 'captures'))
        directory = Path(os.environ['XDG_CONFIG_HOME'])
        directory.mkdir()
        (directory / 'user-dirs.dirs').write_text('XDG_VIDEOS_DIR="' + str(self.root / 'custom videos') + '"\n')
        self.assertEqual(d.capture_directory('xdg:VIDEOS'), self.root / 'custom videos')

    def test_settings_panel_has_all_plugins(self):
        launcher = r.Launcher(demo=True)
        launcher._icons = Mock(resolve=lambda name: name)
        self.assertEqual(len(launcher.query('/config plugin_settings.Recorder')['results']), 7)
        self.assertEqual(len(launcher.query('/config plugin_settings.Screenshots')['results']), 6)
        self.assertEqual(len(launcher.query('/config plugin_settings.Clipboard')['results']), 1)

    def test_config_migration_preserves_customizations_and_is_idempotent(self):
        path = r.config_path()
        source = '# personal\n[ui]\nwidth = 900\n[plugins]\nenabled = ["Example"] # keep\n[plugin_settings.Recorder]\naudio = true\ndirectory = "~/My Videos"\n'
        path.write_text(source)
        path.chmod(0o640)
        result = r.migrate_config(True)
        self.assertEqual(Path(result['backup']).read_text(), source)
        self.assertEqual(path.stat().st_mode & 0o777, 0o640)
        settings = r.config()
        self.assertEqual(settings['ui']['width'], 900)
        self.assertEqual(settings['plugins']['enabled'], ['Example', *r.BUNDLED_PROVIDERS])
        self.assertTrue(settings['plugin_settings']['Recorder']['audio'])
        self.assertEqual(settings['plugin_settings']['Recorder']['directory'], '~/My Videos')
        self.assertIn('# keep', path.read_text())
        migrated = path.read_text()
        self.assertFalse(r.migrate_config(True)['changed'])
        self.assertEqual(path.read_text(), migrated)

    def test_config_migration_does_not_enable_disabled_plugins_implicitly(self):
        r.config_path().write_text('[plugins]\nenabled = []\n')
        r.migrate_config()
        self.assertEqual(r.config()['plugins']['enabled'], [])

    def test_recording_countdown_cancels_before_recording(self):
        options = self.settings['plugin_settings']['Recorder']
        with patch.object(d.shutil, 'which', return_value='/tool'), patch.object(d, 'recorder_request', return_value=None), patch.object(d, 'selection', return_value='0,0 100x100'), patch.object(d.subprocess, 'run', return_value=Mock(returncode=2)) as run, patch.object(d.subprocess, 'Popen') as start:
            result = d.Recorder().capture({'capture': 'region'}, options)
            self.assertEqual(result['message'], 'Recording cancelled')
            start.assert_not_called()
            self.assertIn('capture_countdown.py', run.call_args.args[0][2])

    def test_history_contains_thumbnail_paths_and_grid_settings(self):
        provider = d.Screenshots(demo=True)
        rows = provider.query('/grab history', self.settings)
        self.assertEqual(len(rows), 9)
        self.assertTrue(all(Path(row['data']['thumbnail']).exists() for row in rows))
        self.assertEqual(self.settings['plugin_settings']['Screenshots']['gallery_columns'], 3)
        self.assertEqual(self.settings['plugin_settings']['Screenshots']['gallery_rows'], 3)

    def test_monitor_choices_sort_by_name(self):
        monitors = [{'name': name, 'activeWorkspace': {'name': name, 'id': index}} for index, name in enumerate(['DP-2', 'DP-1'])]
        with patch.object(d, 'hypr', return_value=monitors):
            rows = d.Screenshots().query('/ss screen', self.settings)
            self.assertEqual([row['name'] for row in rows], ['DP-1', 'DP-2'])
        self.assertEqual(d.Screenshots.aliases['/snap'], '/grab')

    def test_region_selector_has_no_inherited_stdin(self):
        with patch.object(d.subprocess, 'run', return_value=Mock(returncode=0, stdout=b'10,20 300x200\n', stderr=b'')) as run:
            self.assertEqual(d.selection(), '10,20 300x200')
            self.assertEqual(run.call_args.kwargs['stdin'], d.subprocess.DEVNULL)
        with patch.object(d.subprocess, 'run', return_value=Mock(returncode=1, stdout=b'', stderr=b'protocol unavailable')):
            with self.assertRaisesRegex(RuntimeError, 'protocol unavailable'):
                d.selection()

    def test_control_enter_routes_capture_to_trash(self):
        launcher = r.Launcher(demo=True)
        launcher._icons = Mock(resolve=lambda name: name)
        provider = launcher.providers['Screenshots']
        provider.demo = False
        options = launcher.settings['plugin_settings']['Screenshots']
        options['directory'] = str(self.root)
        image = self.root / 'example.png'
        image.write_bytes(b'synthetic')
        row = launcher.query('!snap history')['results'][0]
        with patch.object(d, 'trash_action', return_value={'message': 'Moved to Trash'}) as trash:
            result = launcher.activate(row['id'], 'close')
            trash.assert_called_once_with('trash', image)
            self.assertEqual(result['query'], '!snap history')
            self.assertEqual(result['message'], 'Moved to Trash')

    def test_command_history_is_bounded_deduplicated_and_persistent(self):
        path = self.root / 'history.db'
        store = r.Store(path)
        for index in range(110):
            store.remember_command('!define word' + str(index))
        store.remember_command('!define word50')
        self.assertEqual(len(store.command_history()), 100)
        self.assertEqual(store.command_history()[-1], '!define word50')
        store.db.close()
        store = r.Store(path)
        self.assertEqual(store.command_history()[-1], '!define word50')
        store.db.close()
