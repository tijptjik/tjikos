import asyncio
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import AsyncMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bartjik_core import config, workspaces, providers, quota
from bartjik_core.model import item, untrusted_items
from bartjik_core.engine import Worker, Engine
from bartjik_core.actions import Executor, choose, workspace_commands, lua_string
from bartjik_core.io import command, Unavailable, RateLimit

ROOT = Path(__file__).resolve().parents[1]
SCREEN = dict(name="DP-1", width=1920, height=1080, scale=1)


class ConfigTests(unittest.TestCase):
    def load(self, value):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "config.json"
            p.write_text(json.dumps(value))
            return config.load(p)

    def test_workspace_preview_refresh_limits(self):
        for interval in (33, 50, 250, 5000):
            config.module({"provider": "workspaces", "preview": {"refreshInterval": interval}})
        for interval in (0, 32, 5001):
            with self.assertRaises(ValueError):
                config.module({"provider": "workspaces", "preview": {"refreshInterval": interval}})

    def test_tray_artwork_validation(self):
        config.module({"provider":"icontray", "iconOverrides":{"App*":"builtin:telegram"}})
        for overrides in ({"App":"builtin:unknown"},{"App":"https://example.com/icon.svg"},{"App":3}):
            with self.assertRaises(ValueError):
                config.module({"provider":"icontray","iconOverrides":overrides})

    def test_primary_only_music_and_markets(self):
        c = config.load(ROOT / "examples/comparison.json")
        primary = config.resolve(c, SCREEN, 0)
        secondary = config.resolve(c, {**SCREEN, "name":"DP-2"}, 1)
        self.assertEqual([i["id"] for b in secondary for i in b["islands"]], ["workspaces"])
        self.assertTrue({"markets", "media"} <= {i["id"] for b in primary for i in b["islands"]})
        self.assertEqual(c["modules"]["spaces"]["appearance"], "ring")
        self.assertEqual(c["modules"]["spaces"]["label"], "name")
        self.assertTrue(c["modules"]["music"]["track"]["style"]["marquee"])

    def test_presets_all_modules(self):
        for name in ("desktop", "laptop"):
            c = config.load(ROOT / "examples" / f"{name}.json")
            self.assertEqual(
                set(m["provider"] for m in c["modules"].values()),
                set(config.PROVIDERS) - ({"process", "monitor-layout"} if name == "laptop" else {"process"}),
            )

    def test_exact_default_and_arrays_replace(self):
        c = self.load({})
        self.assertNotIn("centre_width", c["layout"])
        self.assertEqual(config.merge({"a": [1, 2]}, {"a": [3]}), {"a": [3]})

    def test_override_precedence_and_stack(self):
        c = self.load(
            {
                "perMonitor": {
                    "1": {"layout": {"centre_width": "40%"}},
                    "DP-1": {
                        "layout": {"centre_width": 960},
                        "modules": {"clock": {"format": "%S"}},
                    },
                },
                "bars": [
                    {
                        "id": "a",
                        "position": "l",
                        "alignment": "r",
                        "islands": [{"id": "i", "modules": ["clock"]}],
                    },
                    {
                        "id": "b",
                        "position": "l",
                        "islands": [{"id": "j", "modules": ["clock"]}],
                        "perMonitor": {"DP-1": {"offsetY": -40}},
                    },
                ],
            }
        )
        a, b = config.resolve(c, SCREEN, 0)
        self.assertEqual(a["layout"]["centre_width"], 960)
        self.assertEqual(a["moduleConfigs"]["clock"]["format"], "%S")
        self.assertGreater(b["stackOffset"], 0)
        self.assertEqual(b["offsetY"], -40)

    def test_widths_scale_and_vertical(self):
        c = self.load({"layout": {"centre_width": "2000px"}})
        with self.assertRaises(ValueError):
            config.resolve(c, SCREEN, 0)
        self.assertTrue(config.resolve(c, {**SCREEN, "scale": 2}, 0))
        c["bars"][0]["side"] = "l"
        with self.assertRaises(ValueError):
            config.resolve(c, SCREEN, 0)

    def test_conflicting_bar_geometry_rejected(self):
        with self.assertRaises(ValueError):
            self.load({"bars": [{"id": "x", "centre_width": "30%"}]})

    def test_reject_invalid_nested_action_and_output(self):
        for v in [
            {
                "modules": {
                    "x": {"provider": "audio", "icon": {"actions": {"click": "rm *"}}}
                }
            },
            {"layout": {"centre_width": "-10%"}},
            {"bars": [{"id": "x", "monitors": [0]}]},
            {"modules": {"x": {"provider": "process", "command": "echo hello"}}},
        ]:
            with self.assertRaises(ValueError):
                self.load(v)
        with self.assertRaises(ValueError):
            untrusted_items([item("x", "1"), item("x", "2")])

    def test_style_does_not_duplicate_provider(self):
        self.assertEqual(
            config.provider_key({"provider": "audio", "style": {"color": "red"}}),
            config.provider_key(
                {"provider": "audio", "icon": {"actions": {"click": "audio.route"}}}
            ),
        )

    def test_untrusted_process_output_cannot_install_actions(self):
        rows = untrusted_items(
            [item("x", "<b>literal</b>", actions={"leftClick": ["sh", "-c", "bad"]})]
        )
        self.assertNotIn("actions", rows[0])
        self.assertEqual(rows[0]["text"]["value"], "<b>literal</b>")
        self.assertEqual(
            untrusted_items({"text": "<span>A &amp; B</span>", "class": "active"})[0][
                "text"
            ]["value"],
            "A & B",
        )


class WorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.snap = dict(
            monitors=[
                dict(
                    name="DP-1",
                    activeWorkspace={"id": 2},
                    specialWorkspace={"id": -99},
                    focused=False,
                ),
                dict(name="DP-2", activeWorkspace={"id": 12}, focused=True),
            ],
            workspaces=[
                dict(id=n, name=str(n), monitor="DP-1" if n < 10 else "DP-2")
                for n in (1, 2, 10, 11, 12)
            ]
            + [
                dict(id=-99, name="special:codex", monitor="DP-1"),
                dict(id=-3, name="named", monitor="DP-1"),
            ],
            urgent=[1],
        )

    def test_own_active_not_focused(self):
        one = workspaces.view(self.snap, "DP-1", {})
        two = workspaces.view(self.snap, "DP-2", {})
        self.assertEqual([i["id"] for i in one if i["state"]["active"]], ["2", "-99"])
        self.assertFalse(any(i["state"]["focused"] for i in one))
        self.assertEqual([i["id"] for i in two if i["state"]["active"]], ["12"])
        self.assertTrue(next(i for i in one if i["id"] == "1")["state"]["urgent"])
        self.assertEqual(
            [i["text"]["value"] for i in one], ["1", "2", "named", "codex"]
        )

    def test_reassignment_placeholder_never_duplicates_runtime_owner(self):
        opts = {"persistent": {"DP-1": [1, 2, 3]}}
        before = workspaces.view(self.snap, "DP-1", opts)
        self.snap["workspaces"][0]["monitor"] = "DP-2"
        self.assertNotIn(
            "1", [i["id"] for i in workspaces.view(self.snap, "DP-1", opts)]
        )
        self.assertIn("1", [i["id"] for i in workspaces.view(self.snap, "DP-2", opts)])
        self.snap["workspaces"].append(dict(id=3, name="3", monitor="DP-1"))
        after = workspaces.view(self.snap, "DP-1", opts)
        self.assertEqual(
            next(i["id"] for i in before if i["text"]["value"] == "3"),
            next(i["id"] for i in after if i["text"]["value"] == "3"),
        )

    def test_hotplug_reconnect_and_special(self):
        self.snap["monitors"].pop()
        self.assertEqual(workspaces.view(self.snap, "DP-2", {}), [])
        self.snap["monitors"].append(
            dict(name="DP-2", activeWorkspace={"id": 11}, focused=False)
        )
        self.assertEqual(
            [
                i["id"]
                for i in workspaces.view(self.snap, "DP-2", {})
                if i["state"]["active"]
            ],
            ["11"],
        )
        self.assertNotIn(
            "-99",
            [i["id"] for i in workspaces.view(self.snap, "DP-1", {"special": False})],
        )

    def test_numeric_sort_and_explicit_dispatch(self):
        rows = workspaces.view(self.snap, "DP-2", {})
        self.assertEqual([i["id"] for i in rows], ["10", "11", "12"])
        args = workspace_commands(rows[0], "DP-2")
        self.assertIn('monitor = "DP-2"', args[0][-1])
        self.assertIn("workspace = 10", args[1][-1])
        self.assertEqual(lua_string('x"\\\n'), '"x\\"\\\\\\010"')


