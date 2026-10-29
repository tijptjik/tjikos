import os
from pathlib import Path
import sys
import tempfile
import time
from threading import Event
import unittest
from unittest.mock import patch, Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import runtjik as r
import words


class WordTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'runtjik.conf'
        env = patch.dict(os.environ, RUNTJIK_CONFIG=str(self.path))
        env.start()
        self.addCleanup(env.stop)
        self.launcher = r.Launcher(demo=True)
        self.launcher._icons = Mock(resolve=lambda name: name)

    def test_inline_results_publish_without_waiting_for_slow_source(self):
        provider = self.launcher.providers['Words']
        provider.demo = False
        slow = Event()
        def lookup(source, *_):
            if source == 'wiktionary':
                slow.wait(2)
                raise TimeoutError('timed out')
            return [('A lucky discovery', 'Example: A lucky meeting.', 'https://en.wiktionary.org/wiki/luck')]
        with patch.object(words, 'lookup', side_effect=lookup):
            self.launcher.query('/define luck')
            for _ in range(100):
                result = self.launcher.query('/define luck')
                if any(row['name'] == 'A lucky discovery' for row in result['results']):
                    break
                time.sleep(.01)
            self.assertTrue(result['pending'])
            self.assertEqual(result['results'][0]['name'], 'A lucky discovery')
            self.assertTrue(result['results'][0]['keep_open'])
            slow.set()
            for _ in range(100):
                result = self.launcher.query('/define luck')
                if not result['pending']:
                    break
                time.sleep(.01)
            self.assertFalse(result['pending'])
            self.assertEqual(result['results'][1]['name'], 'Source unavailable')
        self.assertEqual(self.launcher.query('/running')['results'], [])

    def test_demo_never_fetches(self):
        with patch.object(words, 'fetch', side_effect=AssertionError('network')):
            result = self.launcher.query('/define demo')
            self.assertIn('Synthetic dictionary', result['results'][0]['subtitle'])
            self.assertTrue(result['results'][0]['keep_open'])

    def test_bang_command_menu_and_double_bang_fish(self):
        menu = self.launcher.query('!')
        self.assertEqual(menu['query'], '!')
        self.assertIn('/define', {row['id'] for row in menu['results']})
        result = self.launcher.query('!define demo')
        self.assertEqual(result['query'], '!define demo')
        self.assertEqual(result['results'][0]['provider'], 'Words')
        self.launcher.entries = [r.Entry('commands:fish', 'fish', provider='commands')]
        self.launcher.checked = time.monotonic()
        self.assertEqual(self.launcher.query('!!fish')['results'][0]['id'], 'commands:fish')

    def test_nested_settings_multiline_arrays_roundtrip(self):
        self.path.write_text('# preserved\n[plugin_settings.Words]\ndefine_sources = [\n "wiktionary",\n]\n# after\ntimeout_seconds = 3\n')
        settings = r.save_setting('plugin_settings.Words.define_sources', '["urban_dictionary"]')
        self.assertEqual(settings['plugin_settings']['Words']['define_sources'], ['urban_dictionary'])
        self.assertEqual(settings['plugin_settings']['Words']['timeout_seconds'], 3)
        self.assertIn('# after', self.path.read_text())
        original = self.path.read_text()
        with self.assertRaises(ValueError):
            r.save_setting('plugin_settings.Words.define_sources', '["unknown"]')
        self.assertEqual(original, self.path.read_text())
        r.save_setting('plugins.enabled', '[]')
        self.assertNotIn('/define', r.Launcher(demo=True).plugin_commands)

    def test_settings_panel_reads_plugin_values(self):
        rows = self.launcher.query('/config plugin_settings.Words')['results']
        self.assertEqual(len(rows), 4)
        edit = self.launcher.activate(rows[0]['id'])
        self.assertIn('["free_dictionary", "wiktionary"]', edit['text'])

    def test_source_parsers_keep_examples_and_synonyms(self):
        with patch.object(words, 'fetch', return_value=[{'meanings': [{'partOfSpeech': 'noun', 'synonyms': ['luck'], 'definitions': [{'definition': 'Chance', 'example': 'By chance.', 'synonyms': ['fortune']}]}]}]):
            rows = words.lookup('free_dictionary', '/define', 'chance', 1, 12)
            self.assertIn('By chance.', rows[0][1])
            self.assertEqual([v[0] for v in words.lookup('free_dictionary', '/synonym', 'chance', 1, 12)], ['luck', 'fortune'])
        with patch.object(words, 'fetch', return_value={'en': [{'partOfSpeech': 'Noun', 'definitions': [{'definition': '<b>Chance</b>', 'examples': ['By <i>chance</i>.']}]}]}):
            row = words.lookup('wiktionary', '/define', 'chance', 1, 12)[0]
            self.assertEqual(row[0], 'Chance')
            self.assertIn('By chance.', row[1])
        with patch.object(words, 'fetch', return_value={'list': [{'definition': '[Cool]', 'example': 'So [cool]', 'defid': 123}]}):
            row = words.lookup('urban_dictionary', '/define', 'cool', 1, 12)[0]
            self.assertEqual(row[0], 'Cool')
            self.assertIn('defid=123', row[2])

    def test_generic_plugin_settings_are_editable(self):
        self.path.write_text('[plugin_settings.Example]\nlimit = 3\n')
        self.assertIn('plugin_settings.Example.limit', r.featured_settings(r.config()))
        self.assertEqual(r.save_setting('plugin_settings.Example.limit', '5')['plugin_settings']['Example']['limit'], 5)
