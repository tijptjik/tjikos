"""Media artwork meters playback without opening a microphone."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import MagicMock, patch

spec=importlib.util.spec_from_file_location('audio_meter',Path(__file__).resolve().parents[1]/'tests/reference/qml/audio_levels.py')
meter=importlib.util.module_from_spec(spec)
spec.loader.exec_module(meter)


class MeterTests(unittest.TestCase):
    def test_playback_only_never_queries_or_opens_input_sources(self):
        selector=MagicMock()
        selector.select.side_effect=SystemExit
        def query(*args):
            if args==('get-default-sink',):return 'demo-output'
            if args[-1]=='sources':self.fail('Playback meter queried microphone sources')
            return '[]'
        with (patch.object(meter.selectors,'DefaultSelector',return_value=selector),
              patch.object(meter,'query',side_effect=query),
              patch.object(meter.signal,'signal'),
              patch.object(meter.subprocess,'Popen') as process,
              self.assertRaises(SystemExit)):
            meter.watch(include_microphone=False)
        process.assert_not_called()
        selector.close.assert_called_once()


if __name__=='__main__':unittest.main()