class ProviderTests(unittest.IsolatedAsyncioTestCase):
    async def test_region_flag_and_optional_credit_symbol(self):
        with patch.object(
            providers,
            "json_command",
            AsyncMock(return_value={"state":"connected","details":{"location":{"country":"Japan","city":"Tokyo","ipv4":"192.0.2.1"}}}),
        ):
            rows = await providers.mullvad({})
            self.assertTrue(rows[0]["icon"]["value"].endswith("/flags/jp.svg"))
            self.assertTrue(Path(rows[0]["icon"]["value"]).is_file())
            self.assertEqual(rows[0]["icon"]["kind"], "image")
            self.assertFalse(rows[0]["icon"]["style"]["greyscaleIcons"])
            self.assertEqual(
                (await providers.mullvad({"regionFlag": False}))[0]["icon"]["value"],
                "shield_lock",
            )
        with (
            patch.dict(os.environ, {"OPENROUTER_API_KEY": "synthetic-test-only"}),
            patch.object(
                providers.credits,
                "build_payload",
                return_value={"text": "$38.00", "tooltip": "Synthetic"},
            ),
        ):
            self.assertEqual(
                (await providers.openrouter({}))[0]["text"]["value"], "38.00"
            )
            self.assertEqual(
                (await providers.openrouter({"currencySymbol": "$"}))[0]["text"][
                    "value"
                ],
                "$38.00",
            )

    async def test_absent_timeout_bounded_and_cancellation(self):
        with self.assertRaises(Unavailable):
            await command(["/no/such/bartjik-command"])
        with self.assertRaisesRegex(Unavailable, "timed out"):
            await command(
                [sys.executable, "-c", "import time;time.sleep(5)"], timeout=0.03
            )
        with self.assertRaisesRegex(Unavailable, "limit"):
            await command([sys.executable, "-c", 'print("x"*5000)'], limit=1024)

    async def test_transient_error_retains_last_data(self):
        first = asyncio.Event()
        changed = []

        async def poll(m):
            if not first.is_set():
                first.set()
                return [item("stable", "ok")]
            raise Unavailable("offline")

        with patch.dict(providers.REGISTRY, {"fixture": poll}):
            w = Worker(
                {"provider": "fixture", "interval": 0.01}, lambda: changed.append(1)
            )
            w.start()
            await first.wait()
            await asyncio.sleep(0.04)
            self.assertEqual(w.status()["state"], "stale")
            self.assertEqual(w.status()["items"][0]["id"], "stable")
            await w.close()
            self.assertTrue(w.task.done())

    async def test_rate_limit_backoff(self):
        fn = AsyncMock(side_effect=RateLimit(20))
        with patch.dict(providers.REGISTRY, {"fixture": fn}):
            w = Worker({"provider": "fixture", "interval": 0.001}, lambda: None)
            w.start()
            await asyncio.sleep(0.08)
            self.assertEqual(fn.await_count, 1)
            self.assertIn("429", w.error)
            await w.close()

    async def test_stream_restart_malformed_and_cleanup(self):
        w = Worker(
            {
                "provider": "process",
                "mode": "stream",
                "command": [sys.executable, "-u", "-c", 'print("not-json")'],
                "timeout": 1,
            },
            lambda: None,
        )
        w.start()
        await asyncio.sleep(0.1)
        self.assertGreaterEqual(w.failures, 1)
        self.assertIn("invalid", w.error)
        w.wake.set()
        await asyncio.sleep(0.1)
        self.assertGreaterEqual(w.polls, 2)
        await w.close()
        w = Worker(
            {
                "provider": "process",
                "mode": "stream",
                "command": [
                    sys.executable,
                    "-u",
                    "-c",
                    'import time;print(\'[ {"id":"one","type":"text","text":{"value":"ready"}} ]\');time.sleep(60)',
                ],
                "timeout": 60,
            },
            lambda: None,
        )
        w.start()
        await asyncio.sleep(0.1)
        self.assertEqual(w.items[0]["id"], "one")
        await w.close()
        self.assertTrue(w.task.done())

    async def test_engine_deduplicates_and_reload_cleans(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "config.json"
            cfg = {
                "modules": {
                    "a": {"provider": "datetime"},
                    "b": {"provider": "datetime", "style": {"color": "pink"}},
                },
                "bars": [
                    {"id": "one", "islands": [{"id": "i", "modules": ["a", "b"]}]},
                    {"id": "two", "islands": [{"id": "i", "modules": ["a"]}]},
                ],
            }
            p.write_text(json.dumps(cfg))
            with patch.dict(os.environ, {"BARTJIK_CONFIG": str(p)}):
                e = Engine(lambda x: None)
                e.screens = [SCREEN, {**SCREEN, "name": "DP-2"}]
                await e.reload()
                self.assertEqual(len(e.workers), 1)
                w = next(iter(e.workers.values()))
                p.write_text("{bad")
                await e.reload()
                self.assertEqual(len(e.workers), 1)
                self.assertTrue(e.error)
                cfg["bars"] = []
                p.write_text(json.dumps(cfg))
                await e.reload()
                self.assertEqual(len(e.workers), 0)
                self.assertTrue(w.task.done())
                await e.close()

    async def test_network_telemetry_failure_not_offline(self):
        with patch(
            "bartjik_core.providers.command",
            AsyncMock(
                side_effect=[
                    "wlan0:wifi:connected:Test\\:WiFi",
                    Unavailable("missed telemetry"),
                ]
            ),
        ):
            row = (await providers.network({}))[0]
            self.assertFalse(row["state"]["disconnected"])
            self.assertEqual(row["text"]["value"], "Test:WiFi")

    async def test_dnd_unavailable_distinct(self):
        with patch(
            "bartjik_core.providers.json_command",
            AsyncMock(return_value={"ok": True, "paused": True}),
        ):
            self.assertTrue((await providers.dnd({}))[0]["state"]["active"])
        with patch(
            "bartjik_core.providers.json_command", AsyncMock(return_value={"ok": False})
        ):
            with self.assertRaises(Unavailable):
                await providers.dnd({})

    async def test_audio_muted_exact_and_parts(self):
        sink = {
            "name": "sink",
            "mute": True,
            "volume": {"left": {"value": 32768}},
            "description": "Demo",
        }
        with patch(
            "bartjik_core.providers.audio_state", AsyncMock(return_value=([sink], sink))
        ):
            row = (await providers.audio({}))[0]
            self.assertEqual(row["text"]["value"], "-%")
            self.assertEqual(choose({}, row, "icon", "leftClick"), "audio.route")
            self.assertEqual(choose({}, row, "text", "leftClick"), "audio.mute")
            for part in ("icon", "text"):
                self.assertEqual(choose({}, row, part, "hoverEnter"), "audio.panel")

    async def test_audio_route_icon_is_independent_of_mute(self):
        for description, symbol in [("USB Audio Speakers", "headphones"), ("USB Audio Front Headphones", "headphones"), ("Navi 21/23 HDMI/DP Screen", "volume_up")]:
            for muted in (False, True):
                sink = dict(name="sink", description=description, mute=muted, volume={"left": {"value": 65536}})
                with patch("bartjik_core.providers.audio_state", AsyncMock(return_value=([sink], sink))):
                    row = (await providers.audio({}))[0]
                self.assertEqual(row["icon"]["value"], symbol)
                self.assertEqual(row["text"]["value"], "-%" if muted else "100%")

    async def test_audio_route_uses_panel_pair_and_respects_explicit_routes(self):
        usb = dict(name="usb", description="USB Audio Speakers")
        dp = dict(name="dp", description="Navi 21/23 HDMI/DP Screen")
        other = dict(name="spdif", description="USB Audio S/PDIF Output")
        for active, module, target in [(usb, {}, "dp"), (dp, {}, "usb"), (usb, {"routes": ["usb", "spdif"]}, "spdif")]:
            with (
                patch("bartjik_core.actions.audio_state", AsyncMock(return_value=([usb, other, dp], active))),
                patch("bartjik_core.actions.json_command", AsyncMock(return_value=[])),
                patch("bartjik_core.actions.command", AsyncMock()) as run,
            ):
                await Executor().execute("audio.route", {}, module, {}, [])
                run.assert_awaited_once_with(["pactl", "set-default-sink", target])

    def test_crypto_ids_format_and_order(self):
        c = {
            "assets": providers.ASSETS,
            "currency": "USD",
            "currencySymbol": "",
            "display": ["price"],
        }
        data = {
            "data": {
                s: {"quote": {"USD": {"price": 123.45}}} for s in ("BTC", "SOL", "AR")
            }
        }
        rows = providers.crypto_items(data, c)
        self.assertEqual([i["id"] for i in rows], ["BTC/USD", "SOL/USD", "AR/USD"])
        self.assertEqual([i["text"]["value"] for i in rows], ["123", "123", "123.5"])
        c["assets"] = list(reversed(c["assets"]))
        self.assertEqual(providers.crypto_items(data, c)[-1]["id"], "BTC/USD")

    def test_quota_weekly_duration_and_weighting(self):
        payload = {
            "accounts": [
                {
                    "status": "active",
                    "alias": "Synthetic A",
                    "planType": "plus",
                    "windowMinutesPrimary": 10080,
                    "windowMinutesSecondary": 300,
                    "usage": {
                        "primaryRemainingPercent": 100,
                        "secondaryRemainingPercent": 0,
                    },
                },
                {
                    "status": "active",
                    "alias": "Synthetic B",
                    "planType": "pro",
                    "windowMinutesSecondary": 10080,
                    "usage": {"secondaryRemainingPercent": 0},
                },
            ]
        }
        rows = providers.quota_api_rows(payload)
        out = quota.build_payload(rows)
        self.assertIn("13%", out["text"])
        self.assertNotIn("5h", out["tooltip"])

    def test_quota_sql_duration_over_field_name(self):
        con = sqlite3.connect(":memory:")
        con.row_factory = sqlite3.Row
        con.executescript(
            'CREATE TABLE accounts(id, alias, email, plan_type,status);CREATE TABLE usage_history(id,account_id,window,window_minutes,used_percent,reset_at,recorded_at);INSERT INTO accounts VALUES(1,"Synthetic","synthetic@example.invalid","plus","active");INSERT INTO usage_history VALUES(1,1,"primary",10080,25,0,1),(2,1,"secondary",300,99,0,2);'
        )
        rows = con.execute(quota.QUERY).fetchall()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["remaining_percent"], 75)
        con.close()

    def test_openrouter_credit_and_usage_fixture(self):
        from bartjik_core import credits

        responses = {
            "/credits": {"data": {"total_credits": 50, "total_usage": 12}},
            "/activity": {
                "data": [
                    {
                        "date": "2026-01-01",
                        "prompt_tokens": 10,
                        "completion_tokens": 20,
                        "reasoning_tokens": 3,
                    }
                ]
            },
        }
        out = credits.build_payload(lambda path, params: responses[path])
        self.assertEqual(out["text"], "$38.00")
        self.assertIn("33 tokens", out["tooltip"])


