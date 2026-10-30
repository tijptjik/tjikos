import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import telegram_clipboard as clipboard
from test_telegram import FakeTD


class ClipboardTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.td, self.notices = FakeTD(), []
        self.sender = clipboard.SavedClipboard(self.td, self.notices.append, self.root)

    def snapshot(self, types, data):
        with patch.object(clipboard, "clipboard", side_effect=[types.encode(), data]):
            return clipboard.snapshot(self.root)

    def resolve_self(self, user_id=42):
        self.assertEqual(self.td.calls[-1][0], "getMe")
        self.td.calls[-1][2]({"@type": "user", "id": user_id})
        self.assertEqual(self.td.calls[-1][1], {"user_id": user_id, "force": True})
        self.td.calls[-1][2]({"@type": "chat", "id": user_id,
                             "type": {"@type": "chatTypePrivate", "user_id": user_id}})

    def confirm(self, message_id=99):
        self.td.calls[-1][2]({"@type": "message", "chat_id": 42, "id": message_id,
                             "sending_state": {"@type": "messageSendingStatePending"}})
        self.assertFalse(any(item["title"] == "Saved to Telegram" for item in self.notices))
        self.sender.handle({"@type": "updateMessageSendSucceeded", "old_message_id": message_id,
                            "message": {"chat_id": 42}})

    def test_text_is_literal_and_success_waits_for_delivery(self):
        text = "**literal** <tag> 中文\nsecond line\n"
        token = self.snapshot("text/plain", text.encode())
        self.sender.start(token)
        self.resolve_self()
        method, fields, _ = self.td.calls[-1]
        self.assertEqual(method, "sendMessage")
        self.assertEqual(fields["chat_id"], 42)
        self.assertEqual(fields["input_message_content"]["text"]["text"], text)
        self.assertEqual(fields["input_message_content"]["text"]["entities"], [])
        self.confirm()
        self.assertEqual(len(self.notices), 1)
        self.assertEqual(self.notices[0]["timeout"], 1500)
        self.assertTrue(self.notices[0]["transient"])
        self.assertFalse((self.root / token).exists())

    def test_wrong_destination_never_sends(self):
        token = self.snapshot("text/plain", b"example")
        self.sender.start(token)
        self.td.calls[-1][2]({"@type": "user", "id": 42})
        self.td.calls[-1][2]({"@type": "chat", "id": 7, "type": {"@type": "chatTypePrivate", "user_id": 7}})
        self.assertFalse(any(call[0] == "sendMessage" for call in self.td.calls))
        self.assertEqual(self.notices[-1]["timeout"], 5000)

    def test_image_preserved_as_private_document_until_confirmed(self):
        token = self.snapshot("image/png\ntext/plain", b"synthetic image fixture")
        self.sender.start(token)
        self.resolve_self()
        content = self.td.calls[-1][1]["input_message_content"]
        self.assertEqual(content["@type"], "inputMessageDocument")
        self.assertEqual(content["document"]["@type"], "inputDocument")
        path = Path(content["document"]["document"]["path"])
        self.assertEqual(path.name, "clipboard.png")
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.confirm()
        self.assertFalse(path.exists())

    def test_copied_files_are_snapshots_and_all_must_confirm(self):
        paths = [self.root / "one.txt", self.root / "two.txt"]
        for path in paths:
            path.write_text(path.name)
        token = self.snapshot("text/uri-list", "\r\n".join(p.as_uri() for p in paths).encode())
        paths[0].unlink()
        self.sender.start(token)
        self.resolve_self()
        self.confirm(10)
        self.assertEqual(self.notices, [])
        self.confirm(11)
        self.assertEqual(len(self.notices), 1)

    def test_long_unicode_text_becomes_document_without_truncation(self):
        data = ("🌱" * 2049).encode()
        token = self.snapshot("text/plain;charset=utf-8", data)
        self.assertEqual((self.root / token / "0/clipboard.txt").read_bytes(), data)

    def test_empty_unsupported_folder_and_remote_selection_rejected(self):
        for types, data in (("text/plain", b" \n"), ("application/octet-stream", b""),
                            ("text/uri-list", self.root.as_uri().encode()),
                            ("text/uri-list", b"file://remote/private")):
            with self.subTest(types=types, data=data), self.assertRaises(ValueError):
                self.snapshot(types, data)

    def test_failure_and_timeout_do_not_claim_success_or_retry(self):
        token = self.snapshot("text/plain", b"example")
        self.sender.start(token)
        self.resolve_self()
        self.td.calls[-1][2]({"@type": "message", "chat_id": 42, "id": 99,
                             "sending_state": {"@type": "messageSendingStatePending"}})
        self.sender.jobs[token]["deadline"] = 0
        before = len(self.td.calls)
        self.sender.poll()
        self.sender.poll()
        self.assertEqual(len(self.notices), 1)
        self.assertEqual(len(self.td.calls), before)
        self.sender.handle({"@type": "updateMessageSendFailed", "old_message_id": 99, "message": {"chat_id": 42}})
        self.assertFalse(any(item["title"] == "Saved to Telegram" for item in self.notices))
        self.assertTrue((self.root / token).exists())

    def test_duplicate_and_outside_attachment_rejected(self):
        token = self.snapshot("text/plain", b"example")
        self.sender.start(token)
        with self.assertRaises(ValueError):
            self.sender.start(token)
        outside = self.root / "outside.txt"
        outside.write_text("example")
        token = self.snapshot("text/plain", b"example")
        (self.root / token / "items.json").write_text(json.dumps([{"file": "../outside.txt"}]))
        with self.assertRaises(ValueError):
            self.sender.start(token)

    def test_cli_does_not_read_clipboard_until_bridge_ready(self):
        with patch("notjik.ipc") as ipc, patch.object(clipboard, "snapshot") as snapshot:
            result = clipboard.save(self.root, self.root, lambda _: {"authorized": False})
        self.assertEqual(result, 1)
        snapshot.assert_not_called()
        self.assertEqual(ipc.call_args[0][1]["notice"]["timeout"], 5000)


if __name__ == "__main__":
    unittest.main()
