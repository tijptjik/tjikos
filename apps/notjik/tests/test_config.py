import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
spec = importlib.util.spec_from_file_location("notjik", ROOT / "tests/reference/notjik.py")
notjik = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notjik)


class ConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        env = patch.dict(os.environ, {"NOTJIK_CONFIG": str(self.path / "config.json"), "NOTJIK_STATE_DIR": str(self.path / "state")})
        env.start()
        self.addCleanup(env.stop)

    def write(self, value):
        (self.path / "config.json").write_text(json.dumps(value))

    def test_defaults_and_theme_override(self):
        self.assertEqual(notjik.load_config(ROOT)["theme"], "monochrome")
        self.assertEqual(notjik.load_config(ROOT)["placement"]["monitors"], "primary")
        self.assertEqual(notjik.load_config(ROOT)["stack"], {"animationMs":220,"heightPercent":75})
        self.write({"theme": "rose-punk", "style": {"width": 500}, "timeouts": {"critical": 30}})
        cfg = notjik.load_config(ROOT)
        self.assertEqual(cfg["style"]["accent"], "#ff53a6")
        self.assertEqual(cfg["style"]["width"], 500)
        self.assertEqual(cfg["timeouts"], {"low": 5, "normal": 8, "critical": 30})

    def test_native_hooks_configuration(self):
        hook = {"caller": "chatgpt", "identifier": "default", "command": ["show-codex"]}
        self.write({"actions": {"nativeHooks": [hook]}})
        self.assertEqual(notjik.load_config(ROOT)["actions"]["nativeHooks"], [hook])
        for hooks in (None, {}, [None], [{}], [dict(hook, command="show-codex")], [dict(hook, command=[])], [dict(hook, command=[1])]):
            self.write({"actions": {"nativeHooks": hooks}})
            with self.assertRaisesRegex(ValueError, "actions.nativeHooks"):
                notjik.load_config(ROOT)

    def test_action_blacklist_configuration(self):
        self.assertEqual(notjik.load_config(ROOT)["actions"], {"blacklist": []})
        self.write({"actions": {"blacklist": ["telegram", "org.telegram.desktop"]}})
        self.assertEqual(notjik.load_config(ROOT)["actions"]["blacklist"], ["telegram", "org.telegram.desktop"])
        for actions in (None, [], {"blacklist": "telegram"}, {"blacklist": None},
                        {"blacklist": [1]}, {"blacklist": [""]}, {"blacklist": ["  "]}):
            with self.subTest(actions=actions):
                self.write({"actions": actions})
                with self.assertRaisesRegex(ValueError, "actions"):
                    notjik.load_config(ROOT)

    def test_invalid_configuration_fails(self):
        for value in ({"placement": {"edge": "x"}}, {"placement": {"offsetX": "4em"}},
                      {"placement": {"monitors": [0]}}, {"timeouts": {"low": -1}},
                      {"maxVisible": 2.5}, {"history": {"maxEvents": 0}},
                      {"style": {"width": 0}}, {"style": {"text": "bogus"}}):
            with self.subTest(value=value):
                self.write(value)
                with self.assertRaises(ValueError):
                    notjik.load_config(ROOT)

    def test_weekly_history_configuration(self):
        self.write({"history": {"maxEvents": 0, "maxAgeDays": 7, "drawerLimit": 0}})
        history = notjik.load_config(ROOT)["history"]
        self.assertEqual((history["maxEvents"], history["maxAgeDays"], history["drawerLimit"]), (0, 7, 0))
        self.write({"history": {"drawerLimit": 1000}})
        self.assertEqual(notjik.load_config(ROOT)["history"]["drawerLimit"], 1000)
        for history in ({"maxAgeDays": -1}, {"maxAgeDays": True}, {"maxAgeDays": "7"},
                        {"maxEvents": 0, "maxAgeDays": 0}, {"maxEvents": 1.5},
                        {"drawerLimit": -1}, {"drawerLimit": 1.5}, {"drawerLimit": True}):
            with self.subTest(history=history):
                self.write({"history": history})
                with self.assertRaisesRegex(ValueError, "history"):
                    notjik.load_config(ROOT)

    def test_custom_component_is_resolved_against_config(self):
        (self.path / "Task.qml").write_text("import QtQuick\nItem {}")
        self.write({"components": {"task": "Task.qml"}})
        self.assertEqual(notjik.load_config(ROOT)["components"]["task"], (self.path / "Task.qml").as_uri())

    def test_duration_units(self):
        for value, expected in [("default", -1), ("never", 0), ("until-dismissed", 0),
                                ("500ms", 500), ("2m", 120000), ("0.5", 500)]:
            self.assertEqual(notjik.duration(value), expected)

    def test_history_retention_privacy_and_transient(self):
        cfg = copy.deepcopy(notjik.DEFAULTS)
        cfg["history"]["maxEvents"] = 2
        db = notjik.database()
        self.addCleanup(db.close)
        event = {"event": "received", "notice": {"id": "local-1", "app": "demo", "title": "hello",
                                                  "actions": [{"label": "Open", "command": ["secret"]}]}}
        for _ in range(3):
            notjik.record(db, cfg, event)
        rows = db.execute("SELECT payload FROM events").fetchall()
        self.assertEqual(len(rows), 2)
        self.assertNotIn("secret", rows[0][0])
        event["notice"]["transient"] = True
        notjik.record(db, cfg, event)
        self.assertEqual(db.execute("SELECT count(*) FROM events").fetchone()[0], 2)
        self.assertEqual((self.path / "state/history.sqlite3").stat().st_mode & 0o777, 0o600)

    def test_terminal_escape_sequences_are_inert(self):
        self.assertNotIn("\x1b", notjik.clean_terminal("\x1b[2Jbad"))


if __name__ == "__main__":
    unittest.main()
