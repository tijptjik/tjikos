import datetime as dt
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch, AsyncMock
from bartjik_core import details, providers, config


class DetailsTests(unittest.IsolatedAsyncioTestCase):
    def test_codex_reset_timestamp_is_separate_from_capacity(self):
        rows = [
            dict(label="A", plan_type="pro", remaining_percent=97, reset_at=600000),
            dict(label="B", plan_type=None, remaining_percent=None, reset_at=None),
            dict(label="C", plan_type="plus", remaining_percent=0, reset_at=0),
        ]
        with patch.object(details.time, "time", return_value=100000):
            panel = details.codex_panel(rows, [], "85%")
        accounts = panel["rows"][1:]
        self.assertEqual([a["resetAt"] for a in accounts], [600000, None, 0])
        self.assertEqual([a["progress"] for a in accounts], [.97, -1, 0])
        self.assertEqual([a["plan"] for a in accounts], ["pro", "", "plus"])
        self.assertEqual(panel["rows"][0]["value"], "85%")

    def test_world_clock_dst_and_config(self):
        winter = details.world_clock(
            now=dt.datetime(2026, 1, 15, 12, tzinfo=dt.timezone.utc)
        )["rows"]
        summer = details.world_clock(
            now=dt.datetime(2026, 7, 15, 12, tzinfo=dt.timezone.utc)
        )["rows"]
        self.assertEqual(
            [r["value"] for r in winter], ["07:00", "12:00", "13:00", "21:00"]
        )
        self.assertEqual(
            [r["value"] for r in summer], ["08:00", "13:00", "14:00", "21:00"]
        )
        self.assertEqual(details.world_clock([])["rows"], [])
        with self.assertRaises(ValueError):
            config.module({"provider": "datetime", "worldClocks": "London"})
        with self.assertRaises(Exception):
            config.module(
                {
                    "provider": "datetime",
                    "worldClocks": [{"label": "Oops", "timezone": "Nowhere/None"}],
                }
            )

    def test_active_group_methods(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "profile"
            p.write_text(
                "[Groups/0]\nName=Main\n[Groups/0/Items/2]\nName=pinyin\n[Groups/0/Items/0]\nName=keyboard-us\n[Groups/1]\nName=Other\n[Groups/1/Items/0]\nName=quick-classic\n"
            )
            self.assertEqual(
                details.input_methods("Main", p), ["keyboard-us", "pinyin"]
            )
            self.assertEqual(details.input_methods("Missing", p), [])

    def test_read_only_usage_summary_and_missing_schema(self):
        con = sqlite3.connect(":memory:")
        con.execute(
            "CREATE TABLE request_logs(input_tokens,output_tokens,cached_input_tokens,cost_usd,latency_ms,status,requested_at,deleted_at)"
        )
        con.execute(
            "INSERT INTO request_logs VALUES(100,20,50,0.5,2000,'success',datetime('now'),NULL)"
        )
        con.execute(
            "INSERT INTO request_logs VALUES(100,20,50,0.5,2000,'error',datetime('now','-2 days'),NULL)"
        )
        rows = details.request_summary(con)
        self.assertEqual(rows[0]["value"], "1")
        self.assertEqual(rows[1]["value"], "100 / 20")
        self.assertEqual(rows[1]["id"], "tokens")
        self.assertEqual((rows[1]["input"], rows[1]["output"]), (100, 20))
        self.assertEqual(rows[0]["amount"], 1)
        self.assertEqual(rows[0]["failures"], 0)
        self.assertEqual(rows[2]["amount"], 50)
        self.assertEqual(rows[3]["value"], "$0.50")
        self.assertEqual(
            con.execute("SELECT COUNT(*) FROM request_logs").fetchone()[0], 2
        )
        con.execute("DROP TABLE request_logs")
        self.assertEqual(details.request_summary(con)[0]["value"], "Unavailable")
        con.close()

    async def test_vpn_and_tailnet_panels(self):
        with patch.object(
            providers,
            "json_command",
            AsyncMock(return_value={"state":"connected","details":{"location":{"country":"Japan","city":"Tokyo","ipv4":"192.0.2.1"}}}),
        ):
            rows = await providers.mullvad({})
            self.assertEqual(rows[0]["panel"]["title"], "Mullvad")
        with patch.object(
            providers,
            "json_command",
            AsyncMock(
                return_value={
                    "BackendState": "NeedsLogin",
                    "Peer": {"a": {"HostName": "demo", "Online": True}},
                    "Self": {},
                }
            ),
        ):
            panel = (await providers.tailscale({}))[0]["panel"]
            self.assertEqual(panel["rows"][0]["value"], "NeedsLogin")
            self.assertEqual(panel["rows"][3]["value"], "1 / 1")
        with patch.object(providers, "json_command", AsyncMock(return_value={})):
            with self.assertRaises(providers.Unavailable):
                await providers.tailscale({})

    async def test_input_selection_checks_current_group_and_monitor(self):
        import asyncio
        from types import SimpleNamespace
        from bartjik_core.engine import Engine

        e = Engine(lambda _: None)
        e.changed = lambda: None
        e.bars = [
            {
                "key": "DP-1/bar",
                "screen": "DP-1",
                "moduleConfigs": {"input": {"provider": "locale"}},
                "sources": {"input": "worker"},
            }
        ]
        e.workers = {"worker": SimpleNamespace(wake=asyncio.Event())}
        context = {
            "barKey": "DP-1/bar",
            "monitor": "DP-1",
            "module": "input",
            "selection": "pinyin",
        }
        with (
            patch("bartjik_core.io.command", AsyncMock(return_value="Main")) as command,
            patch(
                "bartjik_core.details.input_methods",
                return_value=["keyboard-us", "pinyin"],
            ),
        ):
            await e.panel_action({"context": context})
            self.assertEqual(
                command.call_args.args[0], ["fcitx5-remote", "--check", "-s", "pinyin"]
            )
            self.assertTrue(e.workers["worker"].wake.is_set())
            command.reset_mock()
            await e.panel_action({"context": {**context, "selection": "not-in-group"}})
            self.assertEqual(command.call_count, 1)
            self.assertTrue(e.action_error)
            command.reset_mock()
            await e.panel_action({"context": {**context, "monitor": "DP-2"}})
            command.assert_not_called()
