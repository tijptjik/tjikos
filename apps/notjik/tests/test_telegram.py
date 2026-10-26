import base64
import copy
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import telegram_bridge as telegram


class FakeTD:
    def __init__(self):
        self.calls = []

    def send(self, method, callback=None, **fields):
        self.calls.append((method, fields, callback))


class TelegramTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.env = patch.dict(os.environ, {"NOTJIK_TELEGRAM_DIR": self.temp.name})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.td, self.cards, self.closed = FakeTD(), [], []
        self.bridge = telegram.Bridge(self.td, lambda n: self.cards.append(copy.deepcopy(n)), self.closed.append, clock=lambda: 1000)
        self.file = {"id": 5, "size": 12, "local": {"is_downloading_completed": False}}
        self.message = {"id": 42 << 20, "chat_id": 123, "date": 1000, "can_be_saved": True,
                        "content": {"@type": "messagePhoto", "photo": {"sizes": [{"width": 800, "height": 600, "photo": self.file}]},
                                    "caption": {"text": "A photo"}}}

    def add(self, message=None, preview=True, ident=9):
        self.bridge.handle({"@type": "updateNotificationGroup", "notification_group_id": 8, "chat_id": 123,
                            "added_notifications": [{"id": ident, "date": 1000, "type": {
                                "@type": "notificationTypeNewMessage", "message": message or self.message, "show_preview": preview}}]})

    def test_delivery_diagnostics_separate_filtering_and_publish_failure(self):
        self.bridge.publish = lambda notice: (_ for _ in ()).throw(OSError("offline"))
        self.add()
        self.assertEqual(self.bridge.delivery["notificationEvents"], 1)
        self.assertEqual(self.bridge.delivery["publishFailures"], 1)
        self.assertFalse(next(iter(self.bridge.entries.values()))["published"])
        self.bridge.publish = self.cards.append
        self.bridge.show(next(iter(self.bridge.entries)))
        self.assertTrue(next(iter(self.bridge.entries.values()))["published"])
        self.assertEqual(self.bridge.delivery["publishAttempts"], 2)
        self.add(message={**self.message, "is_outgoing":True})
        self.bridge.add(8, 123, {"id":10, "date":900, "type":{
            "@type":"notificationTypeNewMessage", "message":self.message}})
        self.bridge.add(8, 123, {"id":11, "type":{"@type":"notificationTypeNewCall"}})
        self.assertEqual(self.bridge.delivery["ignoredOutgoing"], 1)
        self.assertEqual(self.bridge.delivery["ignoredOld"], 1)
        self.assertEqual(self.bridge.delivery["ignoredType"], 1)
        self.assertEqual(self.bridge.delivery["lastNotificationAt"], 1000)
        self.assertNotIn("A photo", json.dumps(self.bridge.delivery))

    def test_photo_download_and_real_identity(self):
        self.add()
        self.assertEqual(self.cards[-1]["data"]["chatId"], "123")
        self.assertEqual(self.cards[-1]["data"]["messageId"], str(42 << 20))
        self.assertEqual(self.cards[-1]["type"], "telegram")
        self.assertEqual([x[0] for x in self.td.calls], ["getChat", "downloadFile"])
        path = Path(self.temp.name) / "files/photo.jpg"
        path.parent.mkdir()
        path.write_bytes(b"synthetic")
        self.bridge.file_update({**self.file, "local": {"is_downloading_completed": True, "path": str(path)}})
        self.assertEqual(self.cards[-1]["data"]["source"], path.as_uri())
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.cards[0]["id"], self.cards[-1]["id"])

    def test_does_not_read_on_receive_or_download(self):
        self.add()
        self.assertFalse(any(call[0] in ("viewMessages", "openMessageContent", "sendMessage") for call in self.td.calls))

    def test_preview_privacy_protected_spoiler(self):
        for change in ({"can_be_saved": False}, {"self_destruct_type": {"@type": "messageSelfDestructTypeImmediately"}},
                       {"content": {**self.message["content"], "has_spoiler": True}}):
            message = {**self.message, **change}
            body, data, file = telegram.describe(message)
            self.assertIsNone(file)
            self.assertEqual(data["kind"], "text")
        self.add(preview=False)
        self.assertEqual(self.cards[-1]["body"], "New message")
        self.assertFalse(any(call[0] == "downloadFile" for call in self.td.calls))

    def test_rich_media_formats_and_privacy(self):
        for td_format, format in (("stickerFormatWebp","webp"),("stickerFormatTgs","tgs"),("stickerFormatWebm","webm")):
            sticker = {"format":{"@type":td_format}, "sticker":self.file, "emoji":"🙂"}
            for content, kind in (({"@type":"messageSticker", "sticker":sticker},"sticker"),
                                  ({"@type":"messageAnimatedEmoji", "animated_emoji":{"sticker":sticker}, "emoji":"🙂"},"emoji")):
                message = {**self.message,"content":content}
                body,data,file = telegram.describe(message)
                self.assertEqual((data["kind"],data["format"],file["id"]),(kind,format,5))
                self.assertEqual(telegram.describe(message, False)[1]["kind"], "text")
                self.assertIsNone(telegram.describe({**message,"can_be_saved":False})[2])
        for mime,format in (("image/gif","gif"),("video/mp4","video")):
            content={"@type":"messageAnimation", "animation":{"mime_type":mime,"animation":self.file}}
            self.assertEqual(telegram.describe({**self.message,"content":content})[1]["format"],format)
            self.assertIsNone(telegram.describe({**self.message,"content":{**content,"has_spoiler":True}})[2])

    def test_custom_emoji_utf16_positions_and_download(self):
        formatted={"text":"Hi 🙂 there", "entities":[{"offset":3,"length":2,"type":{"@type":"textEntityTypeCustomEmoji","custom_emoji_id":"12345"}}]}
        self.add({**self.message,"content":{"@type":"messageText","text":formatted}})
        call=next(call for call in self.td.calls if call[0]=="getCustomEmojiStickers")
        self.assertEqual(call[1],{"custom_emoji_ids":[12345]})
        call[2]({"@type":"stickers","stickers":[{"full_type":{"custom_emoji_id":"12345"},"format":{"@type":"stickerFormatWebp"},"sticker":self.file}]})
        path=Path(self.temp.name)/"files/emoji.webp";path.parent.mkdir();path.write_bytes(b"synthetic")
        with patch.object(self.bridge, "prepare_media", side_effect=lambda key,path,fmt,eid:self.bridge.media_ready(key,self.bridge.entries[key],eid,path.as_uri(),"png")):
            self.bridge.handle({"@type":"updateFile","file":{**self.file,"local":{"is_downloading_completed":True,"path":str(path)}}})
        parts=self.cards[-1]["data"]["parts"]
        self.assertEqual("".join(p["text"] for p in parts),formatted["text"])
        emoji=next(p for p in parts if p["kind"]=="emoji")
        self.assertEqual(emoji["source"],path.as_uri())
        self.assertEqual(emoji["text"],"🙂")
        self.td.calls.clear()
        self.add({**self.message,"content":{"@type":"messageText","text":formatted}},preview=False)
        self.assertFalse(any(c[0]=="getCustomEmojiStickers" for c in self.td.calls))
        self.assertNotIn("parts",self.cards[-1]["data"])

    def test_partial_transcripts_preserve_download_and_card_identity(self):
        self.add(self.voice_message())
        key=next(iter(self.bridge.entries));entry=self.bridge.entries[key]
        entry["notice"]["data"]["source"]="file:///synthetic/voice.ogg"
        before=len(self.td.calls)
        for text in ("Hello", "Hello there", "Hello there again"):
            self.bridge.handle({"@type":"updateMessageContent","chat_id":123,"message_id":self.message["id"],
                "new_content":self.voice_message({"@type":"speechRecognitionResultPending","partial_text":text})["content"]})
            self.assertIs(self.bridge.entries[key],entry)
            self.assertEqual(self.cards[-1]["data"]["source"],"file:///synthetic/voice.ogg")
            self.assertEqual(self.cards[-1]["data"]["transcript"],text)
        self.assertEqual(len(self.td.calls),before)

    def test_completed_render_for_removed_card_does_not_reappear(self):
        self.add();key=next(iter(self.bridge.entries));entry=self.bridge.entries[key]
        self.bridge.remove(key);count=len(self.cards)
        self.bridge.media_ready(key,entry,None,"file:///synthetic.gif","gif")
        self.assertEqual(len(self.cards),count)

    def voice_message(self, result=None):
        return {**self.message, "content":{"@type":"messageVoiceNote", "voice_note":{
            "duration":8, "voice":self.file, "speech_recognition_result":result}}}

    def test_transcribe_checks_eligibility_and_publishes_partial_and_final_text(self):
        self.add(self.voice_message())
        answers = []
        request = {"action":"transcribe", "chatId":"123", "messageId":str(self.message["id"])}
        self.bridge.action(request, answers.append)
        self.assertEqual(self.cards[-1]["data"]["transcriptionState"], "pending")
        method, fields, callback = self.td.calls[-1]
        self.assertEqual(method, "getMessageProperties")
        self.assertEqual(fields, {"chat_id":123, "message_id":self.message["id"]})
        before = len(self.td.calls)
        self.bridge.action(request, answers.append)
        self.assertEqual(len(self.td.calls), before)
        callback({"@type":"messageProperties", "can_recognize_speech":True})
        method, fields, callback = self.td.calls[-1]
        self.assertEqual(method, "recognizeSpeech")
        callback({"@type":"ok"})
        for result, state, text in (({"@type":"speechRecognitionResultPending", "partial_text":"Hello"}, "pending", "Hello"),
                                    ({"@type":"speechRecognitionResultText", "text":"Hello world"}, "done", "Hello world")):
            self.bridge.handle({"@type":"updateMessageContent", "chat_id":123, "message_id":self.message["id"],
                                "new_content":self.voice_message(result)["content"]})
            self.assertEqual(self.cards[-1]["data"]["transcriptionState"], state)
            self.assertEqual(self.cards[-1]["data"]["transcript"], text)
        self.assertTrue(all(answer["ok"] for answer in answers))
        self.assertFalse(any(call[0] == "viewMessages" for call in self.td.calls))

    def test_transcribe_unavailable_and_server_error(self):
        self.add(self.voice_message())
        answers = []
        request = {"action":"transcribe", "chatId":"123", "messageId":str(self.message["id"])}
        self.bridge.action(request, answers.append)
        self.td.calls[-1][2]({"@type":"messageProperties", "can_recognize_speech":False})
        self.assertFalse(answers[-1]["ok"])
        self.assertFalse(any(call[0] == "recognizeSpeech" for call in self.td.calls))
        self.assertEqual(self.cards[-1]["data"]["transcriptionState"], "error")
        self.bridge.action(request, answers.append)
        self.td.calls[-1][2]({"@type":"messageProperties", "can_recognize_speech":True})
        self.td.calls[-1][2]({"@type":"error", "message":"FLOOD_WAIT_60"})
        self.assertEqual(self.cards[-1]["data"]["transcriptionError"], "FLOOD_WAIT_60")

    def test_transcribe_respects_hidden_previews_and_dismissal(self):
        self.add(self.voice_message(), preview=False)
        answers = []
        request = {"action":"transcribe", "chatId":"123", "messageId":str(self.message["id"])}
        self.bridge.action(request, answers.append)
        self.assertFalse(answers[-1]["ok"])
        self.assertFalse(any(call[0] == "getMessageProperties" for call in self.td.calls))
        self.add(self.voice_message())
        self.bridge.action(request, answers.append)
        callback = self.td.calls[-1][2]
        self.bridge.remove(next(iter(self.bridge.entries)))
        callback({"@type":"messageProperties", "can_recognize_speech":True})
        self.assertFalse(any(call[0] == "recognizeSpeech" for call in self.td.calls))
        self.assertFalse(answers[-1]["ok"])

    def test_avatar_download_updates_card_and_reuses_cached_photo(self):
        self.add()
        photo = {"id": 91, "size": 12, "local": {"is_downloading_completed": False}}
        self.bridge.chat_result(123, {"@type":"chat", "id":123, "title":"Maya", "photo":{"small":{"id":90}, "big":photo}})
        self.assertEqual(self.td.calls[-1][0], "downloadFile")
        self.assertEqual(self.td.calls[-1][1]["file_id"], 91)
        path = Path(self.temp.name) / "db/profile_photos/avatar.jpg"
        path.parent.mkdir(parents=True); path.write_bytes(b"synthetic")
        self.bridge.handle({"@type":"updateFile", "file":{**photo, "local":{"is_downloading_completed":True, "path":str(path)}}})
        self.assertEqual(self.cards[-1]["image"], path.as_uri())
        self.assertEqual(self.cards[-1]["title"], "Maya")
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.cards[-1]["data"]["kind"], "photo")
        avatar_downloads = sum(method == "downloadFile" and fields["file_id"] == 91 for method, fields, _ in self.td.calls)
        self.add()
        self.assertEqual(sum(method == "downloadFile" and fields["file_id"] == 91 for method, fields, _ in self.td.calls), avatar_downloads)
        self.assertEqual(self.cards[-1]["image"], path.as_uri())
        self.bridge.handle({"@type":"updateChatPhoto", "chat_id":123, "photo":None})
        self.assertNotIn("image", self.cards[-1])

    def test_avatar_rejects_outside_cache_and_stale_download(self):
        self.add()
        photo = {"id":91, "size":12}
        self.bridge.chat_result(123, {"@type":"chat", "id":123, "photo":{"small":photo}})
        outside = Path(self.temp.name) / "outside.jpg"; outside.write_bytes(b"synthetic")
        self.bridge.avatar_result(91, {**photo, "local":{"is_downloading_completed":True, "path":str(outside)}})
        self.assertNotIn("image", self.cards[-1])
        self.bridge.avatar(next(iter(self.bridge.entries)))
        self.bridge.handle({"@type":"updateChatPhoto", "chat_id":123, "photo":None})
        count = len(self.cards)
        self.bridge.avatar_result(91, {"@type":"error"})
        self.assertEqual(len(self.cards), count)
        self.assertFalse(self.bridge.avatar_downloads)

    def test_avatar_failure_and_dismissal_keep_initials_without_republishing(self):
        self.add()
        self.bridge.chat_result(123, {"@type":"chat", "id":123, "photo":{"small":{"id":91}}})
        count = len(self.cards)
        self.bridge.avatar_result(91, {"@type":"error", "message":"unavailable"})
        self.assertEqual(len(self.cards), count)
        self.assertNotIn("image", self.cards[-1])
        key = next(iter(self.bridge.entries))
        self.bridge.avatar(key)
        self.bridge.remove(key)
        self.assertFalse(self.bridge.avatar_downloads)

    def test_waveform_decodes_cross_byte_peaks(self):
        # Eight peaks fill exactly five bytes, including values crossing byte boundaries.
        peaks = [0, 31, 1, 16, 7, 24, 3, 30]
        packed = sum(value << (index * 5) for index, value in enumerate(peaks)).to_bytes(5, "little")
        encoded = base64.b64encode(packed).decode()
        self.assertEqual(telegram.decode_waveform(encoded), peaks)
        message = {**self.message, "content": {"@type": "messageVoiceNote",
                   "voice_note": {"duration": 23, "waveform": encoded, "voice": self.file}}}
        self.add(message)
        self.assertEqual(self.cards[-1]["data"]["waveform"], peaks)
        self.add(message, preview=False)
        self.assertNotIn("waveform", self.cards[-1]["data"])

    def test_waveform_missing_invalid_and_padding(self):
        for encoded in (None, "", "!invalid!", "a" * 4097, 123):
            self.assertEqual(telegram.decode_waveform(encoded), [])
        self.assertEqual(telegram.decode_waveform("/w=="), [31])

    def test_voice_and_size_limit(self):
        message = {**self.message, "content": {"@type": "messageVoiceNote", "voice_note": {"duration": 23, "voice": self.file}}}
        self.add(message)
        self.assertEqual(self.cards[-1]["data"]["duration"], 23)
        self.assertEqual(self.cards[-1]["data"]["kind"], "voice")
        self.td.calls.clear()
        message["content"]["voice_note"]["voice"] = {**self.file, "size": telegram.MAX_FILE + 1}
        self.add(message)
        self.assertIn("20 MiB", self.cards[-1]["data"]["status"])
        self.assertFalse(any(call[0] == "downloadFile" for call in self.td.calls))

    def test_tdlib_sticker_cache_is_valid_media(self):
        path = Path(self.temp.name) / "db/stickers/sticker.tgs"
        path.parent.mkdir(parents=True)
        path.write_bytes(b"synthetic")
        file = {**self.file, "local": {"is_downloading_completed": True, "path": str(path)}}
        self.assertEqual(self.bridge.local_media(file), path)
        self.add()
        with patch.object(self.bridge, "prepare_media") as render:
            key = next(iter(self.bridge.entries))
            self.bridge.entries[key]["notice"]["data"].update(kind="sticker", format="tgs")
            self.bridge.file_update(file)
            render.assert_called_once_with(key, path, "tgs")
        outside = Path(self.temp.name) / "db/private.txt"
        outside.write_bytes(b"private")
        path.unlink()
        path.symlink_to(outside)
        with self.assertRaises(ValueError):
            self.bridge.local_media(file)

    def test_download_path_cannot_escape_cache(self):
        self.add()
        path = Path(self.temp.name) / "private.txt"
        path.write_text("not a downloaded image")
        self.bridge.file_update({**self.file, "local": {"is_downloading_completed": True, "path": str(path)}})
        self.assertEqual(self.cards[-1]["data"]["source"], "")
        self.assertIn("unavailable", self.cards[-1]["data"]["status"])

    def test_removed_notifications_do_not_reappear_after_download(self):
        self.add()
        count = len(self.cards)
        self.bridge.handle({"@type": "updateNotificationGroup", "notification_group_id": 8, "chat_id": 123, "removed_notification_ids": [9]})
        self.bridge.file_update({**self.file, "local": {"is_downloading_completed": True, "path": "/nonexistent"}})
        self.assertEqual(len(self.cards), count)
        self.assertEqual(len(self.closed), 1)
        self.assertFalse(self.bridge.entries)

    def test_startup_and_outgoing_messages_are_not_replayed(self):
        self.bridge.handle({"@type": "updateActiveNotifications", "groups": [{"notifications": [self.message]}]})
        self.add({**self.message, "is_outgoing": True})
        self.assertFalse(self.cards)
        self.bridge.started = 2000
        self.add()
        self.assertFalse(self.cards)

    def test_sender_names_never_determine_message_identity(self):
        self.add()
        self.bridge.chat_result(123, {"@type": "chat", "id": 123, "title": "Same name; $(touch /tmp/nope)"})
        replies = []
        self.bridge.action({"action": "read", "chatId": "123", "messageId": str(42 << 20)}, replies.append)
        method, payload, callback = self.td.calls[-1]
        self.assertEqual(method, "viewMessages")
        self.assertEqual(payload["message_ids"], [42 << 20])
        callback({"@type": "ok"})
        self.assertEqual(replies, [{"ok": True}])

    def test_reply_is_explicit_literal_and_does_not_clear_desktop_draft(self):
        self.add()
        replies = []
        text = "literal `text` $(no shell)\nsecond line"
        self.bridge.action({"action": "reply", "chatId": "123", "messageId": str(42 << 20), "text": text}, replies.append)
        method, payload, callback = self.td.calls[-1]
        self.assertEqual(method, "sendMessage")
        self.assertEqual(payload["input_message_content"]["text"]["text"], text)
        self.assertFalse(payload["input_message_content"]["clear_draft"])
        self.assertEqual(payload["reply_to"]["message_id"], 42 << 20)
        callback({"@type": "error", "message": "CHAT_WRITE_FORBIDDEN"})
        self.assertFalse(replies[-1]["ok"])
        before = len(self.td.calls)
        self.bridge.action({"action": "reply", "chatId": "123", "messageId": str(42 << 20), "text": " "}, replies.append)
        self.assertEqual(len(self.td.calls), before)
        self.assertFalse(replies[-1]["ok"])

    def test_quote_reply_targets_message_and_plain_reply_does_not(self):
        self.add()
        for quote in (True, False):
            replies = []
            self.bridge.action({"action":"reply", "chatId":"123", "messageId":str(42 << 20),
                                "text":"A reply", "quote":quote}, replies.append)
            method, payload, callback = self.td.calls[-1]
            self.assertEqual(method, "sendMessage")
            self.assertEqual(payload["chat_id"], 123)
            self.assertEqual(payload["reply_to"], {"@type":"inputMessageReplyToMessage", "message_id":42 << 20} if quote else None)
            self.assertFalse(payload["input_message_content"]["clear_draft"])
            callback({"@type":"ok"})
            self.assertEqual(replies, [{"ok":True}])

    def voice_copy(self):
        path = Path(self.temp.name) / "voice-outbox/voice-test.ogg"
        path.parent.mkdir(exist_ok=True)
        path.write_bytes(b"synthetic upload copy")
        return path

    def test_demo_voice_routes_only_to_verified_self_without_reply(self):
        responses = []
        request = {"action":"demoVoiceReply", "chatId":"999", "messageId":"777", "token":"a"*32}
        self.bridge.action(request, responses.append)
        self.assertEqual(self.td.calls[-1][0], "getMe")
        self.td.calls[-1][2]({"@type":"user", "id":456, "usernames":{"active_usernames":["TijpTjik"]}})
        self.assertEqual(self.td.calls[-1][0], "createPrivateChat")
        self.assertEqual(self.td.calls[-1][1], {"user_id":456, "force":True})
        path = self.voice_copy()
        with patch("telegram_recording.copy_for_upload", return_value=(path,1.2)):
            self.td.calls[-1][2]({"@type":"chat", "id":456, "type":{"@type":"chatTypePrivate", "user_id":456}})
        method, fields, callback = self.td.calls[-1]
        self.assertEqual(method, "sendMessage")
        self.assertEqual(fields["chat_id"], 456)
        self.assertIsNone(fields["reply_to"])
        callback({"@type":"message", "id":99, "content":{"voice_note":{"voice":{"id":55, "remote":{"is_uploading_completed":False}}}}})
        self.assertTrue(responses[-1]["ok"])
        self.assertTrue(path.exists())
        self.bridge.voice_file_update({"id":55, "remote":{"is_uploading_completed":True}})
        self.assertFalse(path.exists())

    def test_demo_voice_rejects_wrong_account_and_destination(self):
        responses = []
        with patch("telegram_recording.copy_for_upload") as copy:
            self.bridge.action({"action":"demoVoiceReply", "token":"a"*32}, responses.append)
            self.td.calls[-1][2]({"@type":"user", "id":456, "usernames":{"active_usernames":["someone_else"]}})
            self.assertFalse(responses[-1]["ok"])
            self.assertFalse(any(call[0] == "createPrivateChat" for call in self.td.calls))
            self.bridge.action({"action":"demoVoiceReply", "token":"a"*32}, responses.append)
            self.td.calls[-1][2]({"@type":"user", "id":456, "usernames":{"active_usernames":["tijptjik"]}})
            self.td.calls[-1][2]({"@type":"chat", "id":999, "type":{"@type":"chatTypePrivate", "user_id":999}})
            self.assertFalse(responses[-1]["ok"])
            self.assertFalse(any(call[0] == "sendMessage" for call in self.td.calls))
            copy.assert_not_called()

    def test_voice_reply_uses_original_message_and_retains_upload_until_complete(self):
        self.add()
        path = self.voice_copy()
        responses = []
        with patch("telegram_recording.copy_for_upload", return_value=(path, 1.2)) as copy:
            self.bridge.action({"action":"voiceReply", "chatId":"123", "messageId":str(42 << 20), "token":"a"*32}, responses.append)
        copy.assert_called_once_with("a"*32, Path(self.temp.name)/"voice-outbox")
        method, fields, callback = self.td.calls[-1]
        self.assertEqual(method, "sendMessage")
        self.assertEqual(fields["reply_to"]["message_id"], 42 << 20)
        self.assertEqual(fields["chat_id"], 123)
        self.assertEqual(fields["input_message_content"], {"@type":"inputMessageVoiceNote", "voice_note":{
            "@type":"inputVoiceNote", "voice_note":{"@type":"inputFileLocal", "path":str(path)}, "duration":2, "waveform":""}})
        callback({"@type":"message", "id":99, "content":{"voice_note":{"voice":{"id":55,"remote":{"is_uploading_completed":False}}}}})
        self.assertEqual(responses,[{"ok":True}])
        self.assertTrue(path.exists())
        self.assertTrue(path.with_suffix(".json").exists())
        self.bridge.handle({"@type":"updateFile", "file":{"id":55,"remote":{"is_uploading_completed":True}}})
        self.assertFalse(path.exists())
        self.assertFalse(path.with_suffix(".json").exists())
        self.bridge.handle({"@type":"updateMessageSendSucceeded", "old_message_id":99, "message":{"id":100,"chat_id":123}})
        self.assertEqual(self.cards[-1]["data"]["voiceReplyStatus"],"Voice reply sent")

    def test_voice_rejection_preserves_draft_and_removes_upload_copy(self):
        self.add()
        path = self.voice_copy()
        responses = []
        with patch("telegram_recording.copy_for_upload", return_value=(path,1)):
            self.bridge.action({"action":"voiceReply", "chatId":"123", "messageId":str(42 << 20), "token":"a"*32}, responses.append)
        self.td.calls[-1][2]({"@type":"error", "message":"CHAT_WRITE_FORBIDDEN"})
        self.assertFalse(responses[-1]["ok"])
        self.assertFalse(path.exists())
        self.assertIn("CHAT_WRITE_FORBIDDEN",responses[-1]["error"])

    def test_voice_upload_recovers_after_bridge_restart(self):
        path = self.voice_copy()
        telegram.private_write(path.with_suffix(".json"), {"file_id":55,"chat":123,"message":99,"origin":"123/456"})
        self.bridge.recover_voice_uploads()
        method, fields, callback = self.td.calls[-1]
        self.assertEqual((method,fields),("getFile",{"file_id":55}))
        callback({"@type":"file","id":55,"remote":{"is_uploading_completed":True}})
        self.assertFalse(path.exists())

    def test_voice_timeout_keeps_copy_and_warns_against_duplicate_retry(self):
        self.add()
        path = self.voice_copy()
        responses=[]
        with patch("telegram_recording.copy_for_upload", return_value=(path,1)):
            self.bridge.action({"action":"voiceReply", "chatId":"123", "messageId":str(42 << 20), "token":"a"*32}, responses.append)
        self.td.calls[-1][2]({"@type":"error", "message":"Telegram request timed out"})
        self.assertTrue(path.exists())
        self.assertTrue(responses[-1]["uncertain"])

    def test_unknown_or_removed_message_actions_are_rejected(self):
        replies = []
        self.bridge.action({"action": "read", "chatId": "123", "messageId": "55"}, replies.append)
        self.assertFalse(replies[-1]["ok"])
        self.assertFalse(self.td.calls)

    def test_session_lock_excludes_second_writer(self):
        with telegram.session_lock():
            with self.assertRaisesRegex(ValueError, "already in use"):
                telegram.session_lock()

    def test_failed_ui_delivery_is_retained_for_retry(self):
        def fail(_):
            raise ValueError("not running")
        self.bridge.publish = fail
        self.add()
        self.assertFalse(self.bridge.ui_ready)
        self.assertFalse(next(iter(self.bridge.entries.values()))["published"])

    def test_bridge_notice_id_matches_quickshell_encoding(self):
        self.assertEqual(telegram.notice_id("123/456"), "named-Telegram%20via%20Notjik/123%2F456")

    def test_private_chat_open_uses_supported_username_link(self):
        self.add()
        self.bridge.chats[123] = {"type": {"@type":"chatTypePrivate", "user_id":123}}
        responses=[]
        self.bridge.action({"action":"open", "chatId":"123", "messageId":str(42 << 20)}, responses.append)
        self.td.calls[-1][2]({"@type":"error", "message":"link unavailable"})
        method, fields, callback = self.td.calls[-1]
        self.assertEqual((method,fields), ("getUser", {"user_id":123}))
        with patch.object(telegram.subprocess, "Popen") as open_process:
            callback({"@type":"user", "usernames":{"active_usernames":["sample_user"]}})
        self.assertEqual(open_process.call_args.args[0], ["xdg-open", "tg://resolve?domain=sample_user"])
        self.assertTrue(responses[-1]["ok"])

    def test_authentication_state_is_not_repeated_by_response_metadata(self):
        with patch.object(telegram, "credentials", return_value={"api_id":1,"api_hash":"a"*32,"key":""}):
            auth=telegram.Authorization(self.td)
        auth.handle({"@type":"authorizationStateWaitTdlibParameters", "@extra":"1"})
        auth.handle({"@type":"authorizationStateWaitTdlibParameters"})
        self.assertEqual([call[0] for call in self.td.calls], ["setTdlibParameters"])
        self.assertFalse(self.td.calls[0][1]["use_secret_chats"])

    def test_private_chat_without_link_has_explicit_fallback(self):
        responses=[]
        with patch.object(telegram.subprocess, "Popen") as open_process:
            self.bridge.open_user({"@type":"user"},responses.append)
        self.assertEqual(open_process.call_args.args[0],["xdg-open","tg://"])
        self.assertIn("no direct desktop link",responses[-1]["message"])

    def test_lost_voice_confirmation_blocks_retry_but_connection_failure_does_not(self):
        with patch.object(telegram.socket, "socket") as socket:
            client = socket.return_value.__enter__.return_value
            for response in (b"", b"not json", b"[]"):
                client.makefile.return_value = io.BytesIO(response)
                result = telegram.socket_request({"action":"voiceReply", "token":"a"*32})
                self.assertFalse(result["ok"])
                self.assertTrue(result["uncertain"])
            client.makefile.side_effect = TimeoutError
            self.assertTrue(telegram.socket_request({"action":"voiceReply"})["uncertain"])
            client.connect.side_effect = ConnectionRefusedError
            with self.assertRaises(ConnectionRefusedError):
                telegram.socket_request({"action":"voiceReply"})

    def test_handler_timeout_after_queuing_voice_is_uncertain(self):
        from unittest.mock import Mock
        handler = object.__new__(telegram.ControlHandler)
        handler.connection = Mock()
        handler.server = Mock()
        handler.rfile = io.BytesIO(b'{"action":"voiceReply"}\n')
        handler.wfile = io.BytesIO()
        with patch.object(telegram.queue, "Queue") as queue:
            queue.return_value.get.side_effect = telegram.queue.Empty
            handler.handle()
        self.assertTrue(json.loads(handler.wfile.getvalue())["uncertain"])
        handler.server.requests.put_nowait.assert_called_once()

    def test_malformed_control_request_rejected_before_connecting(self):
        with self.assertRaisesRegex(ValueError, "must be an object"):
            telegram.socket_request(["not an object"])

    def test_cancelled_login_has_nonzero_exit_status(self):
        with patch.object(telegram, "run", side_effect=KeyboardInterrupt), patch.object(telegram.sys, "stderr"):
            self.assertEqual(telegram.main(ROOT, ["login"]), 130)


if __name__ == "__main__":
    unittest.main()
