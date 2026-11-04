"""Synthetic-only coverage of the account maintenance planner and replacement order."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("consolidate", Path(__file__).resolve().parents[1] / "scripts/consolidate-filters.py")
consolidate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(consolidate)


class FakeGmail:
    def __init__(self, rows, fail_create=False, fail_delete=None):
        self.rows = rows.copy()
        self.fail_create = fail_create
        self.fail_delete = fail_delete
        self.calls = []

    def filters(self):
        return self.rows.copy()

    def request(self, route, method="GET", body=None, params=None):
        self.calls.append((method, route))
        if route == "messages":
            return {"messages": [{"id": "synthetic-received"}]} if "active@example.test" in params["q"] else {}
        if method == "POST":
            if self.fail_create:
                raise RuntimeError("Synthetic failure")
            row = dict(body, id=f"new-{len(self.rows)}")
            self.rows.append(row)
            return row
        if method == "DELETE":
            identity = route.rsplit("/", 1)[1]
            if identity == self.fail_delete:
                raise RuntimeError("Synthetic uncertain deletion")
            self.rows = [row for row in self.rows if row["id"] != identity]
            return None
        raise AssertionError((route, method))


class FilterConsolidationTest(unittest.TestCase):
    def test_sender_activity_ignores_obsolete_subject_but_keeps_full_condition(self):
        criteria = {"from": "Active Person <active@example.test>", "subject": "old campaign"}
        self.assertEqual(consolidate.activity_query(criteria), "from:(active@example.test)")
        self.assertIn("subject:(old campaign)", consolidate.criteria_query(criteria))
        self.assertEqual(len(consolidate.sender_branches({"from": "(a@example.test OR b@example.test)"})), 2)
        self.assertEqual(consolidate.sender_branches({"from": "A Person <a@example.test>"}), [{"from": "A Person <a@example.test>"}])

    def test_plan_preserves_delete_conditions_and_prunes_only_inactive_address_conditions(self):
        action = {"removeLabelIds": ["INBOX"]}
        rows = [
            {"id": "active", "criteria": {"from": "active@example.test", "subject": "old"}, "action": action},
            {"id": "stale", "criteria": {"from": "stale@example.test"}, "action": action},
            {"id": "delete", "criteria": {"from": "stale-delete@example.test"}, "action": {"addLabelIds": ["TRASH"]}},
            {"id": "general", "criteria": {"subject": "future report"}, "action": action},
            {"id": "noop", "criteria": {"from": "noop@example.test"}},
        ]
        with tempfile.TemporaryDirectory(dir="/tmp") as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            gmail = FakeGmail(rows)
            consolidate.plan(gmail, root, consolidate.datetime.date(2024, 11, 4))
            plan = json.loads((root / "plan.json").read_text())
            self.assertIn("delete", plan["keep_ids"])
            self.assertEqual(plan["summary"]["inactive_conditions_removed"], 1)
            self.assertEqual(plan["summary"]["no_action_filters_removed"], 1)
            self.assertTrue(all(method == "GET" for method, _ in gmail.calls))
            consolidate.apply(gmail, root)
            self.assertTrue(json.loads((root / "receipt.json").read_text())["verified_at"])
            self.assertEqual(len(gmail.rows), 2)
            self.assertEqual(gmail.calls[2][0], "POST")
            self.assertFalse(any(route.startswith("messages/") for _, route in gmail.calls))

    def test_failed_create_preserves_every_original_and_records_uncertain_outcome(self):
        rows = [{"id": str(n), "criteria": {"from": "active@example.test"}, "action": {"removeLabelIds": ["INBOX"]}} for n in range(2)]
        with tempfile.TemporaryDirectory(dir="/tmp") as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            gmail = FakeGmail(rows)
            consolidate.plan(gmail, root, consolidate.datetime.date(2024, 11, 4))
            gmail.fail_create = True
            with self.assertRaises(RuntimeError):
                consolidate.apply(gmail, root)
            self.assertEqual(gmail.rows, rows)
            self.assertFalse(any(method == "DELETE" for method, _ in gmail.calls))
            self.assertIsNotNone(json.loads((root / "receipt.json").read_text())["pending_create"])

    def test_interrupted_delete_resumes_without_recreating_replacements(self):
        rows = [{"id": str(n), "criteria": {"from": "active@example.test"}, "action": {"removeLabelIds": ["INBOX"]}} for n in range(2)]
        with tempfile.TemporaryDirectory(dir="/tmp") as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            gmail = FakeGmail(rows, fail_delete="1")
            consolidate.plan(gmail, root, consolidate.datetime.date(2024, 11, 4))
            with self.assertRaises(RuntimeError):
                consolidate.apply(gmail, root)
            receipt = json.loads((root / "receipt.json").read_text())
            self.assertEqual(receipt["deleted"], ["0"])
            self.assertEqual(receipt["pending_delete_ids"], ["1"])
            gmail.fail_delete = None
            consolidate.apply(gmail, root)
            self.assertEqual(sum(method == "POST" for method, _ in gmail.calls), 1)
            self.assertEqual(len(gmail.rows), 1)
            self.assertTrue(json.loads((root / "receipt.json").read_text())["verified_at"])

    def test_slow_delete_is_confirmed_by_readback_before_retrying(self):
        class Response:
            status_code = 500
            ok = False
            content = b"{}"
        class Session:
            calls = []
            def request(self, method, route, **kwargs):
                self.calls.append(method)
                return Response()
            def get(self, route, **kwargs):
                self.calls.append("GET")
                response = Response()
                response.status_code = 404
                return response
        gmail = object.__new__(consolidate.Gmail)
        gmail.local = consolidate.threading.local()
        gmail.local.session = Session()
        gmail.token = lambda: "synthetic"
        self.assertIsNone(gmail.request("settings/filters/synthetic", "DELETE"))
        self.assertEqual(gmail.local.session.calls, ["DELETE", "GET"])

    def test_action_identity_and_large_groups(self):
        self.assertEqual(consolidate.action_key({"addLabelIds": ["B", "A"]}), consolidate.action_key({"addLabelIds": ["A", "B"], "removeLabelIds": []}))
        queries = [f"from:sender{n}@example.test" for n in range(250)]
        filters = consolidate.compile_group({"removeLabelIds": ["INBOX"]}, queries)
        self.assertGreater(len(filters), 1)
        self.assertTrue(all(len(row["criteria"]["query"].encode()) <= 1400 for row in filters))
        combined = " ".join(row["criteria"]["query"] for row in filters)
        self.assertTrue(all(query in combined for query in queries))


if __name__ == "__main__":
    unittest.main()
