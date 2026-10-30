"""Persistent DND storage; isolated from the real notification state."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tests/reference"))
import notjik


class PausePreferenceTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.directory = Path(temp.name) / 'state'
        env = patch.dict(os.environ, NOTJIK_STATE_DIR=str(self.directory))
        env.start()
        self.addCleanup(env.stop)

    def test_default_and_both_saved_modes(self):
        self.assertFalse(notjik.load_paused())
        for value in (True, False):
            notjik.save_paused(value)
            self.assertEqual(notjik.load_paused(), value)
            self.assertEqual(json.loads((self.directory/'preferences.json').read_text()), {'paused': value})

    def test_configuration_changes_do_not_reset_preference(self):
        notjik.save_paused(True)
        with patch.dict(os.environ, NOTJIK_CONFIG='/nonexistent/notjik-config.json'):
            self.assertTrue(notjik.load_paused())

    def test_invalid_state_is_not_treated_as_resume(self):
        self.directory.mkdir()
        for value in ('{"paused":"yes"}', '{bad', '[]'):
            (self.directory/'preferences.json').write_text(value)
            with self.assertRaises(ValueError):
                notjik.load_paused()

    def test_invalid_write_preserves_previous_choice(self):
        notjik.save_paused(True)
        with self.assertRaises(ValueError):
            notjik.save_paused(0)
        self.assertTrue(notjik.load_paused())

    def test_failed_replace_preserves_previous_choice_and_cleans_temporary(self):
        notjik.save_paused(True)
        with patch.object(notjik.os, 'replace', side_effect=OSError('disk error')):
            with self.assertRaises(OSError):
                notjik.save_paused(False)
        self.assertTrue(notjik.load_paused())
        self.assertEqual([p.name for p in self.directory.iterdir()], ['preferences.json'])


if __name__ == '__main__':
    unittest.main()
