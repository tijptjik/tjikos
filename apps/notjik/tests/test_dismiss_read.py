import copy
import json
import os
import shutil
import subprocess
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
import notjik
import telegram_bridge as telegram
from test_telegram import FakeTD


class DismissReadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        env = patch.dict(os.environ, {"NOTJIK_CONFIG": str(self.path / "config.json"),
                                     "NOTJIK_TELEGRAM_DIR": str(self.path / "telegram")})
        env.start()
        self.addCleanup(env.stop)
        self.td = FakeTD()
        self.bridge = telegram.Bridge(self.td, lambda notice: None, lambda ident: None)
        self.addCleanup(self.bridge.media_pool.shutdown)
        self.entry = {"message": {"chat_id": 123, "id": 42}}
        self.results = []
        self.configure()

    def configure(self, rules=None, mode="safe"):
        (self.path / "config.json").write_text(json.dumps({"telegram": {
            "dismissIsRead": ["123"] if rules is None else rules, "dismissReadMode": mode}}))

    def respond(self, result):
        self.td.calls[-1][2](result)

    def chat(self, **fields):
        self.respond({"@type": "chat", "id": 123, "last_read_inbox_message_id": 40,
                      "unread_count": 2, **fields})

    def test_configuration_validation(self):
        self.assertEqual(notjik.load_config(ROOT)["telegram"]["dismissReadMode"], "safe")
        for rules in (True, [123], [""], ["display name"], ["0"], ["9007199254740992"]):
            self.configure(rules)
            with self.assertRaisesRegex(ValueError, "dismissIsRead"):
                notjik.load_config(ROOT)
        self.configure(["@Example_Bot", "-1001234567890"])
        notjik.load_config(ROOT)
        self.configure(mode="single")
        with self.assertRaisesRegex(ValueError, "dismissReadMode"):
            notjik.load_config(ROOT)

    def test_only_explicit_real_dismissals_are_forwarded(self):
        cfg = notjik.load_config(ROOT)
        event = {"event": "user-dismissed", "notice": {"type": "telegram", "app": telegram.APP,
                 "data": {"chatId": "123", "messageId": "42"}}}
        self.assertEqual(notjik.dismiss_read_request(cfg, event)["action"], "dismissRead")
        for reason in ("dismissed", "expired", "action", "suppressed", "sender-closed"):
            self.assertIsNone(notjik.dismiss_read_request(cfg, {**event, "event": reason}))
        for field, value in (("archived", True), ("app", "Telegram Desktop")):
            changed = copy.deepcopy(event)
            changed["notice"][field] = value
            self.assertIsNone(notjik.dismiss_read_request(cfg, changed))
        event["notice"]["data"]["demo"] = True
        self.assertIsNone(notjik.dismiss_read_request(cfg, event))

    def test_safe_skips_earlier_unread_and_incomplete_history(self):
        for messages in ([], [{"id": 42, "chat_id": 123}],
                         [{"id": 42, "chat_id": 123}, {"id": 41, "chat_id": 123}]):
            self.bridge.dismiss_read(self.entry, self.results.append)
            self.chat()
            self.assertEqual(self.td.calls[-1][0], "getChatHistory")
            self.respond({"@type": "messages", "messages": messages})
            self.assertIn("skipped", self.results[-1])
        self.assertNotIn("viewMessages", [call[0] for call in self.td.calls])

    def test_safe_reads_oldest_unread(self):
        self.bridge.dismiss_read(self.entry, self.results.append)
        self.chat()
        self.respond({"@type": "messages", "messages": [
            {"id": 42, "chat_id": 123}, {"id": 40, "chat_id": 123}]})
        self.assertEqual(self.td.calls[-1][:2], ("viewMessages", {
            "chat_id": 123, "message_ids": [42], "source": {"@type": "messageSourceNotification"}, "force_read": True}))
        self.respond({"@type": "ok"})
        self.assertEqual(self.results[-1], {"ok": True})

    def test_safe_only_unread_latest_message(self):
        self.bridge.dismiss_read(self.entry, self.results.append)
        self.chat(unread_count=1, last_message={"id": 42})
        self.assertEqual(self.td.calls[-1][0], "viewMessages")

    def test_safe_skips_topics_and_already_read(self):
        self.bridge.dismiss_read({"message": {**self.entry["message"], "message_thread_id": 12}}, self.results.append)
        self.assertEqual(self.td.calls, [])
        self.bridge.dismiss_read(self.entry, self.results.append)
        self.chat(last_read_inbox_message_id=42)
        self.assertIn("skipped", self.results[-1])

    def test_username_must_resolve_to_actual_chat_and_through_is_explicit(self):
        self.configure(["@example_bot"], "through")
        self.bridge.dismiss_read(self.entry, self.results.append)
        self.assertEqual(self.td.calls[-1][:2], ("searchPublicChat", {"username": "example_bot"}))
        self.respond({"@type": "chat", "id": 456, "title": "example_bot"})
        self.assertIn("skipped", self.results[-1])
        self.bridge.dismiss_read(self.entry, self.results.append)
        self.respond({"@type": "chat", "id": 123})
        self.assertEqual(self.td.calls[-1][0], "viewMessages")
        self.respond({"@type": "error", "message": "denied"})
        self.assertEqual(self.results[-1], {"ok": False, "error": "denied"})

    def test_ctrl_click_bypasses_allowlist_but_never_safe_checks(self):
        self.configure([], "through")
        event = {"event": "user-safe-read", "notice": {"type": "telegram", "app": telegram.APP,
                 "data": {"chatId": "123", "messageId": "42"}}}
        request = notjik.dismiss_read_request(notjik.load_config(ROOT), event)
        self.assertEqual(request["action"], "safeRead")
        self.bridge.entries[telegram.message_key(123, 42)] = self.entry
        self.bridge.action(request, self.results.append)
        self.assertEqual(self.td.calls[-1][0], "getChat")
        self.chat()
        self.respond({"@type": "messages", "messages": [
            {"id": 42, "chat_id": 123}, {"id": 41, "chat_id": 123}]})
        self.assertIn("skipped", self.results[-1])
        self.assertNotIn("viewMessages", [call[0] for call in self.td.calls])
        self.bridge.action(request, self.results.append)
        self.chat(unread_count=1, last_message={"id": 42})
        self.assertEqual(self.td.calls[-1][0], "viewMessages")
        event["notice"]["data"]["demo"] = True
        self.assertIsNone(notjik.dismiss_read_request(notjik.load_config(ROOT), event))

    @unittest.skipUnless(shutil.which("bun"), "Bun required for QML JavaScript checks")
    def test_qml_dismiss_only_reads_visible_cards_and_freezes_bulk_selection(self):
        source = (ROOT / "qml/shell.qml").read_text()
        functions = source[source.index("    function dismissReadIds()"):source.index("    function close(id, expired)")]
        bulk = source[source.index('        case "dismiss": {'):source.index('        case "openTelegramChat":')]
        script = r"""
const assert = require('node:assert/strict');
let ready=true, paused=false, locked=false, screens=[{}], draggingId='';
let floatingIds=[], visibleNotices=[], notices=[], events=[];
function find(id) { return notices.find(n => n.id === id); }
function log(event, notice) { events.push([event,notice.id]); }
function close(id) {
    notices=notices.filter(n => n.id !== id);
    visibleNotices=notices.slice(0,1);
}
function reset() {
    notices=[{id:'shown'},{id:'queued'}];
    visibleNotices=notices.slice(0,1);events=[];
}
reset();paused=true;dismissNotice('shown');assert.deepEqual(events,[]);paused=false;
reset();locked=true;dismissNotice('shown');assert.deepEqual(events,[]);locked=false;
reset();dismissNotice('queued');assert.deepEqual(events,[]);
reset();dismissNotice('shown',true);assert.deepEqual(events,[['user-safe-read','shown']]);
reset();bulkDismiss({op:'dismiss',id:'all'});
assert.deepEqual(events,[['user-dismissed','shown']]);assert.equal(notices.length,0);
"""
        subprocess.run(["bun", "-e", functions + '\nfunction bulkDismiss(request) {switch(request.op) {\n'
                        + bulk + '}}\n' + script], check=True)

    def test_default_disabled_and_live_config_reload(self):
        self.configure([])
        self.bridge.dismiss_read(self.entry, self.results.append)
        self.assertEqual(self.td.calls, [])
        self.configure(mode="through")
        self.bridge.dismiss_read(self.entry, self.results.append)
        self.assertEqual(self.td.calls[-1][0], "viewMessages")


if __name__ == "__main__":
    unittest.main()
