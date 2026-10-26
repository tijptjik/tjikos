import contextlib
import io
from pathlib import Path
import sys
import unittest
import json
import tempfile
import shutil

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import demo_cycle


class DemoCycleTests(unittest.TestCase):
    def test_comparison_keeps_native_tray_and_does_not_force_waybar_height(self):
        with tempfile.TemporaryDirectory() as directory:
            for scene in demo_cycle.SCENES:
                config = json.loads(demo_cycle.waybar_config(Path(directory), "HEADLESS-1", scene).read_text())
                self.assertNotIn("height", config)
                if scene[0] == "applications":
                    self.assertEqual(config["modules-center"], ["tray"])
                    self.assertEqual(config["tray"], demo_cycle.load_waybar_config()[1]["tray"])
                    self.assertFalse(any(key.startswith("custom/tray") for key in config))

    def test_presentation_data_matches_track_workspace_limit_and_tooltip_states(self):
        from bartjik_core.demo import items
        config = demo_cycle.demo_config("HEADLESS-1")
        modules = config["modules"]
        workspaces = items("workspaces", modules["spaces"])
        self.assertEqual([row["id"] for row in workspaces], ["1", "2", "3", "6", "10"])
        player = items("playerctl", modules["music"])[0]["media"]
        self.assertEqual((player["artist"], player["title"], player["playing"]), ("Karla Blum", "Acid Rain", True))
        self.assertEqual(len(items("icontray", modules["tray"])), len(demo_cycle.tray_fixtures()))
        self.assertEqual(items("nightlight", modules["filter"])[0]["tooltip"], "Nightshift : OFF")

    def test_instrumentation_preserves_production_panel_tracking_surface(self):
        with tempfile.TemporaryDirectory() as directory:
            qml = Path(directory) / "qml"
            shutil.copytree(ROOT / "qml", qml)
            before = (qml / "StablePanelSurface.qml").read_bytes()
            demo_cycle.prepare(qml)
            self.assertEqual((qml / "StablePanelSurface.qml").read_bytes(), before)
            self.assertIn("controller.pointer={valid:true", (qml / "Bar.qml").read_text())

    def test_plan_lists_island_groups_then_the_two_requested_endings(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(demo_cycle.main(["cycle", "--plan"]), 0)
        scenes = output.getvalue().splitlines()
        self.assertEqual(scenes[:3], ["markets", "media", "workspaces"])
        self.assertEqual(scenes[-2:], ["persistent-panel", "detached-widgets"])
        self.assertEqual(len(scenes), len(demo_cycle.SCENES) + 2)

    def test_single_island_keeps_all_its_modules_on_the_synthetic_output(self):
        config = demo_cycle.demo_config("HEADLESS-1", demo_cycle.SCENES[5])
        self.assertTrue(config["bars"])
        self.assertTrue(all(bar["monitors"] == ["HEADLESS-1"] for bar in config["bars"]))
        shown = [module for bar in config["bars"] for island in bar["islands"] for module in island["modules"]]
        self.assertEqual(shown, ["net", "vpn", "tailnet"])

    def test_no_scene_can_run_original_waybar_actions_or_providers(self):
        import tempfile
        import json
        with tempfile.TemporaryDirectory() as directory:
            for scene in demo_cycle.SCENES:
                config = json.loads(demo_cycle.waybar_config(Path(directory), "HEADLESS-1", scene).read_text())
                def inspect(value):
                    if isinstance(value, dict):
                        for key, item in value.items():
                            self.assertFalse(key.startswith("on-"), (scene,key))
                            self.assertNotEqual(key, "exec-if")
                            if key == "exec":
                                self.assertTrue(item.startswith(str(Path(directory) / "waybar-fixture.py")))
                            inspect(item)
                    elif isinstance(value, list):
                        for item in value:
                            inspect(item)
                inspect(config)

    def test_waybar_fixture_is_static_and_tooltip_only(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            path = demo_cycle.waybar_config(Path(directory), "HEADLESS-1", demo_cycle.SCENES[-1])
            config = __import__("json").loads(path.read_text())
        self.assertEqual(config["modules-center"], ["custom/notifications", "custom/caffeine", "custom/nightlight", "custom/display-mode"])
        self.assertEqual(config["mode"], "dock")
        for module in config["modules-center"]:
            self.assertTrue(config[module]["tooltip"])
            self.assertFalse(any(key.startswith("on-") for key in config[module]))
