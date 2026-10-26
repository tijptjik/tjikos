import asyncio
import unittest
from unittest.mock import patch, AsyncMock
from bartjik_core import pointer, config


class PointerTests(unittest.IsolatedAsyncioTestCase):
    def test_connector_scale_transform_and_negative_origins(self):
        screens = [
            dict(name="left", x=-1920, y=0, width=3840, height=2160, scale=2),
            dict(
                name="portrait", x=0, y=0, width=1920, height=1080, scale=1, transform=1
            ),
        ]
        self.assertEqual(
            pointer.local_position(dict(x=-1, y=1079), screens),
            dict(valid=True, monitor="left", x=1919, y=1079),
        )
        self.assertEqual(
            pointer.local_position(dict(x=100, y=1900), screens)["monitor"], "portrait"
        )
        self.assertEqual(
            pointer.local_position(dict(x=2000, y=2000), screens)["monitor"], ""
        )

    async def test_unavailable_reconnect_and_cleanup(self):
        events = []
        monitor = dict(name="DP-1", x=0, y=0, width=1920, height=1080, scale=1)
        with patch.object(
            pointer,
            "query",
            AsyncMock(
                side_effect=[OSError(), [monitor], dict(x=10, y=20), dict(x=11, y=21)]
            ),
        ) as query:
            observer = pointer.Observer(events.append)
            await asyncio.sleep(0.01)
            self.assertFalse(events[0]["pointer"]["valid"])
            await asyncio.sleep(1.02)
            self.assertTrue(events[-1]["pointer"]["valid"])
            await observer.close()
            calls = query.call_count
            await asyncio.sleep(0.07)
            self.assertEqual(query.call_count, calls)
            self.assertTrue(observer.task.done())

    def test_defaults(self):
        self.assertEqual(config.BAR["activationDelay"], 0)
        self.assertEqual(config.BAR["hideDelay"], 0)
        self.assertIs(config.BAR["syncMonitors"], True)


class EdgeConfigTests(unittest.TestCase):
    def test_validation(self):
        for key, value in [
            ("activationDelay", -1),
            ("hideDelay", -1),
            ("fadeDuration", -1),
            ("keepVisibleDistance", "-2px"),
            ("activationEdge", "diagonal"),
            ("syncMonitors", "true"),
            ("syncMonitors", 1),
            ("syncMonitors", None),
            ("screenGutter", "-2px"),
            ("screenGutter", "automatic"),
        ]:
            with self.assertRaises(ValueError, msg=key):
                config.bar({**config.BAR, "id": "test", key: value}, {})
        config.bar({**config.BAR, "id": "test", "keepVisibleDistance": "40%"}, {})

    def test_monitor_sync_override(self):
        import json
        import tempfile
        from pathlib import Path

        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "config.json"
            data = {"bars": [{"id": "test", "perMonitor": {"DP-2": {"syncMonitors": False}}}]}
            path.write_text(json.dumps(data))
            cfg = config.load(path)
            for index, name in enumerate(("DP-1", "DP-2")):
                screen = dict(name=name, width=1920, height=1080, scale=1)
                resolved = config.resolve(cfg, screen, index)
                self.assertIs(resolved[0]["syncMonitors"], name == "DP-1")
            data["bars"][0]["perMonitor"]["DP-2"]["syncMonitors"] = "false"
            path.write_text(json.dumps(data))
            with self.assertRaisesRegex(ValueError, "syncMonitors must be boolean"):
                config.load(path)


class ObserverLifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def test_one_observer_across_bars_reload_and_cleanup(self):
        import json
        import tempfile
        from pathlib import Path
        from bartjik_core.engine import Engine

        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "config.json"
            data = {
                "bars": [{"id": "a", "autoHide": True}, {"id": "b", "autoHide": True}]
            }
            path.write_text(json.dumps(data))
            with (
                patch.dict("os.environ", {"BARTJIK_CONFIG": str(path)}),
                patch.object(pointer, "Observer") as constructor,
            ):
                observer = constructor.return_value
                observer.close = AsyncMock()
                engine = Engine(lambda value: None, demo_mode=True)
                engine.screens = [dict(name="DP-1", width=1920, height=1080, scale=1)]
                await engine.reload()
                await engine.reload(True)
                self.assertEqual(constructor.call_count, 1)
                data["bars"][0]["autoHide"] = False
                path.write_text(json.dumps(data))
                await engine.reload()
                observer.close.assert_not_awaited()
                data["bars"][1]["autoHide"] = False
                path.write_text(json.dumps(data))
                await engine.reload()
                observer.close.assert_awaited_once()
                self.assertIsNone(engine.pointer)
                await engine.close()