class ActionTests(unittest.IsolatedAsyncioTestCase):
    async def test_monitor_scroll_targets_own_active_and_context(self):
        items = [
            dict(id=str(n), workspace=n, state={"active": n == 12})
            for n in (11, 12, 13)
        ]
        with patch("bartjik_core.actions.command", AsyncMock()) as run:
            await Executor().execute(
                "workspace.next", {"monitor": "DP-2"}, {}, items[0], items
            )
            self.assertIn('monitor = "DP-2"', run.call_args_list[0].args[0][-1])
            self.assertIn("workspace = 13", run.call_args_list[1].args[0][-1])

    async def test_commands_no_interpolation_context(self):
        context = {
            "bar": "test",
            "monitor": "DP-2",
            "module": "audio",
            "item": "master",
            "part": "text",
            "anchor": {"x": 4, "y": 8, "width": 30, "height": 20},
        }
        with patch("bartjik_core.actions.command", AsyncMock()) as run:
            await Executor().execute(
                ["echo", "$(touch /tmp/never)"], context, {}, {}, []
            )
            self.assertEqual(run.call_args.args[0], ["echo", "$(touch /tmp/never)"])
            self.assertEqual(
                json.loads(run.call_args.kwargs["env"]["BARTJIK_CONTEXT"]), context
            )

    async def test_audio_routes_move_streams_and_report_failure(self):
        a = dict(name="a")
        b = dict(name="b")
        with (
            patch(
                "bartjik_core.actions.audio_state", AsyncMock(return_value=([a, b], a))
            ),
            patch(
                "bartjik_core.actions.json_command",
                AsyncMock(return_value=[{"index": 7}]),
            ),
            patch(
                "bartjik_core.actions.command",
                AsyncMock(side_effect=[None, Unavailable("failed")]),
            ) as run,
        ):
            with self.assertRaisesRegex(Unavailable, "streams"):
                await Executor().execute("audio.route", {}, {}, {}, [])
            self.assertEqual(
                run.call_args_list[0].args[0], ["pactl", "set-default-sink", "b"]
            )
            self.assertEqual(
                run.call_args_list[1].args[0], ["pactl", "move-sink-input", "7", "b"]
            )

    async def test_tailscale_failure_never_reports_success(self):
        with (
            patch(
                "bartjik_core.actions.json_command",
                AsyncMock(return_value={"BackendState": "Stopped"}),
            ),
            patch(
                "bartjik_core.actions.command",
                AsyncMock(side_effect=Unavailable("permission denied")),
            ),
        ):
            with self.assertRaises(Unavailable):
                await Executor().execute("tailscale.toggle", {}, {}, {}, [])

    async def test_destructive_actions_only_mocked(self):
        with patch("bartjik_core.actions.command", AsyncMock()) as run:
            for args in [["systemctl", "suspend"], ["systemctl", "poweroff"]]:
                await Executor().execute(args, {}, {}, {}, [])
            self.assertEqual(run.await_count, 2)


