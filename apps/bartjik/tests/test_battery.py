
# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import tempfile
import unittest
from unittest.mock import AsyncMock, patch
from pathlib import Path

from bartjik_core.battery import snapshot
from bartjik_core.model import validate_items
from bartjik_core import battery
from bartjik_core.actions import Executor
from bartjik_core.io import Unavailable


class BatteryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def supply(self, name, **values):
        path = self.root / name
        path.mkdir(exist_ok=True)
        for key, value in values.items():
            (path / key).write_text(str(value))
        return path

    def status(self):
        return validate_items(snapshot(self.root))

    def test_desktop_and_peripherals_stay_hidden(self):
        self.assertEqual(self.status(), [])
        self.supply('mouse', type='Battery', scope='Device', capacity=80)
        self.supply('AC', type='Mains', online=1)
        self.supply('BAT0', type='Battery', present=0, capacity=75)
        self.assertEqual(self.status(), [])

    def test_ac_full_not_charging_unplug_and_hot_remove(self):
        path = self.supply('BAT0', type='Battery', capacity=100, status='Full')
        self.supply('AC', type='Mains', online=1)
        self.assertTrue(self.status()[0]['battery']['external'])
        self.supply('BAT0', status='Not charging', capacity=80)
        self.assertTrue(self.status()[0]['battery']['external'])
        self.supply('AC', online=0)
        row = self.status()[0]
        self.assertEqual(row['battery'], dict(percent=80, external=False))
        self.assertEqual(row['tooltip'], 'Battery · 80%')
        for file in path.iterdir():
            file.unlink()
        path.rmdir()
        self.assertEqual(self.status(), [])

    def test_charging_fallback_missing_invalid_and_clamped_capacity(self):
        self.supply('BAT0', type='Battery', status='Charging')
        self.assertEqual(self.status(), [])
        for value in ('bad', 'nan', 'inf', -1):
            self.supply('BAT0', capacity=value)
            self.assertEqual(self.status(), [])
        for value, expected in ((0, 0), (1, 1), (101, 100)):
            self.supply('BAT0', capacity=value)
            self.assertEqual(self.status()[0]['battery'], dict(percent=expected, external=True))

    def test_multiple_batteries_weight_by_energy(self):
        self.supply('BAT0', type='Battery', energy_now=10, energy_full=20)
        self.supply('BAT1', type='Battery', energy_now=72, energy_full=80)
        self.assertEqual(self.status()[0]['battery']['percent'], 82)


    def test_charge_fallback_and_mixed_units(self):
        self.supply('BAT0', type='Battery', charge_now=1, charge_full=2)
        self.assertEqual(self.status()[0]['battery']['percent'], 50)
        self.supply('BAT1', type='Battery', energy_now=72, energy_full=80)
        self.assertEqual(self.status()[0]['battery']['percent'], 70)
        self.supply('BAT0', voltage_min_design=10000000)
        self.assertEqual(self.status()[0]['battery']['percent'], 82)


class BatteryPanelTests(unittest.IsolatedAsyncioTestCase):
    def profiles(self, active='balanced'):
        return {'ActiveProfile': active, 'Profiles': [
            {'Profile': {'data': key}} for key, _ in battery.PROFILES]}

    def test_estimates_change_direction_and_ignore_stale_or_unknown(self):
        data = dict(percent=60, external=True)
        panel = battery.panel_data(data, dict(State=1, TimeToFull=3600), self.profiles())
        self.assertEqual(panel['seconds'], 3600)
        self.assertEqual(panel['estimateLabel'], 'Until fully charged')
        self.assertEqual(sum(p['active'] for p in panel['profiles']), 1)
        data['external'] = False
        self.assertIsNone(battery.panel_data(data, dict(State=1, TimeToFull=3600), {})['seconds'])
        panel = battery.panel_data(data, dict(State=2, TimeToEmpty=7200), {})
        self.assertEqual(panel['seconds'], 7200)
        self.assertEqual(panel['estimateLabel'], 'Battery life remaining')
        for value in (0, -1, None, float('nan')):
            self.assertIsNone(battery.panel_data(data, dict(State=2, TimeToEmpty=value), {})['seconds'])
        self.assertTrue(all(not p['available'] for p in panel['profiles']))
        self.assertEqual(battery.panel_data(dict(percent=100, external=True), dict(State=4), {})['estimateUnavailable'], 'Fully charged')
        self.assertEqual(battery.panel_data(dict(percent=80, external=True), dict(State=3), {})['estimateUnavailable'], 'Charging paused')

    async def test_select_checks_support_and_confirms_actual_profile(self):
        with patch.object(battery, 'profile_status', AsyncMock(side_effect=[self.profiles(), self.profiles('power-saver')])):
            with patch.object(battery, 'command', AsyncMock()) as run:
                await battery.select_profile('profile', 'power-saver', Executor())
                self.assertEqual(run.call_args.args[0][-3:], ['ActiveProfile', 's', 'power-saver'])
        with patch.object(battery, 'profile_status', AsyncMock(return_value=self.profiles())):
            with patch.object(battery, 'command', AsyncMock()):
                with self.assertRaisesRegex(Unavailable, 'did not change'):
                    await battery.select_profile('profile', 'performance', Executor())

    async def test_invalid_and_unsupported_profiles_never_write(self):
        with patch.object(battery, 'profile_status', AsyncMock(return_value={})), patch.object(battery, 'command', AsyncMock()) as run:
            for action, selection in [('profile', '--help'), ('invalid', 'balanced'), ('profile', 'performance')]:
                with self.assertRaises(Unavailable):
                    await battery.select_profile(action, selection, Executor())
            run.assert_not_awaited()

    async def test_services_unavailable_do_not_hide_battery(self):
        with patch.object(battery, 'snapshot', return_value=[battery.battery_item(40, False)]), patch.object(battery, 'properties', AsyncMock(side_effect=Unavailable('offline'))):
            row = (await battery.provider({}))[0]
            self.assertEqual(row['battery']['percent'], 40)
            self.assertEqual(row['panel']['profileMessage'], 'Power modes unavailable')
