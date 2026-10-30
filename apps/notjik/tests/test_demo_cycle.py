import contextlib
import io
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
import comparison
import demo
import demo_cycle


class CycleTests(unittest.TestCase):
    def test_requested_order_and_plan_needs_no_desktop(self):
        expected = ('standard', 'icon', 'status', 'error', 'timer', 'actions-multiple',
                    'urgency', 'update', 'grouped', 'telegram-text', 'telegram-photo',
                    'telegram-voice', 'telegram-gif', 'robots')
        self.assertEqual(demo_cycle.SCENES, expected)
        with (patch.object(demo_cycle, 'output_monitor') as monitor,
              contextlib.redirect_stdout(io.StringIO()) as output):
            self.assertEqual(demo.main(ROOT, ['cycle', '--plan']), 0)
        monitor.assert_not_called()
        self.assertEqual(len(output.getvalue().splitlines()), 14)

    def test_private_copy_disables_account_and_microphone_helpers(self):
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory)
            shutil.copytree(ROOT / 'qml', app / 'qml',copy_function=lambda src,dst: shutil.copy2(src,dst) if Path(src).suffix in ('.qml','.js','.json') else os.symlink(Path(src).resolve(),dst))
            source = (ROOT / 'qml/TelegramRecorder.qml').read_bytes()
            demo_cycle.prepare(app)
            self.assertEqual((ROOT / 'qml/TelegramRecorder.qml').read_bytes(), source)
            for helper in ('telegram_bridge.py', 'telegram_recording.py'):
                self.assertEqual((app/helper).read_text(),
                    'raise SystemExit("Real Telegram and microphone access are disabled in the demo cycle")\n')
            recording = (app/'qml/TelegramRecorder.qml').read_text()
            self.assertIn('Only synthetic demo recordings are allowed', recording)
            self.assertIn('phase = "sending"; cycleSend.restart(); return;', recording)
            self.assertIn('DemoStage { controller: root }', (app/'qml/shell.qml').read_text())
            self.assertTrue((app/'qml/DemoStage.qml').is_file())

    def test_robot_finale_has_four_of_each_style(self):
        images = demo_cycle.selected_robots(ROOT)
        self.assertEqual(len(set(images)), 12)
        self.assertEqual(sum('-alert-' in p.name for p in images), 4)
        self.assertEqual(sum('-dream-' in p.name for p in images), 4)
        self.assertTrue(all(p.is_file() for p in images))

    def test_introduction_covers_cards_and_fades_before_comparison(self):
        cycle = demo_cycle.Cycle.__new__(demo_cycle.Cycle)
        cycle.root = ROOT
        events = []
        cycle.control = lambda **data: events.append(data)
        cycle.pause = lambda seconds: None
        with patch.object(demo_cycle.time, 'sleep'):
            cycle.introduction()
        cards = events[0]['introCards']
        self.assertEqual(len(cards), 15)
        self.assertEqual(len({n['image'] for n in cards}), 12)
        self.assertTrue(all(n['data'].get('demo') for n in cards if n['type']=='telegram'))
        self.assertTrue(events[-1]['intro'])

    def test_existing_recording_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)/'demo.mp4'
            target.write_bytes(b'keep')
            with (contextlib.redirect_stderr(io.StringIO()),
                  patch.object(demo_cycle, 'output_monitor') as monitor,
                  self.assertRaises(SystemExit)):
                demo_cycle.main(ROOT, ['record', str(target)])
            monitor.assert_not_called()
            self.assertEqual(target.read_bytes(), b'keep')

    def test_cycle_session_does_not_reuse_normal_comparison(self):
        with patch.dict(os.environ, {'NOTJIK_DEMO_CYCLE_DIR':'/tmp/isolated-cycle'}):
            self.assertEqual(comparison.runtime_dir(), Path('/tmp/isolated-cycle'))

    def test_every_scene_has_intro_and_outro_and_only_safe_fixtures(self):
        for name in demo_cycle.SCENES:
            with self.subTest(scene=name):
                events = []
                class Pair:
                    def clear(self): events.append(('clear',))
                    def __call__(self, request):
                        notice = request['notice']
                        events.append(('send', notice))
                        return {'id':notice['id']}
                cycle = demo_cycle.Cycle.__new__(demo_cycle.Cycle)
                cycle.root, cycle.session, cycle.speed = ROOT, {'app':str(ROOT)}, 1
                cycle.paired = Pair()
                cycle.timeline, cycle.started = [], 0
                cycle.pause = lambda seconds: events.append(('pause',seconds))
                cycle.control = lambda **data: events.append(('control',data)) or {'recording':'recording'}
                with patch.object(comparison, 'set_grouping'), patch.object(demo_cycle.time, 'sleep'), patch('notjik.ipc', return_value={'drawerOpen':True,'historyCount':3}):
                    cycle.scene(name, 1, 13)
                self.assertEqual(events[0], ('clear',))
                self.assertEqual(events[-2], ('clear',))
                self.assertEqual(events[-1], ('control', {'closeDrawer':True}))
                self.assertEqual(cycle.timeline[0]['scene'], name)
                for event in events:
                    if event[0] == 'send':
                        notice = event[1]
                        self.assertEqual(notice['app'], demo.APP)
                        self.assertTrue(notice['transient'])
                        if notice.get('type') == 'telegram': self.assertTrue(notice['data']['demo'])
                        for action in notice.get('actions',[]): self.assertEqual(action['command'], ['true'])
                targets = [event[1].get('target') for event in events if event[0]=='control']
                if name=='actions-multiple':
                    self.assertIn('cycle-action-0',targets)
                    self.assertIn('cycle-action-1',targets)
                if name=='telegram-voice':
                    self.assertIn('telegram-play',targets)
                    self.assertIn('telegram-record',targets)
                    self.assertIn('telegram-record-send',targets)
