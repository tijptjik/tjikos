import contextlib
import io
import json
import tempfile
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
import demo
import comparison
import notjik


class DemoTests(unittest.TestCase):
    def test_clear_preserves_other_senders_and_handles_expiration(self):
        requests = []
        def call(request):
            requests.append(request)
            if request["op"] == "list":
                return {"notices": [{"app":"mail", "id":"real"}, {"app":demo.APP, "id":"demo"}]}
            raise ValueError("Notification not found")
        self.assertEqual(demo.clear(call), 0)
        self.assertEqual(requests[1:], [{"op":"dismiss", "id":"demo"}])

    def test_all_fixtures_are_transient_and_actions_are_harmless(self):
        captured = []
        def call(request):
            captured.append(request["notice"])
            return {"id":request["notice"]["id"]}
        with patch.object(demo.time, "sleep"):
            for name in demo.TYPES:
                demo.show(ROOT, name, call)
        self.assertTrue(all(n["transient"] and n["app"] == demo.APP for n in captured))
        self.assertTrue(all(action["command"] == ["true"] for n in captured for action in n.get("actions", [])))
        self.assertTrue((ROOT / "examples/demo-artwork.png").is_file())
        updates = [n for n in captured if n["id"] == "demo-update-0"]
        self.assertEqual([n["data"]["value"] for n in updates], [10,25,50,75,100])
        self.assertEqual(len({n["group"] for n in captured if n["id"].startswith("demo-grouped-")}), 1)
        self.assertEqual(len({n["group"] for n in captured if n["id"].startswith("demo-stack-")}), 4)

    def test_telegram_fixtures_use_local_media_and_synthetic_identities(self):
        import wave
        from urllib.parse import unquote, urlparse
        for kind in ("text", "photo", "voice"):
            notice = demo.fixtures(ROOT, "telegram-" + kind)[0]
            self.assertEqual(notice["type"], "telegram")
            self.assertTrue(notice["data"]["demo"])
            self.assertEqual(notice["data"]["chatId"], "1")
            self.assertEqual(notice["data"]["messageId"], "1")
            if kind != "text":
                source = Path(unquote(urlparse(notice["data"]["source"]).path))
                self.assertTrue(source.is_file())
            if kind == "voice":
                with wave.open(str(source)) as sound:
                    self.assertEqual(sound.getnframes() / sound.getframerate(), notice["data"]["duration"])
                self.assertEqual(len(notice["data"]["waveform"]), 100)
                self.assertTrue(all(0 <= peak <= 31 for peak in notice["data"]["waveform"]))

    def test_grouped_telegram_has_one_chat_and_three_distinct_messages(self):
        notices = []
        demo.show(ROOT, "telegram-grouped", lambda request: notices.append(request["notice"]) or {"id":request["notice"]["id"]})
        self.assertEqual([n["data"]["kind"] for n in notices], ["text", "photo", "voice"])
        self.assertEqual({n["group"] for n in notices}, {"telegram-grouped"})
        self.assertEqual({n["data"]["chatId"] for n in notices}, {"1"})
        self.assertEqual(len({n["data"]["messageId"] for n in notices}), 3)
        self.assertEqual(len({n["id"] for n in notices}), 3)
        self.assertTrue(all(n["transient"] and n["data"]["demo"] for n in notices))

    def test_timeout_and_logging_overrides(self):
        calls = []
        demo.show(ROOT, "timeout", lambda req: calls.append(req) or {"id":"test"}, timeout=9000, log=True)
        self.assertEqual(calls[0]["notice"]["timeout"], 9000)
        self.assertFalse(calls[0]["notice"]["transient"])

    def test_list_does_not_need_a_running_server(self):
        with patch.object(notjik, "ipc") as ipc, contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(demo.main(ROOT, ["list"]), 0)
            ipc.assert_not_called()
        self.assertTrue(all(name in output.getvalue() for name in demo.TYPES))

    def test_paused_server_does_not_clear_or_resume(self):
        with patch.object(notjik, "ipc", return_value={"paused":True}) as ipc, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(demo.main(ROOT, ["standard", "--solo"]), 1)
        self.assertEqual(ipc.call_count, 1)
        self.assertEqual(ipc.call_args.args[1], {"op":"status"})

    def test_invalid_type_rejected_before_any_changes(self):
        with patch.object(notjik, "ipc") as ipc, contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            demo.main(ROOT, ["not-a-demo"])
        ipc.assert_not_called()

    def test_paired_mapping_is_not_reused_across_sessions(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            (path / "demo-ids.json").write_text(json.dumps({"birth":"old", "ids":{"old-notjik":42}}))
            with patch.object(comparison, "runtime_dir", return_value=path), patch.object(notjik, "ipc", return_value={"ok":True,"lockState":"unlocked","notices":[]}), patch.object(comparison, "demo_close") as close:
                paired = demo.PairedDemo({"birth":"new", "app":str(ROOT)})
                self.assertEqual(paired.ids, {})
                paired.clear()
                close.assert_not_called()

    def test_next_tracks_successful_examples_and_wraps(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "cursor.json"
            def call(root, request):
                return {"paused":False,"lockState":"unlocked","drawerOpen":False} if request["op"] == "status" else {"notices":[]}
            with patch.object(demo, "cursor_path", return_value=path), patch.object(notjik, "ipc", side_effect=call), patch.object(demo, "show", return_value=["id"]) as show, contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(demo.main(ROOT, ["next", "--solo"]), 0)
                self.assertEqual(show.call_args.args[1], "standard")
                self.assertEqual(demo.main(ROOT, ["next", "--solo"]), 0)
                self.assertEqual(show.call_args.args[1], "title-only")
                demo.main(ROOT, ["actions-wrapped", "--solo"])
                demo.main(ROOT, ["next", "--solo"])
                self.assertEqual(show.call_args.args[1], "resident")
                demo.main(ROOT, [list(demo.TYPES)[-1], "--solo"])
                demo.main(ROOT, ["clear", "--solo"])
                demo.main(ROOT, ["next", "--solo"])
                self.assertEqual(show.call_args.args[1], "standard")
                demo.main(ROOT, ["overview", "--solo"])
                self.assertEqual(json.loads(path.read_text())["last"], "timer")
                show.side_effect = ValueError("send failed")
                self.assertEqual(demo.main(ROOT, ["next", "--solo"]), 1)
                self.assertEqual(json.loads(path.read_text())["last"], "timer")

    def test_next_recovers_missing_or_invalid_cursor_from_current_demo(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "cursor.json"
            call = lambda request: {"notices":[
                {"app":demo.APP, "id":"named-notjik%20demo/demo-actions-wrapped-0"},
                {"app":"other", "id":"named-other/demo-stack-0"}]}
            self.assertEqual(demo.next_type(path, call), "resident")
            for invalid in ('bad json', '[]', '{"last":[]}', '{"last":"removed"}'):
                path.write_text(invalid)
                self.assertEqual(demo.next_type(path, call), "resident")
            self.assertEqual(demo.next_type(path, lambda request: {"notices":[]}), "standard")

    def test_paired_and_solo_cursors_are_separate(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(comparison, "runtime_dir", return_value=Path(temp)/"pair"), patch.object(notjik, "state_path", return_value=Path(temp)/"solo"):
            demo.save_cursor(demo.cursor_path(True), list(demo.TYPES)[-1])
            demo.save_cursor(demo.cursor_path(False), "icon")
            call = lambda request: {"notices":[]}
            self.assertEqual(demo.next_type(demo.cursor_path(True), call), "standard")
            self.assertEqual(demo.next_type(demo.cursor_path(False), call), "missing-icon")

    def test_robot_preview_discovers_artwork_and_updates_one_card(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            icons = root / "qml/icons"
            (icons / "robots").mkdir(parents=True)
            for relative in ("robot-00-cover.png", "robot-cover-painterly-v1.png", "vintage-robot.png", "robots/robot-02-greek.png", "robots/robot-01-bronze-age.png", "robots/robot-88-dream-cosmic.png", "robots/robot-65-alert-bronze.png", "wifi-cover.png"):
                (icons / relative).touch()
            requests = []
            with patch.object(demo.time, "sleep") as sleep, contextlib.redirect_stdout(io.StringIO()):
                demo.show_robots(root, requests.append, 500)
            notices = [request["notice"] for request in requests]
            self.assertEqual(len(notices), 5)
            self.assertEqual(len({notice["id"] for notice in notices}), 1)
            self.assertEqual(len({notice["image"] for notice in notices}), 5)
            self.assertTrue(notices[0]["image"].endswith("robot-00-cover.png"))
            self.assertTrue(notices[2]["image"].endswith("robot-02-greek.png"))
            self.assertTrue(notices[3]["image"].endswith("robot-65-alert-bronze.png"))
            self.assertTrue(notices[4]["image"].endswith("robot-88-dream-cosmic.png"))
            self.assertEqual([n["urgency"] for n in notices], ["normal", "normal", "normal", "critical", "low"])
            self.assertTrue(all(notice["transient"] and notice["timeout"] == 0 for notice in notices))
            self.assertEqual(sleep.call_count, 4)
            sleep.assert_called_with(.5)

    def test_robot_preview_shares_rendered_artwork_with_reference(self):
        paired = object.__new__(demo.PairedDemo)
        paired.session = {"app": str(ROOT)}
        paired.ids = {}
        notice = {"image": "file:///robot.png", "data": {"demoRobot": True}}
        with patch.object(notjik, "ipc", side_effect=[{"id":"robot"}, {"image":"file:///robot.png"}]), patch.object(comparison, "demo_notify", return_value=42) as notify, patch.object(paired, "save"):
            paired({"op": "send", "notice": notice})
            self.assertEqual(notify.call_args.args[1]["image"], "file:///robot.png")
            self.assertEqual(notice["image"], "file:///robot.png")


class SharedFallbackTests(unittest.TestCase):
    def test_random_fallback_is_read_from_the_notjik_notice(self):
        paired = object.__new__(demo.PairedDemo)
        paired.session = {"app":str(ROOT)}
        paired.ids = {}
        notice = {"title":"Reactor fault", "urgency":"critical"}
        with patch.object(notjik, "ipc", side_effect=[{"id":"fault"}, {"image":"file:///robot-66-alert-clockwork.png"}]) as ipc, patch.object(comparison, "demo_notify", return_value=42) as notify, patch.object(paired, "save"):
            paired({"op":"send", "notice":notice})
            self.assertEqual(ipc.call_args.args[1], {"op":"reference-artwork", "id":"fault"})
            self.assertEqual(notify.call_args.args[1]["image"], "file:///robot-66-alert-clockwork.png")
            self.assertNotIn("image", notice)
