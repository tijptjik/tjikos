import asyncio
import time
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch
from bartjik_core import history, workspaces
from bartjik_core.io import RateLimit


class HistoryTests(unittest.IsolatedAsyncioTestCase):
    async def test_dedup_cache_failure_and_backoff(self):
        history_data = {"prices": [[100000000, 10], [100000001, 11]]}
        service = history.History()
        with patch.object(history, "http_json", return_value=history_data) as request:
            a, b = await asyncio.gather(
                service.get("bitcoin", "usd"), service.get("bitcoin", "usd")
            )
            self.assertEqual(a, b)
            self.assertEqual(request.call_count, 1)
            await service.get("bitcoin", "usd")
            self.assertEqual(request.call_count, 1)
        service.cache[("bitcoin", "usd")]["updated"] = time.time() - 61
        service.attempts[("bitcoin", "usd")] = time.time() - 61
        with patch.object(history, "http_json", side_effect=RateLimit(90)) as request:
            a = await service.get("bitcoin", "usd")
            b = await service.get("bitcoin", "usd")
            self.assertTrue(a["stale"])
            self.assertEqual(len(a["points"]), 2)
            self.assertIn("429", b["error"])
            self.assertEqual(request.call_count, 1)
        await service.close()

    async def test_minute_throttle_persists_success_and_failure(self):
        data = {"prices": [[100000000, 10], [100000001, 11]]}
        with tempfile.TemporaryDirectory() as td:
            path = Path(td)/"history.json"
            with patch.object(history.time,"time",return_value=1000), patch.object(history,"http_json",return_value=data) as request:
                service = history.History(path)
                await service.get("bitcoin","usd")
                self.assertEqual(request.call_count,1)
                restarted = history.History(path)
                self.assertEqual((await restarted.get("bitcoin","usd"))["points"],data["prices"])
                self.assertEqual(request.call_count,1)
            with patch.object(history.time,"time",return_value=1059), patch.object(history,"http_json",return_value=data) as request:
                await restarted.get("bitcoin","usd")
                request.assert_not_called()
            with patch.object(history.time,"time",return_value=1060), patch.object(history,"http_json",side_effect=RateLimit(1)) as request:
                self.assertTrue((await restarted.get("bitcoin","usd"))["stale"])
                self.assertEqual(request.call_count,1)
            with patch.object(history.time,"time",return_value=1119), patch.object(history,"http_json",return_value=data) as request:
                restarted = history.History(path)
                self.assertTrue((await restarted.get("bitcoin","usd"))["stale"])
                request.assert_not_called()
            with patch.object(history.time,"time",return_value=1120), patch.object(history,"http_json",return_value=data) as request:
                await restarted.get("bitcoin","usd")
                self.assertEqual(request.call_count,1)
            await service.close()
            await restarted.close()

    def test_malformed_and_order(self):
        for data in ({}, {"prices": [[1, 2]]}, {"prices": [[1, float("nan")], [2, 3]]}):
            with self.assertRaises(history.Unavailable):
                history.normalize(data)
        self.assertEqual(
            history.normalize({"prices": [[20, 2], [10, 1], [20, 3]]}),
            [[10, 1], [20, 3]],
        )

    def test_only_occupied_and_current_workspace_circles(self):
        snapshot = {
            "monitors": [
                {"name": "DP-1", "activeWorkspace": {"id": 2}, "focused": True}
            ],
            "workspaces": [
                {"id": 1, "name": "1", "monitor": "DP-1", "windows": 1},
                {"id": 2, "name": "2", "monitor": "DP-1", "windows": 0},
                {"id": 3, "name": "3", "monitor": "DP-1", "windows": 0},
                {"id": -9, "name": "special:codex", "monitor": "DP-1", "windows": 2},
            ],
        }
        options = {
            "occupiedOnly": True,
            "special": False,
            "label": "circles",
            "persistent": {"DP-1": [1, 2, 3, 4]},
        }
        rows = workspaces.view(snapshot, "DP-1", options)
        self.assertEqual([r["id"] for r in rows], ["1", "2"])
        self.assertTrue(rows[0]["icon"]["value"].endswith("circle-open.svg"))
        self.assertTrue(rows[1]["icon"]["value"].endswith("circle-filled.svg"))
        self.assertEqual(
            len(workspaces.view(snapshot, "DP-1", {**options, "special": True})), 3
        )

        numbered = workspaces.view(
            snapshot, "DP-1", {**options, "label": "circles_name"}
        )
        self.assertEqual([r["text"]["value"] for r in numbered], ["1", "2"])
        self.assertTrue(all(r["type"] == "icon_text" for r in numbered))
