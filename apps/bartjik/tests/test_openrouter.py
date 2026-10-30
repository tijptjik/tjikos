
# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bartjik_core import config, credits, details, providers


class OpenRouterTests(unittest.IsolatedAsyncioTestCase):
    def fetcher(self, activity):
        def fetch(path, params=None):
            if path == "/credits":
                return {"data": {"total_credits": 50, "total_usage": 12}}
            if isinstance(activity, Exception):
                raise activity
            return activity
        return Mock(side_effect=fetch)

    def test_balance_and_daily_rows_share_the_same_fetch(self):
        fetch = self.fetcher({"data": [
            {"date": "2026-06-14", "prompt_tokens": 10, "completion_tokens": 20},
            {"date": "2026-06-14", "reasoning_tokens": 3},
            {"date": "2026-06-13", "prompt_tokens": 15},
        ]})
        payload = credits.build_payload(fetch)
        panel = details.openrouter_panel(payload)
        self.assertEqual(fetch.call_count, 2)
        self.assertEqual(panel["balance"], "38.00")
        self.assertEqual(panel["rows"][0]["tokens"], 33)
        self.assertEqual([r["id"] for r in panel["rows"]], ["2026-06-14", "2026-06-13"])
        self.assertEqual(panel["activityStatus"], "available")

    def test_restricted_activity_keeps_valid_balance_without_raw_error(self):
        payload = credits.build_payload(self.fetcher(credits.OpenRouterError("HTTP 403")))
        panel = details.openrouter_panel(payload)
        self.assertEqual(panel["balance"], "38.00")
        self.assertIsNone(payload["dailyUsage"])
        self.assertEqual(panel["rows"], [])
        self.assertEqual(panel["activityStatus"], "restricted")
        self.assertIn("this key", panel["activityNote"])
        self.assertNotIn("403", str(panel))

    def test_malformed_activity_is_unavailable_not_zero_usage(self):
        for response in ({}, [], {"data": {}}, {"data": ["oops"]},
                         {"data": [{"date": "2026-06-14", "prompt_tokens": float("nan")}]},
                         {"data": [{"date": "2026-06-14", "prompt_tokens": True}]}):
            with self.subTest(response=response):
                panel = details.openrouter_panel(credits.build_payload(self.fetcher(response)))
                self.assertEqual(panel["balance"], "38.00")
                self.assertEqual(panel["activityStatus"], "unavailable")
                self.assertEqual(panel["rows"], [])

    def test_empty_usage_and_transient_error_are_distinct(self):
        empty = details.openrouter_panel(credits.build_payload(self.fetcher({"data": []})))
        offline = details.openrouter_panel(credits.build_payload(self.fetcher(credits.OpenRouterError("API unavailable"))))
        self.assertEqual(empty["activityStatus"], "available")
        self.assertIn("No token activity", empty["activityNote"])
        self.assertEqual(offline["activityStatus"], "unavailable")
        self.assertEqual(offline["balance"], "38.00")

    def test_key_limit_is_labelled_and_invalid_credit_rejected(self):
        def fetch(path, params):
            if path == "/credits":
                raise credits.OpenRouterError("HTTP 403")
            return {"data": {"limit_remaining": 7}} if path == "/key" else {"data": []}
        panel = details.openrouter_panel(credits.build_payload(fetch))
        self.assertEqual(panel["balance"], "7.00")
        self.assertEqual(panel["balanceLabel"], "Key spending limit remaining")
        with self.assertRaises(credits.OpenRouterError):
            credits.build_payload(lambda path, params: {"data": {"total_credits": float("nan"), "total_usage": 0}})

    async def test_provider_attaches_panel_and_retains_bar_format(self):
        fetch = self.fetcher(credits.OpenRouterError("HTTP 403"))
        def http(url, headers):
            try:
                return fetch(url.rsplit("/v1", 1)[1])
            except credits.OpenRouterError as error:
                raise providers.Unavailable(str(error)) from None
        with patch.dict(os.environ, {"BARTJIK_TEST_KEY": "synthetic"}), patch.object(providers, "http_json", side_effect=http) as mocked:
            row = (await providers.openrouter({"keyEnv": "BARTJIK_TEST_KEY"}))[0]
            self.assertEqual(row["text"]["value"], "38.00")
            self.assertEqual(row["panel"]["title"], "OpenRouter")
            self.assertEqual(row["panel"]["activityStatus"], "restricted")
            self.assertEqual(mocked.call_count, 2)

    async def test_no_credentials_and_rate_limit_remain_provider_failures(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(providers.Unavailable):
                await providers.openrouter({})
        with patch.dict(os.environ, {"BARTJIK_TEST_KEY": "synthetic"}), patch.object(providers, "http_json", side_effect=providers.RateLimit(90)):
            with self.assertRaises(providers.RateLimit) as error:
                await providers.openrouter({"keyEnv": "BARTJIK_TEST_KEY"})
            self.assertEqual(error.exception.delay, 90)

    def test_panel_validation(self):
        for panel in ({"hover": "true"}, {"delay": -1}, {"closeDelay": 10001}):
            with self.assertRaises(ValueError):
                config.module({"provider": "openrouter", "panel": panel})
        config.module({"provider": "openrouter", "panel": {"hover": True, "delay": 300}})
