
# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('tray_focus', Path(__file__).resolve().parents[1] / 'tests/reference/qml/tray_focus.py')
focus = importlib.util.module_from_spec(spec)
spec.loader.exec_module(focus)


class TrayFocusTests(unittest.TestCase):
    def setUp(self):
        self.client = dict(address='0xabc', monitor=0, workspace={'name':'special:codex'},
                           mapped=True, **{'class':'Chatgpt'})
        self.monitors = [dict(id=0, name='DP-1', specialWorkspace={'name':''})]

    def test_reveal_hidden_scratch_and_focus_without_toggling_visible_scratch(self):
        commands=focus.focus_commands(self.client,self.monitors)
        self.assertEqual(commands[1][-1], 'hl.dsp.workspace.toggle_special("codex")')
        self.assertEqual(commands[-1][-1], 'hl.dsp.focus({ window = "address:0xabc" })')
        self.monitors[0]['specialWorkspace']['name']='special:codex'
        self.assertEqual(len(focus.focus_commands(self.client,self.monitors)),2)

    def test_class_match_and_recent_window(self):
        unrelated={**self.client,'class':'Chromium','focusHistoryID':0}
        recent={**self.client,'focusHistoryID':1}
        older={**self.client,'focusHistoryID':2}
        self.assertIs(focus.select_client('chatgpt',[unrelated,older,recent]),recent)
        telegram={**self.client,'class':'org.telegram.desktop._123'}
        self.assertIs(focus.select_client('telegram',[telegram]),telegram)
        self.assertIsNone(focus.select_client('chatgpt',[{**self.client,'mapped':False}]))

    def test_existing_window_never_calls_native_toggle(self):
        with patch.object(focus,'snapshot',side_effect=[[self.client],self.monitors,{'address':'0xabc'}]), patch.object(focus,'activate') as activate, patch.object(focus.subprocess,'run'):
            focus.main('chatgpt')
        activate.assert_not_called()

    def test_missing_window_activates_then_focuses(self):
        with patch.object(focus,'snapshot',side_effect=[[],[self.client],self.monitors,{'address':'0xabc'}]), patch.object(focus,'activate') as activate, patch.object(focus.time,'sleep'), patch.object(focus.subprocess,'run'):
            focus.main('chatgpt')
        activate.assert_called_once_with('chatgpt')

    def test_reject_invalid_address(self):
        with self.assertRaises(ValueError):focus.focus_commands({**self.client,'address':'"; evil()'},self.monitors)
