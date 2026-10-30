
# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / 'tests/reference/qml/media_play.py'
spec = importlib.util.spec_from_file_location('media_play', SOURCE)
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)


class MediaPlayTests(unittest.TestCase):
    def test_resume_does_not_launch_another_spotify(self):
        with patch.object(media, 'spotify_running', return_value=True), \
             patch.object(media.subprocess, 'Popen') as launch, \
             patch.object(media.subprocess, 'run') as run:
            media.play()
            launch.assert_not_called()
            self.assertEqual(run.call_args.args[0], ['playerctl', '--player=spotify', 'play'])

    def test_closed_spotify_waits_before_play(self):
        with patch.object(media, 'spotify_running', side_effect=[False, False, True]), \
             patch.object(media.shutil, 'which', return_value=None), \
             patch.object(media.subprocess, 'Popen') as launch, \
             patch.object(media.subprocess, 'run') as run, \
             patch.object(media.time, 'sleep') as sleep:
            media.play()
            self.assertEqual(launch.call_args.args[0], ['flatpak', 'run', 'com.spotify.Client'])
            sleep.assert_called_once_with(0.5)
            run.assert_called_once()

    def test_unavailable_spotify_reports_failure(self):
        with patch.object(media, 'spotify_running', return_value=False), \
             patch.object(media.subprocess, 'Popen'), \
             patch.object(media.subprocess, 'run') as run, \
             patch.object(media.time, 'sleep'):
            with self.assertRaisesRegex(RuntimeError, '30 seconds'):
                media.play()
            run.assert_not_called()
