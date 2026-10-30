import concurrent.futures
import datetime as dt
import sys
import tempfile
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from todo import Store, occurrence, next_occurrence


class TodoTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(self.temp.name)

    def create(self, **fields):
        return self.store.request({'action': 'create', 'fields': {'title': 'Synthetic task', **fields}})

    def test_validation_and_concurrent_edit(self):
        with self.assertRaises(ValueError):
            self.create(title=' ')
        task = self.create()
        self.store.request({'action': 'edit', 'id': task['id'], 'revision': task['revision'], 'fields': {'title': 'Changed'}})
        with self.assertRaises(ValueError):
            self.store.request({'action': 'edit', 'id': task['id'], 'revision': task['revision'], 'fields': {'title': 'Stale'}})
        self.assertEqual(self.store.request({})[0]['title'], 'Changed')

    def test_recurrence_month_end_and_leap_year(self):
        task = {'due': '2024-01-31', 'repeat': {'unit': 'months', 'value': 1}}
        self.assertEqual(occurrence(task, dt.date(2024, 2, 29)), '2024-02-29')
        self.assertEqual(occurrence(task, dt.date(2024, 3, 30)), '2024-02-29')
        self.assertEqual(occurrence(task, dt.date(2024, 3, 31)), '2024-03-31')
        task = {'due': '2024-02-29', 'repeat': {'unit': 'years', 'value': 1}}
        self.assertEqual(occurrence(task, dt.date(2025, 3, 1)), '2025-02-28')

    def test_completion_history_is_per_period(self):
        task = self.create(due='2026-10-01', repeat={'unit': 'days', 'value': 1})
        for period in ('2026-10-01', '2026-10-02'):
            self.store.request({'action': 'done', 'id': task['id'], 'period': period})
        result = self.store.request({'action': 'toggle', 'id': task['id'], 'period': '2026-10-02'})
        self.assertEqual(list(result['completions']), ['2026-10-01'])

    def test_claim_is_atomic_between_devices(self):
        task = self.create(due=dt.date.today().isoformat())
        def claim(device):
            return Store(self.temp.name).request({'action': 'claim', 'id': task['id'], 'device': str(device)})
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            self.assertEqual(sum(bool(result) for result in executor.map(claim, range(20))), 1)

    def test_snooze_suppresses_claims(self):
        task = self.create(due=dt.date.today().isoformat())
        self.store.request({'action': 'claim', 'id': task['id']})
        self.store.request({'action': 'snooze', 'id': task['id'], 'until': (dt.datetime.now(dt.timezone.utc)+dt.timedelta(days=1)).isoformat()})
        self.assertFalse(self.store.request({'action': 'claim', 'id': task['id']}))

    def test_selected_weekdays(self):
        task = {'due': '2026-10-05', 'repeat': {'unit': 'days-of-the-week', 'value': '1,5'}}
        self.assertEqual(occurrence(task, dt.date(2026, 10, 8)), '2026-10-05')
        self.assertEqual(occurrence(task, dt.date(2026, 10, 9)), '2026-10-09')

    def test_next_period_and_unaligned_weekday_anchor(self):
        task = {'due': '2026-10-05', 'repeat': {'unit': 'days-of-the-week', 'value': '5'}}
        self.assertEqual(occurrence(task, dt.date(2026, 10, 5)), '2026-10-09')
        self.assertEqual(next_occurrence(task, '2026-10-09'), '2026-10-16')
        task = {'due': '2024-01-31', 'repeat': {'unit': 'months', 'value': 1}}
        self.assertEqual(next_occurrence(task, '2024-02-29'), '2024-03-31')
