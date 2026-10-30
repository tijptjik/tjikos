import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tests/reference"))
import comparison


class ComparisonTests(unittest.TestCase):
    def test_demo_preserves_production_presentation(self):
        import copy
        cfg = {"theme":"monochrome", "style":{"width":480,"fontSize":17,"border":"#123456"},
               "placement":{"edge":"b","position":"c","offsetY":-24,"margin":16},
               "grouping":{"enabled":True}, "history":{},
               "components":{"results":"file:///tmp/Custom%20Results.qml"}}
        with patch("notjik.load_config", return_value=copy.deepcopy(cfg)):
            result = comparison.comparison_config(Path("/unused"), "DP-1")
        self.assertEqual(result["style"], cfg["style"])
        self.assertEqual(result["theme"], cfg["theme"])
        self.assertEqual(result["components"], {"results":"/tmp/Custom Results.qml"})
        self.assertEqual(result["placement"]["offsetX"], 520)
        self.assertEqual(result["placement"]["offsetY"], -24)

    def test_stale_demo_is_restarted_before_reuse(self):
        with patch.object(comparison, "state", return_value={"signature":"old"}), \
             patch.object(comparison, "presentation_signature", return_value="new"), \
             patch.object(comparison, "stop") as stop, \
             patch.object(comparison.shutil, "which", return_value="/missing/dunst"):
            with self.assertRaisesRegex(ValueError, "Dunst comparison binary missing"):
                comparison.start(Path("/unused"), None)
            stop.assert_called_once()

    def test_replacement_ids_are_mapped_per_private_bus(self):
        session = {"buses": {"dunst": "unix:path=/test/dunst", "notjik": "unix:path=/test/notjik"}}
        captured = []
        def run(command, **kwargs):
            captured.append((command, kwargs["env"]["DBUS_SESSION_BUS_ADDRESS"]))
            return SimpleNamespace(returncode=0, stdout="12\n", stderr="")
        with patch.object(comparison.subprocess, "run", side_effect=run):
            comparison.send(session, ["-h", "int:value:67", "Upload", "body; literal"], {"dunst": 5, "notjik": 9})
        self.assertEqual(len(captured), 2)
        for command, address in captured:
            self.assertEqual(command[1:3], ["--replace-id", "5" if address.endswith("dunst") else "9"])
            self.assertEqual(command[-1], "body; literal")
            self.assertTrue(address.startswith("unix:path=/test/"))

    def test_malformed_send_explains_separate_commands(self):
        session = {"buses": {"dunst": "unix:path=/test/dunst", "notjik": "unix:path=/test/notjik"}}
        failure = SimpleNamespace(returncode=1, stdout="", stderr="Invalid number of options.")
        with patch.object(comparison.subprocess, "run", return_value=failure):
            with self.assertRaisesRegex(ValueError, "separate lines"):
                comparison.send(session, ["Title", "Bodynotjikctl", "compare", "send"])

    def test_stop_does_not_signal_reused_pid(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(comparison, "runtime_dir", return_value=Path(directory)):
            (Path(directory) / "session.json").write_text(json.dumps({"pid":123, "birth":"old"}))
            with patch.object(comparison, "birth", return_value="new"), patch.object(comparison.os, "kill") as kill:
                comparison.stop()
                kill.assert_not_called()

    def test_demo_native_translation_never_waits_for_actions(self):
        session = {"buses":{"dunst":"unix:path=/test/dunst"}}
        notice = {"app":"notjik demo", "title":"Upload", "body":"Body", "type":"progress", "data":{"value":67,"label":"Remaining"},
                  "actions":[{"label":"Open", "command":["must-not-execute"]}], "resident":True, "timeout":0, "image":"file:///tmp/demo.svg"}
        with patch.object(comparison.subprocess, "run", return_value=SimpleNamespace(returncode=0, stdout="u 42\n", stderr="")) as run:
            self.assertEqual(comparison.demo_notify(session, notice, 12), 42)
        command = run.call_args.args[0]
        self.assertEqual(command[0], "busctl")
        self.assertNotIn("must-not-execute", command)
        self.assertIn("Body\nRemaining", command)
        signature = command.index("susssasa{sv}i")
        self.assertEqual(command[signature+2], "12")
        self.assertIn("image-path", command)
        self.assertIn("default", command)
        self.assertIn("67", command)

    def test_demo_rich_layouts_have_textual_reference(self):
        session = {"buses":{"dunst":"unix:path=/test/dunst"}}
        for kind, data, expected in (("status", {"state":"Failed", "detail":"Build 184"}, "Failed · Build 184"),
                                     ("timer", {"seconds":90,"label":"Waiting"}, "01:30 · Waiting")):
            with self.subTest(kind=kind), patch.object(comparison.subprocess, "run", return_value=SimpleNamespace(returncode=0, stdout="u 1\n", stderr="")) as run:
                comparison.demo_notify(session, {"app":"notjik demo", "title":"Test", "type":kind, "data":data})
                self.assertIn(expected, run.call_args.args[0])