if __name__ == "__main__":
    unittest.main()


class LifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def test_compositor_socket_disconnect_and_reconcile(self):
        connections = []
        with tempfile.TemporaryDirectory() as td:
            socket = Path(td) / "hypr/test/.socket2.sock"
            socket.parent.mkdir(parents=True)

            async def accept(reader, writer):
                connections.append(writer)

            server = await asyncio.start_unix_server(accept, str(socket))
            samples = [
                {
                    "monitors": [{"name": "DP-1", "activeWorkspace": {"id": 1}}],
                    "workspaces": [{"id": 1, "name": "1", "monitor": "DP-1"}],
                },
                {
                    "monitors": [{"name": "DP-2", "activeWorkspace": {"id": 12}}],
                    "workspaces": [{"id": 12, "name": "12", "monitor": "DP-2"}],
                },
            ]

            async def until(fn):
                for _ in range(150):
                    if fn():
                        return
                    await asyncio.sleep(0.02)
                self.fail("workspace subscription did not reconcile")

            with (
                patch.dict(
                    os.environ,
                    {"XDG_RUNTIME_DIR": td, "HYPRLAND_INSTANCE_SIGNATURE": "test"},
                ),
                patch(
                    "bartjik_core.workspaces.snapshot", AsyncMock(side_effect=samples)
                ),
            ):
                watcher = workspaces.Watcher(lambda: None)
                watcher.start()
                try:
                    await until(lambda: watcher.data == samples[0])
                    connections[0].close()
                    await until(lambda: bool(watcher.error))
                    await until(lambda: watcher.data == samples[1])
                    self.assertFalse(watcher.error)
                    self.assertEqual(workspaces.view(watcher.data, "DP-1", {}), [])
                    self.assertEqual(
                        workspaces.view(watcher.data, "DP-2", {})[0]["id"], "12"
                    )
                finally:
                    await watcher.close()
                    for writer in connections:
                        writer.close()
                    server.close()
                    await server.wait_closed()

    async def test_panel_receives_actual_anchor_without_waybar(self):
        with tempfile.TemporaryDirectory() as td:
            (Path(td) / "shell.qml").touch()
            context = {
                "monitor": "DP-2",
                "anchor": {"x": 40, "y": 900, "width": 20, "height": 24},
                "event": "leftClick",
            }
            with patch(
                "bartjik_core.actions.command", AsyncMock(side_effect=["false", ""])
            ) as run:
                await Executor().panel("audio", context, {"panelPath": td})
                args = run.call_args.args[0]
                self.assertIn("toggleAt", args)
                self.assertEqual(
                    json.loads(args[-1]), {"screen": "DP-2", "x": 50, "bottom": 900}
                )
            # Crossing from the icon to the number must not toggle the mixer shut.
            with patch("bartjik_core.actions.command", AsyncMock(return_value="true")) as run:
                await Executor().panel("audio", dict(context, event="hoverEnter"), {"panelPath": td})
                run.assert_awaited_once()
                self.assertEqual(run.call_args.args[0][-1], "isOpen")
            with patch(
                "bartjik_core.actions.command",
                AsyncMock(side_effect=[Unavailable("not running"), "", "true"]),
            ) as run:
                await Executor().panel("network", context, {"panelPath": td})
                launch = run.call_args_list[1]
                self.assertIn("-n", launch.args[0])
                self.assertIn("NETWORK_WIDGET_ANCHOR", launch.kwargs["env"])

    async def test_gui_launch_is_detached(self):
        with patch("bartjik_core.actions.subprocess.Popen") as popen:
            await Executor().execute(
                {"command": ["fixture-gui"], "detach": True},
                {"monitor": "DP-2"},
                {},
                {},
                [],
            )
            self.assertTrue(popen.call_args.kwargs["start_new_session"])
