import asyncio
from pathlib import Path
import tempfile
import unittest
from unittest.mock import AsyncMock, patch

from bartjik_core.battery import battery_item
from bartjik_core.battery_alert import CriticalBatteryAlert
from bartjik_core.engine import Engine, Worker
from bartjik_core.io import Unavailable


class BatteryAlertTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "state/battery-alert.json"
        self.alert = CriticalBatteryAlert(self.path)
        self.sender = AsyncMock(return_value="notification-id")
        patcher = patch("bartjik_core.battery_alert.command", self.sender)
        patcher.start()
        self.addCleanup(patcher.stop)

    async def update(self, percent, external=False):
        await self.alert.update([battery_item(percent, external)])

    async def test_crossing_five_sends_one_persistent_critical_warning(self):
        await self.update(6)
        await self.update(5.01)
        self.sender.assert_not_awaited()
        await self.update(5)
        command = self.sender.call_args.args[0]
        self.assertIn('5% remaining. Connect to power now.', command)
        self.assertEqual(command[command.index('--urgency') + 1], 'critical')
        self.assertEqual(command[command.index('--timeout') + 1], 'never')
        self.assertEqual(command[command.index('--id') + 1], 'battery-critical')
        for percent in (5, 4, 5.2, 5, 6, 4, 0):
            await self.update(percent)
        self.sender.assert_awaited_once()

    async def test_restart_and_multiple_workers_do_not_duplicate(self):
        await asyncio.gather(*(self.update(4) for _ in range(3)))
        self.alert = CriticalBatteryAlert(self.path)
        await self.update(3)
        self.sender.assert_awaited_once()

    async def test_ac_and_recovery_rearm_next_low_episode(self):
        await self.update(4, True)
        self.sender.assert_not_awaited()
        await self.update(4)
        await self.update(4, True)
        await self.update(3)
        self.assertEqual(self.sender.await_count, 2)
        await self.update(7)
        await self.update(5)
        self.assertEqual(self.sender.await_count, 3)

    async def test_unavailable_notjik_retries_without_marking_delivered(self):
        self.sender.side_effect = [Unavailable('not running'), 'notification-id']
        with patch('bartjik_core.battery_alert.time.monotonic', return_value=100):
            await self.update(5)
            await self.update(4)
        self.assertFalse(self.alert.alerted)
        self.assertFalse(self.path.exists())
        self.sender.assert_awaited_once()
        with patch('bartjik_core.battery_alert.time.monotonic', return_value=130):
            await self.update(4)
        self.assertTrue(self.alert.alerted)
        self.assertEqual(self.sender.await_count, 2)

    async def test_missing_or_invalid_readings_do_not_rearm(self):
        await self.update(4)
        await self.alert.update([])
        for percent in (float('nan'), -1, 101):
            await self.update(percent)
        await self.update(3)
        self.sender.assert_awaited_once()

    async def test_worker_uses_fresh_samples_and_keeps_provider_healthy(self):
        observer = AsyncMock()
        worker = Worker({'provider': 'battery'}, lambda: None, observer)
        rows = [battery_item(5, False)]
        with patch('bartjik_core.providers.REGISTRY', {'battery': AsyncMock(return_value=rows)}):
            worker.start()
            try:
                for _ in range(50):
                    if observer.update.await_count:
                        break
                    await asyncio.sleep(.01)
                observer.update.assert_awaited_once_with(rows)
                self.assertEqual(worker.status()['state'], 'ready')
            finally:
                await worker.close()

    async def test_demo_never_creates_notification_observer(self):
        with patch('bartjik_core.engine.CriticalBatteryAlert') as observer:
            engine = Engine(lambda value: None, demo_mode=True)
            try:
                self.assertIsNone(engine.battery_alert)
                observer.assert_not_called()
            finally:
                await engine.close()
