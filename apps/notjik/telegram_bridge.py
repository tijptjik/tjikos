"""Opt-in TDLib notification client. Account data never belongs in the checkout.

The UI calls a private Unix socket; only this process owns the TDLib session.
Notification updates (not a message-history poll) preserve Telegram mute rules.
"""
import argparse
import concurrent.futures
import hashlib
import html
import re
import base64
import binascii
from collections import OrderedDict
import fcntl
import getpass
import importlib
import importlib.util
import json
import math
import shutil
import os
from pathlib import Path
import queue
import secrets
import signal
import socket
import socketserver
import subprocess
import sys
import threading
import time

APP = "Telegram via Notjik"
MAX_FILE = 20 * 1024 * 1024
MAX_ACTIVE = 100


def state_dir():
    return Path(os.environ.get("NOTJIK_TELEGRAM_DIR", str(Path(os.environ.get(
        "XDG_STATE_HOME", str(Path.home() / ".local/state"))) / "notjik/telegram")))


def socket_path():
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not runtime:
        raise ValueError("XDG_RUNTIME_DIR is required for Telegram controls")
    return Path(os.environ.get("NOTJIK_TELEGRAM_SOCKET", str(Path(runtime) / "notjik-telegram/control.sock")))


def private_dir(path):
    path.mkdir(parents=True, mode=0o700, exist_ok=True)
    os.chmod(path, 0o700)
    return path


def private_write(path, value):
    temp = path.with_name(path.name + ".tmp")
    with os.fdopen(os.open(temp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "w") as stream:
        json.dump(value, stream)
    temp.replace(path)
    os.chmod(path, 0o600)


def identifier(value):
    # TDLib int53 values are safe in JS, but strings keep IPC identities explicit.
    if isinstance(value, bool) or not str(value).lstrip("-").isdigit():
        raise ValueError("Invalid Telegram message identity")
    result = int(value)
    if not result or abs(result) > 2 ** 53 - 1:
        raise ValueError("Invalid Telegram message identity")
    return result


def message_key(chat, message):
    chat = identifier(chat)
    # Named notification IDs must start with a letter or digit. TDLib uses
    # negative chat IDs for groups/channels; preserve positive private-chat IDs.
    return f"{'chat' if chat < 0 else ''}{chat}/{identifier(message)}"


def notice_id(key):
    from urllib.parse import quote
    return "named-" + quote(APP, safe="~()*!.'-") + "/" + quote(key, safe="~()*!.'-")


def decode_waveform(encoded):
    """TDLib voice waveforms are base64 bytes containing little-endian 5-bit peaks."""
    if not isinstance(encoded, str) or not encoded or len(encoded) > 4096:
        return []
    try:
        packed = base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error):
        return []
    bits = int.from_bytes(packed, "little")
    return [(bits >> offset) & 31 for offset in range(0, len(packed) * 8 - 4, 5)]


def transcription_data(result):
    result = result or {}
    kind = result.get("@type")
    if kind == "speechRecognitionResultPending":
        return {"transcriptionState": "pending", "transcript": result.get("partial_text", "")[:65536]}
    if kind == "speechRecognitionResultText":
        return {"transcriptionState": "done", "transcript": result.get("text", "")[:65536]}
    if kind == "speechRecognitionResultError":
        return {"transcriptionState": "error", "transcriptionError": result.get("error", {}).get("message", "Transcription failed")[:4096]}
    return {"transcriptionState": "idle", "transcript": ""}


def sticker_data(sticker, kind="sticker"):
    format = {"stickerFormatWebp":"webp", "stickerFormatTgs":"tgs", "stickerFormatWebm":"webm"}.get((sticker.get("format") or {}).get("@type"), "")
    return {"kind":kind, "source":"", "format":format, "status":"Downloading sticker…", "fallback":sticker.get("emoji", ""),
            "width":sticker.get("width", 256), "height":sticker.get("height", 256)}, sticker.get("sticker")


def styled_text_renderer(formatted):
    """Render TDLib UTF-16 entity ranges as a small, escaped Qt StyledText subset.

    Split at every boundary so nested and overlapping entities always produce
    balanced tags. Literal HTML and Markdown in messages remain literal.
    """
    text = formatted.get("text", "")[:65536]
    positions, units = {0: 0}, 0
    for index, char in enumerate(text):
        units += 2 if ord(char) > 0xffff else 1
        positions[units] = index + 1
    tags = {
        "textEntityTypeBold": ("<b>", "</b>"),
        "textEntityTypeItalic": ("<i>", "</i>"),
        "textEntityTypeUnderline": ("<u>", "</u>"),
        "textEntityTypeStrikethrough": ("<s>", "</s>"),
        "textEntityTypeCode": ('<font face="monospace">', "</font>"),
        "textEntityTypePre": ('<font face="monospace">', "</font>"),
        "textEntityTypePreCode": ('<font face="monospace">', "</font>"),
        # Links retain their visual distinction; Open goes to the original
        # Telegram message, where Telegram handles the destination.
        "textEntityTypeTextUrl": ("<u>", "</u>"),
        "textEntityTypeUrl": ("<u>", "</u>"),
    }
    ranges = []
    for entity in formatted.get("entities", []):
        tag = tags.get(entity.get("type", {}).get("@type"))
        offset, length = entity.get("offset"), entity.get("length")
        if (tag is None or type(offset) is not int or type(length) is not int
                or length <= 0 or offset not in positions or offset + length not in positions):
            continue
        ranges.append((offset, offset + length, tag))

    def render(start=0, stop=None):
        stop = units if stop is None else min(stop, units)
        if start not in positions or stop not in positions or start > stop:
            return ""
        boundaries = {start, stop}
        for left, right, _ in ranges:
            if left < stop and right > start:
                boundaries.update((max(start, left), min(stop, right)))
        result = []
        ordered = sorted(boundaries)
        for left, right in zip(ordered, ordered[1:]):
            active = list(dict.fromkeys(tag for a, b, tag in ranges if a <= left and right <= b))
            value = html.escape(text[positions[left]:positions[right]], quote=False)
            # Keep line breaks/indentation while retaining ordinary word wrapping.
            value = value.replace("\t", "    ").replace("\n", "<br>")
            value = re.sub(r" {2,}", lambda match: "&nbsp;" * (len(match[0]) - 1) + " ", value)
            result.append("".join(tag[0] for tag in active) + value + "".join(tag[1] for tag in reversed(active)))
        return "".join(result)
    return render


def styled_text(formatted):
    return styled_text_renderer(formatted)()


def emoji_parts(formatted):
    """TDLib entity offsets are UTF-16 code units, including surrogate pairs."""
    text = formatted.get("text", "")[:4096]
    encoded = text.encode("utf-16-le")
    entities = sorted((e for e in formatted.get("entities", []) if e.get("type", {}).get("@type") == "textEntityTypeCustomEmoji"), key=lambda e:e.get("offset", 0))[:24]
    if not entities:
        return []
    parts, end = [], 0
    render = styled_text_renderer(formatted)
    def plain(value, offset):
        for word in re.findall(r"\n|[^\S\n]+|[^\s]+", value):
            stop = offset + len(word.encode("utf-16-le")) // 2
            part = {"kind":"text", "text":word}
            if not word.isspace():
                part["styledText"] = render(offset, stop)
            parts.append(part)
            offset = stop
    for entity in entities:
        start, stop = entity.get("offset", -1)*2, (entity.get("offset", -1)+entity.get("length", 0))*2
        if start < end or stop <= start or stop > len(encoded):
            continue
        try:
            before = encoded[end:start].decode("utf-16-le")
            fallback = encoded[start:stop].decode("utf-16-le")
        except UnicodeDecodeError:
            continue
        plain(before, end // 2)
        parts.append({"kind":"emoji", "text":fallback, "styledText":render(start // 2, stop // 2), "emojiId":str(entity["type"]["custom_emoji_id"]), "source":"", "format":""})
        end = stop
    plain(encoded[end:].decode("utf-16-le"), end // 2)
    return parts


def describe(message, preview=True):
    """Return only display-safe metadata and the one optional download target."""
    content = message.get("content", {})
    kind = content.get("@type", "")
    data = {"kind": "text", "source": "", "status": ""}
    if not preview:
        return "New message", data, None
    if (message.get("self_destruct_type") or message.get("self_destruct_in", 0)
            or content.get("is_secret") or message.get("can_be_saved") is False):
        return "Protected message · open in Telegram", data, None
    caption = content.get("caption", {}).get("text", "")
    formatted = content.get("text") if kind == "messageText" else content.get("caption")
    if isinstance(formatted, dict):
        if any(entity.get("type", {}).get("@type") == "textEntityTypeSpoiler"
               for entity in formatted.get("entities", [])):
            return "Message with spoiler · open in Telegram", data, None
        if formatted.get("entities"):
            data["styledText"] = styled_text(formatted)
        data["parts"] = emoji_parts(formatted)
    if kind == "messageSticker":
        media, file = sticker_data(content.get("sticker", {}))
        data.update(media)
        return "", data, file
    if kind == "messageAnimatedEmoji":
        media, file = sticker_data((content.get("animated_emoji") or {}).get("sticker") or {}, "emoji")
        data.update(media, fallback=content.get("emoji", ""))
        return "", data, file
    if kind == "messageAnimation":
        if content.get("has_spoiler"):
            return "Animation with spoiler · open in Telegram", {"kind":"text", "source":"", "status":""}, None
        animation = content.get("animation", {})
        data.update(kind="animation", format="gif" if animation.get("mime_type") == "image/gif" else "video",
                    width=animation.get("width", 320), height=animation.get("height", 200), status="Downloading animation…")
        return caption, data, animation.get("animation")
    if kind == "messagePhoto":
        if content.get("has_spoiler"):
            return "Photo with spoiler · open in Telegram", {"kind":"text", "source":"", "status":""}, None
        sizes = content.get("photo", {}).get("sizes", [])
        sizes = [size for size in sizes if size.get("photo", {}).get("id")]
        # A preview is sufficient; avoid downloading the original full-size photo.
        preferred = [size for size in sizes if max(size.get("width", 0), size.get("height", 0)) <= 1280]
        size = max(preferred or sizes, key=lambda s: s.get("width", 0) * s.get("height", 0), default={})
        data.update(kind="photo", status="Downloading photo…")
        return caption, data, size.get("photo")
    if kind == "messageVoiceNote":
        voice = content.get("voice_note", {})
        data.update(kind="voice", duration=voice.get("duration", 0),
                    waveform=decode_waveform(voice.get("waveform", "")), status="Downloading voice message…")
        data.update(transcription_data(voice.get("speech_recognition_result")))
        return caption, data, voice.get("voice")
    if kind == "messageText":
        return content.get("text", {}).get("text", ""), data, None
    labels = {"messageVideo": "Video", "messageVideoNote": "Video message", "messageDocument": "Document",
              "messageAudio": "Audio", "messageAnimation": "Animation", "messageSticker": "Sticker",
              "messageLocation": "Location", "messagePoll": "Poll", "messageContact": "Contact"}
    return caption or labels.get(kind, "New message · open in Telegram"), data, None


class TDLib:
    def __init__(self):
        try:
            self.api = importlib.import_module("tdjson")
        except ImportError as error:
            raise ValueError("TDLib runtime missing. Run scripts/install-telegram.sh from the Notjik source.") from error
        self.api.td_execute(json.dumps({"@type": "setLogVerbosityLevel", "new_verbosity_level": 0}).encode())
        self.client = self.api.td_create_client_id()
        self.sequence = 0
        self.pending = {}

    def send(self, method, callback=None, **fields):
        self.sequence += 1
        extra = str(self.sequence)
        if callback:
            self.pending[extra] = (time.monotonic() + 30, callback)
        self.api.td_send(self.client, json.dumps({"@type": method, **fields, "@extra": extra}).encode())

    def receive(self):
        raw = self.api.td_receive(0.1)
        now = time.monotonic()
        for extra, (deadline, callback) in list(self.pending.items()):
            if deadline < now:
                del self.pending[extra]
                callback({"@type": "error", "message": "Telegram request timed out"})
        if not raw:
            return None
        event = json.loads(raw)
        pending = self.pending.pop(str(event.get("@extra", "")), None)
        if pending:
            pending[1](event)
            return None
        return event


def credentials(interactive=False):
    path = private_dir(state_dir()) / "credentials.json"
    if path.exists():
        result = json.loads(path.read_text())
        os.chmod(path, 0o600)
    elif interactive:
        print("Create an API application at https://my.telegram.org/apps. Credentials stay on this computer.")
        result = {"api_id": int(input("API ID: ").strip()), "api_hash": getpass.getpass("API hash: ").strip()}
        if result["api_id"] <= 0 or len(result["api_hash"]) != 32 or any(c not in "0123456789abcdefABCDEF" for c in result["api_hash"]):
            raise ValueError("Invalid API ID or hash")
        result["key"] = base64.b64encode(secrets.token_bytes(32)).decode()
        private_write(path, result)
    else:
        raise ValueError("Telegram is not configured. Run notjikctl telegram login in a terminal.")
    return result


class Authorization:
    def __init__(self, td, interactive=False):
        self.td = td
        self.interactive = interactive
        self.creds = credentials(interactive)
        self.ready = False
        self.closed = False
        self.error = ""
        self.state = None

    def checked(self, result):
        if result.get("@type") == "error":
            # Never print the request, phone, password, code, or credential values.
            self.error = "Telegram rejected authentication: " + result.get("message", "unknown error")

    def handle(self, state):
        state = {key: value for key, value in state.items() if not key.startswith("@") or key == "@type"}
        if self.state == state:
            return
        self.state = state
        kind = state.get("@type")
        if kind == "authorizationStateWaitTdlibParameters":
            self.td.send("setTdlibParameters", self.checked,
                         database_directory=str(private_dir(state_dir() / "db")),
                         files_directory=str(private_dir(state_dir() / "files")),
                         database_encryption_key=self.creds.get("key", ""),
                         use_file_database=True, use_chat_info_database=True, use_message_database=True,
                         use_secret_chats=False, api_id=int(self.creds["api_id"]), api_hash=self.creds["api_hash"],
                         system_language_code="en", device_model="Notjik", system_version="Linux", application_version="0.1")
        elif kind == "authorizationStateReady":
            self.ready = True
        elif kind == "authorizationStateClosed":
            self.closed = True
        elif kind in ("authorizationStateClosing", "authorizationStateLoggingOut"):
            self.ready = False
        elif not self.interactive:
            raise ValueError("Telegram sign-in is required. Stop the bridge and run notjikctl telegram login.")
        elif kind == "authorizationStateWaitPhoneNumber":
            self.td.send("setAuthenticationPhoneNumber", self.checked, phone_number=input("Phone number (with country code): ").strip())
        elif kind == "authorizationStateWaitCode":
            self.td.send("checkAuthenticationCode", self.checked, code=getpass.getpass("Telegram login code: ").strip())
        elif kind == "authorizationStateWaitPassword":
            self.td.send("checkAuthenticationPassword", self.checked, password=getpass.getpass("Two-step verification password: "))
        elif kind == "authorizationStateWaitEmailAddress":
            self.td.send("setAuthenticationEmailAddress", self.checked, email_address=input("Login email address: ").strip())
        elif kind == "authorizationStateWaitEmailCode":
            self.td.send("checkAuthenticationEmailCode", self.checked,
                         code={"@type": "emailAddressAuthenticationCode", "code": getpass.getpass("Email verification code: ").strip()})
        elif kind == "authorizationStateWaitOtherDeviceConfirmation":
            print("Confirm this sign-in from Telegram on another device:", state["link"])
        else:
            raise ValueError("This account needs an authentication step in the official Telegram app: " + str(kind))


class Bridge:
    def __init__(self, td, publish, dismiss, clock=time.time):
        self.td, self.publish, self.dismiss, self.clock = td, publish, dismiss, clock
        self.started = clock()
        self.delivery = {"notificationEvents": 0, "lastNotificationAt": 0,
                         "ignoredType": 0, "ignoredOutgoing": 0, "ignoredOld": 0,
                         "publishAttempts": 0, "publishFailures": 0}
        self.entries = OrderedDict()
        self.notifications = {}
        self.chats = {}
        self.downloads = {}
        self.avatar_downloads = {}
        self.emoji_downloads = {}
        self.media_pool = concurrent.futures.ThreadPoolExecutor(max_workers=2)
        self.media_jobs = {}
        self.render_root = private_dir(state_dir() / "rendered")
        self.files_root = (state_dir() / "files").resolve()
        self.stickers_root = (state_dir() / "db" / "stickers").resolve()
        # TDLib writes downloaded chat photos into its own private database
        # cache, rather than the ordinary attachment directory.
        self.profile_photos_root = (state_dir() / "db" / "profile_photos").resolve()
        self.ui_ready = True
        self.voice_uploads = {}
        self.voice_sends = {}
        from telegram_clipboard import SavedClipboard
        self.clipboard = SavedClipboard(td, publish, state_dir() / "clipboard-outbox")

    def show(self, key):
        entry = self.entries.get(key)
        if not entry:
            return
        self.delivery["publishAttempts"] += 1
        try:
            self.publish(entry["notice"])
            entry["published"] = True
            self.ui_ready = True
        except (OSError, ValueError, subprocess.SubprocessError):
            entry["published"] = False
            self.delivery["publishFailures"] += 1
            self.ui_ready = False

    def remove(self, key):
        entry = self.entries.pop(key, None)
        if entry:
            for file_id, keys in list(self.downloads.items()):
                keys.discard(key)
                if not keys:
                    self.downloads.pop(file_id, None)
                    self.td.send("cancelDownloadFile", file_id=file_id, only_if_pending=False)
            for file_id, targets in list(self.emoji_downloads.items()):
                self.emoji_downloads[file_id] = {target for target in targets if target[0] != key}
                if not self.emoji_downloads[file_id]: self.emoji_downloads.pop(file_id, None)
            for file_id, keys in list(self.avatar_downloads.items()):
                keys.discard(key)
                if not keys:
                    self.avatar_downloads.pop(file_id, None)
            for ref, value in list(self.notifications.items()):
                if value == key:
                    self.notifications.pop(ref, None)
            try:
                self.dismiss(notice_id(key))
            except (OSError, ValueError, subprocess.SubprocessError):
                pass

    def chat_result(self, chat_id, result):
        if result.get("@type") != "chat":
            return
        self.chats[chat_id] = result
        for key, entry in list(self.entries.items()):
            if entry["message"]["chat_id"] == chat_id:
                entry["notice"]["title"] = result.get("title", "Telegram")[:4096]
                self.avatar(key)
                self.show(key)

    def avatar(self, key):
        entry = self.entries.get(key)
        if not entry:
            return
        chat = self.chats.get(entry["message"]["chat_id"], {})
        photo = chat.get("photo") or {}
        file = photo.get("big") or photo.get("small") or {}
        file_id = file.get("id")
        entry["avatar_file"] = file_id
        entry["notice"].pop("image", None)
        for old_id, keys in list(self.avatar_downloads.items()):
            keys.discard(key)
            if not keys:
                self.avatar_downloads.pop(old_id, None)
        if not file_id or max(file.get("size", 0), file.get("expected_size", 0)) > MAX_FILE:
            return
        pending = file_id in self.avatar_downloads
        self.avatar_downloads.setdefault(file_id, set()).add(key)
        if file.get("local", {}).get("is_downloading_completed"):
            self.avatar_result(file_id, file)
        elif not pending:
            self.td.send("downloadFile", lambda result: self.avatar_result(file_id, result),
                         file_id=file_id, priority=8, offset=0, limit=MAX_FILE + 1, synchronous=False)

    def avatar_result(self, file_id, file):
        keys = self.avatar_downloads.get(file_id)
        if not keys:
            return
        if file.get("@type") == "error":
            self.avatar_downloads.pop(file_id, None)
            return
        local = file.get("local", {})
        if max(file.get("size", 0), local.get("downloaded_size", 0)) > MAX_FILE:
            self.avatar_downloads.pop(file_id, None)
            self.td.send("cancelDownloadFile", file_id=file_id, only_if_pending=False)
            return
        if not local.get("is_downloading_completed"):
            return
        self.avatar_downloads.pop(file_id, None)
        try:
            path = Path(local["path"]).resolve(strict=True)
            if (not (path.is_relative_to(self.files_root) or path.is_relative_to(self.profile_photos_root))
                    or not path.is_file() or path.stat().st_size > MAX_FILE):
                return
            os.chmod(path, 0o600)
        except (OSError, KeyError, ValueError):
            return
        for key in keys:
            entry = self.entries.get(key)
            if entry and entry.get("avatar_file") == file_id:
                entry["notice"]["image"] = path.as_uri()
                chat = self.chats.get(entry["message"]["chat_id"], {})
                for size in ("big", "small"):
                    photo = (chat.get("photo") or {}).get(size) or {}
                    if photo.get("id") == file_id:
                        photo.update(file)
                self.show(key)

    def add(self, group_id, chat_id, notification):
        self.delivery["notificationEvents"] += 1
        self.delivery["lastNotificationAt"] = self.clock()
        item = notification.get("type", {})
        if item.get("@type") != "notificationTypeNewMessage":
            self.delivery["ignoredType"] += 1
            return  # No push registration or call handling: Desktop remains responsible for calls.
        message = item.get("message", {})
        if message.get("is_outgoing"):
            self.delivery["ignoredOutgoing"] += 1
            return
        if notification.get("date", message.get("date", 0)) < self.started - 30:
            self.delivery["ignoredOld"] += 1
            return  # Do not replay old unread messages when starting or reconnecting.
        chat_id = identifier(chat_id)
        message = {**message, "chat_id": chat_id}
        key = message_key(chat_id, message["id"])
        self.notifications[(group_id, notification["id"])] = key
        body, data, file = describe(message, item.get("show_preview", False))
        data.update(chatId=str(chat_id), messageId=str(message["id"]))
        notice = {"id": key, "app": APP, "title": self.chats.get(chat_id, {}).get("title", "Telegram")[:4096],
                  "body": body[:65536], "type": "telegram", "group": str(chat_id), "icon": "org.telegram.desktop",
                  "timeout": 0 if data["kind"] != "text" or data.get("parts") else 12000, "resident": True, "data": data}
        self.entries[key] = {"message": message, "notice": notice, "preview": item.get("show_preview", False),
                             "published": False, "file": file.get("id") if file else None}
        while len(self.entries) > MAX_ACTIVE:
            self.remove(next(iter(self.entries)))
        self.avatar(key)
        self.show(key)
        self.load_emojis(key)
        if chat_id not in self.chats:
            self.td.send("getChat", lambda result: self.chat_result(chat_id, result), chat_id=chat_id)
        if file:
            if max(file.get("size", 0), file.get("expected_size", 0)) > MAX_FILE:
                data["status"] = "Attachment exceeds 20 MiB · open in Telegram"
                self.show(key)
                return
            self.downloads.setdefault(file["id"], set()).add(key)
            if file.get("local", {}).get("is_downloading_completed"):
                self.file_update(file)
            else:
                self.td.send("downloadFile", lambda result: self.download_result(file["id"], result),
                             file_id=file["id"], priority=16, offset=0, limit=MAX_FILE + 1, synchronous=False)
        elif data["kind"] != "text":
            data["status"] = "Attachment unavailable · open in Telegram"
            self.show(key)

    def load_emojis(self, key):
        entry = self.entries.get(key)
        if not entry: return
        ids = list(dict.fromkeys(p["emojiId"] for p in entry["notice"]["data"].get("parts", []) if p["kind"] == "emoji"))
        if not ids: return
        def received(result):
            if self.entries.get(key) is not entry: return
            for sticker in result.get("stickers", []):
                emoji_id = str((sticker.get("full_type") or {}).get("custom_emoji_id", ""))
                if emoji_id not in ids: continue
                media, file = sticker_data(sticker)
                if not file or max(file.get("size", 0), file.get("expected_size", 0)) > MAX_FILE: continue
                for part in entry["notice"]["data"]["parts"]:
                    if part.get("emojiId") == emoji_id:
                        part.update(format=media["format"], fileId=file["id"])
                pending = file["id"] in self.emoji_downloads
                self.emoji_downloads.setdefault(file["id"], set()).add((key, emoji_id))
                if file.get("local", {}).get("is_downloading_completed"):
                    self.emoji_file_update(file)
                elif not pending:
                    self.td.send("downloadFile", lambda result, fid=file["id"]: self.emoji_file_update({**result, "id":fid}),
                                 file_id=file["id"], priority=8, offset=0, limit=MAX_FILE+1, synchronous=False)
        self.td.send("getCustomEmojiStickers", received, custom_emoji_ids=[int(i) for i in ids])

    def local_media(self, file):
        path = Path(file["local"]["path"]).resolve(strict=True)
        if not (path.is_relative_to(self.files_root) or path.is_relative_to(self.stickers_root)) or not path.is_file() or path.stat().st_size > MAX_FILE:
            raise ValueError("Unexpected media path")
        os.chmod(path, 0o600)
        return path

    def emoji_file_update(self, file):
        targets = self.emoji_downloads.get(file.get("id"), set())
        if not targets: return
        if file.get("@type") == "error" or max(file.get("size", 0), file.get("local", {}).get("downloaded_size", 0)) > MAX_FILE:
            self.emoji_downloads.pop(file["id"], None)
            return
        if not file.get("local", {}).get("is_downloading_completed"): return
        self.emoji_downloads.pop(file["id"], None)
        try: path = self.local_media(file)
        except (ValueError, OSError, KeyError): return
        for key, emoji_id in targets:
            entry = self.entries.get(key)
            if not entry: continue
            part = next((p for p in entry["notice"]["data"].get("parts", []) if p.get("emojiId") == emoji_id and p.get("fileId") == file["id"]), None)
            if part: self.prepare_media(key, path, part["format"], emoji_id)

    def prepare_media(self, key, path, format, emoji_id=None):
        entry = self.entries.get(key)
        if not entry: return
        if format not in ("tgs", "webm", "webp"):
            self.media_ready(key, entry, emoji_id, path.as_uri(), format)
            return
        digest = hashlib.sha256((str(path)+str(path.stat().st_mtime_ns)+"v1").encode()).hexdigest()
        output_format = "png" if format == "webp" else "gif"
        output = self.render_root / (digest + "." + output_format)
        if output.is_file():
            self.media_ready(key, entry, emoji_id, output.as_uri(), output_format)
            return
        if len(self.media_jobs) >= 32:
            self.media_ready(key, entry, emoji_id, "", output_format)
            return
        def convert():
            pending = output.with_suffix("." + secrets.token_hex(4) + ".tmp")
            try:
                subprocess.run([sys.executable, "-B", str(Path(__file__).with_name("telegram_media_render.py")), str(path), str(pending), format],
                               check=True, timeout=35, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                os.chmod(pending, 0o600)
                pending.replace(output)
                return output.as_uri()
            finally:
                pending.unlink(missing_ok=True)
        self.media_jobs[self.media_pool.submit(convert)] = (key, entry, emoji_id, output_format)

    def prune_render_cache(self):
        active = {entry["notice"]["data"].get("source", "") for entry in self.entries.values()}
        active.update(part.get("source", "") for entry in self.entries.values() for part in entry["notice"]["data"].get("parts", []))
        files = sorted([p for p in self.render_root.iterdir() if p.suffix in (".gif", ".png")], key=lambda p:p.stat().st_mtime)
        total = sum(p.stat().st_size for p in files)
        for path in files:
            stat = path.stat()
            if path.as_uri() not in active and (time.time() - stat.st_mtime > 7*86400 or total > 256*1024*1024):
                path.unlink(missing_ok=True); total -= stat.st_size

    def poll_media(self):
        for future, (key, entry, emoji_id, format) in list(self.media_jobs.items()):
            if not future.done(): continue
            self.media_jobs.pop(future)
            try: source = future.result()
            except (OSError, ValueError, subprocess.SubprocessError): source = ""
            self.media_ready(key, entry, emoji_id, source, format)

    def media_ready(self, key, entry, emoji_id, source, format):
        if self.entries.get(key) is not entry: return
        data = entry["notice"]["data"]
        if emoji_id:
            for part in data.get("parts", []):
                if part.get("emojiId") == emoji_id: part.update(source=source, format=format)
        else:
            data.update(source=source, format=format, status="" if source else "Animation unavailable · open in Telegram")
        self.show(key)

    def download_result(self, file_id, result):
        if result.get("@type") == "error":
            for key in self.downloads.pop(file_id, set()):
                if key in self.entries:
                    self.entries[key]["notice"]["data"]["status"] = "Download failed · open in Telegram"
                    self.show(key)
        elif result.get("@type") == "file":
            self.file_update(result)

    def file_update(self, file):
        keys = self.downloads.get(file["id"], set())
        if not keys:
            return
        local = file.get("local", {})
        if max(file.get("size", 0), local.get("downloaded_size", 0)) > MAX_FILE:
            self.td.send("cancelDownloadFile", file_id=file["id"], only_if_pending=False)
            self.download_result(file["id"], {"@type": "error"})
            return
        if not local.get("is_downloading_completed"):
            return
        self.downloads.pop(file["id"], None)
        source = ""
        try:
            path = self.local_media(file)
            source = path.as_uri()
        except (OSError, KeyError, ValueError):
            pass
        for key in keys:
            entry = self.entries.get(key)
            if entry and entry["file"] == file["id"]:
                data = entry["notice"]["data"]
                if source and data["kind"] in ("sticker", "emoji", "animation"):
                    data["status"] = "Rendering animation…"
                    self.prepare_media(key, path, data.get("format", ""))
                else:
                    data.update(source=source, status="" if source else "Attachment unavailable · open in Telegram")
                self.show(key)

    def handle(self, event):
        self.clipboard.handle(event)
        kind = event.get("@type")
        if kind == "updateNewChat":
            self.chat_result(event["chat"]["id"], event["chat"])
        elif kind == "updateChatTitle" and event["chat_id"] in self.chats:
            chat = self.chats[event["chat_id"]]
            self.chat_result(event["chat_id"], {**chat, "title": event["title"]})
        elif kind == "updateChatPhoto" and event["chat_id"] in self.chats:
            chat = self.chats[event["chat_id"]]
            self.chat_result(event["chat_id"], {**chat, "photo": event.get("photo")})
        elif kind == "updateNotificationGroup":
            group = event["notification_group_id"]
            for ident in event.get("removed_notification_ids", []):
                key = self.notifications.pop((group, ident), None)
                if key and key not in self.notifications.values():
                    self.remove(key)
            for notification in event.get("added_notifications", []):
                self.add(group, event["chat_id"], notification)
        elif kind == "updateNotification":
            group, notification = event["notification_group_id"], event["notification"]
            message = notification.get("type", {}).get("message")
            if message:
                self.add(group, message["chat_id"], notification)
        elif kind == "updateFile":
            self.emoji_file_update(event["file"])
            self.avatar_result(event["file"]["id"], event["file"])
            self.file_update(event["file"])
            self.voice_file_update(event["file"])
        elif kind in ("updateMessageSendSucceeded", "updateMessageSendFailed"):
            message = event["message"]
            origin = self.voice_sends.pop((message["chat_id"], event["old_message_id"]), None)
            self.voice_status(origin, "Voice reply sent" if kind == "updateMessageSendSucceeded" else "Voice reply failed · open Telegram to retry")
            if kind == "updateMessageSendSucceeded":
                for file_id, upload in list(self.voice_uploads.items()):
                    if upload.get("chat") == message["chat_id"] and upload.get("message") == event["old_message_id"]:
                        self.finish_voice_upload(file_id)
        elif kind == "updateDeleteMessages" and not event.get("from_cache"):
            for ident in event.get("message_ids", []):
                self.remove(message_key(event["chat_id"], ident))
        elif kind == "updateMessageContent":
            key = message_key(event["chat_id"], event["message_id"])
            entry = self.entries.get(key)
            if entry:
                old = entry["message"].get("content", {})
                new = event["new_content"]
                if old.get("@type") == new.get("@type") == "messageVoiceNote":
                    before, after = dict(old.get("voice_note", {})), dict(new.get("voice_note", {}))
                    before.pop("speech_recognition_result", None); after.pop("speech_recognition_result", None)
                    if before == after:
                        entry["message"]["content"] = new
                        if entry["notice"]["data"].get("kind") == "voice":
                            data = entry["notice"]["data"]
                            data.pop("transcriptionError", None)
                            data.update(transcription_data(new["voice_note"].get("speech_recognition_result")))
                            self.show(key)
                        return
                ref = next((ref for ref, value in self.notifications.items() if value == key), None)
                if ref:
                    self.add(ref[0], event["chat_id"], {"id": ref[1], "date": self.clock(), "type": {
                        "@type": "notificationTypeNewMessage", "show_preview": entry["preview"],
                        "message": {**entry["message"], "content": event["new_content"]}}})

    def voice_status(self, key, status):
        if key in self.entries:
            self.entries[key]["notice"]["data"]["voiceReplyStatus"] = status
            self.show(key)

    def finish_voice_upload(self, file_id):
        upload = self.voice_uploads.pop(file_id, None)
        if upload:
            Path(upload["path"]).unlink(missing_ok=True)
            Path(upload["path"]).with_suffix(".json").unlink(missing_ok=True)

    def voice_file_update(self, file):
        if file.get("remote", {}).get("is_uploading_completed"):
            self.finish_voice_upload(file["id"])

    def recover_voice_uploads(self):
        # Retain copies until TDLib confirms upload completion, including across restarts.
        directory = state_dir() / "voice-outbox"
        for metadata in directory.glob("voice-*.json"):
            try:
                upload = json.loads(metadata.read_text())
                path = metadata.with_suffix(".ogg")
                file_id = int(upload["file_id"])
                if path.is_file():
                    upload["path"] = str(path)
                    self.voice_uploads[file_id] = upload
                    self.voice_sends[(upload.get("chat"), upload.get("message"))] = upload.get("origin")
                    self.td.send("getFile", lambda result: self.voice_file_update(result), file_id=file_id)
                else:
                    metadata.unlink()
            except (ValueError, OSError, KeyError):
                continue

    def voice_reply(self, request, entry, key, done):
        from telegram_recording import copy_for_upload
        path, duration = copy_for_upload(request.get("token"), state_dir() / "voice-outbox")
        chat, message = entry["message"]["chat_id"], entry["message"]["id"]
        def accepted(result):
            if result.get("@type") != "message":
                uncertain = "timed out" in result.get("message", "").lower()
                if not uncertain:
                    path.unlink(missing_ok=True)
                done({"ok": False, "uncertain": uncertain, "error": "Telegram did not confirm; check the chat before retrying" if uncertain
                      else result.get("message", "Telegram rejected the voice reply")})
                return
            self.voice_sends[(chat, result["id"])] = key
            file = result.get("content", {}).get("voice_note", {}).get("voice", {})
            if file.get("id"):
                upload = {"file_id":file["id"], "path":str(path), "chat":chat, "message":result["id"], "origin":key}
                self.voice_uploads[file["id"]] = upload
                private_write(path.with_suffix(".json"), upload)
                self.voice_file_update(file)
            # A copy without an ID is retained: the server may have accepted it.
            self.voice_status(key, "Voice reply queued")
            done({"ok": True})
        self.td.send("sendMessage", accepted, chat_id=chat, topic_id=entry["message"].get("topic_id"),
                     reply_to={"@type":"inputMessageReplyToMessage", "message_id":message} if message else None,
                     input_message_content={"@type":"inputMessageVoiceNote", "voice_note":{
                         "@type":"inputVoiceNote", "voice_note":{"@type":"inputFileLocal", "path":str(path)},
                         "duration":min(300, math.ceil(duration)), "waveform":""}})

    def demo_voice_reply(self, request, done):
        # Demo identifiers are synthetic. Resolve only the signed-in account, never a supplied recipient.
        def me(result):
            names = (result.get("usernames") or {}).get("active_usernames", [])
            if result.get("username"):
                names = [*names, result["username"]]
            if result.get("@type") != "user" or "tijptjik" not in [name.lower() for name in names]:
                done({"ok": False, "error": "Demo recording requires the signed-in @tijptjik account"})
                return
            user_id = result["id"]
            def destination(chat):
                if (chat.get("@type") != "chat" or chat.get("type", {}).get("@type") != "chatTypePrivate"
                        or chat["type"].get("user_id") != user_id):
                    done({"ok": False, "error": "Could not verify Saved Messages for @tijptjik"})
                    return
                try:
                    self.voice_reply(request, {"message":{"chat_id":chat["id"], "id":None}}, "demo-self", done)
                except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
                    done({"ok": False, "error": "Voice draft unavailable or unreadable"})
            self.td.send("createPrivateChat", destination, user_id=user_id, force=True)
        self.td.send("getMe", me)

    def transcribe(self, key, entry, done):
        data = entry["notice"]["data"]
        if data.get("kind") != "voice":
            raise ValueError("Transcription requires a visible voice message")
        if data.get("transcriptionState") in ("pending", "done"):
            done({"ok": True, "message": "Transcription already requested" if data["transcriptionState"] == "pending" else "Transcript is ready"})
            return
        data.update(transcriptionState="pending", transcriptionError="")
        self.show(key)
        chat, message = entry["message"]["chat_id"], entry["message"]["id"]

        def failed(message):
            current = self.entries.get(key)
            if current and current["notice"]["data"].get("transcriptionState") != "done":
                current["notice"]["data"].update(transcriptionState="error", transcriptionError=message[:4096])
                self.show(key)
            done({"ok": False, "error": message})

        def accepted(result):
            if result.get("@type") == "error":
                failed(result.get("message", "Telegram could not transcribe this message"))
            else:
                done({"ok": True, "message": "Transcription requested"})

        def properties(result):
            if key not in self.entries:
                done({"ok": False, "error": "This notification is no longer active"})
            elif result.get("@type") == "error":
                failed(result.get("message", "Could not check transcription availability"))
            elif not result.get("can_recognize_speech"):
                failed("Telegram transcription is unavailable for this message or account")
            else:
                self.td.send("recognizeSpeech", accepted, chat_id=chat, message_id=message)

        self.td.send("getMessageProperties", properties, chat_id=chat, message_id=message)

    def dismiss_read(self, entry, done, explicit_safe=False):
        from notjik import load_config
        options = ({"dismissIsRead": [], "dismissReadMode": "safe"} if explicit_safe
                   else load_config(Path(__file__).resolve().parent)["telegram"])
        chat, message = entry["message"]["chat_id"], entry["message"]["id"]
        rules = options["dismissIsRead"]

        def skip(reason):
            done({"ok": True, "skipped": reason})

        def read():
            def checked(result):
                if result.get("@type") == "ok":
                    done({"ok": True})
                else:
                    done({"ok": False, "error": result.get("message", "Telegram rejected the read marker")})
            self.td.send("viewMessages", checked, chat_id=chat, message_ids=[message],
                         source={"@type": "messageSourceNotification"}, force_read=True)

        def matched():
            if options["dismissReadMode"] == "through":
                read()
                return
            # Topic read markers have separate semantics. Fail closed until
            # their boundaries can be verified independently.
            if entry["message"].get("topic_id") or entry["message"].get("message_thread_id"):
                skip("Cannot verify a topic read boundary")
                return

            def history(result, boundary):
                messages = result.get("messages", [])
                if (result.get("@type") == "messages" and len(messages) >= 2
                        and messages[0].get("id") == message
                        and all(item.get("chat_id") == chat for item in messages[:2])
                        and 0 < messages[1].get("id", 0) <= boundary):
                    read()
                else:
                    skip("Earlier unread messages may be affected")

            def chat_result(result):
                if result.get("@type") != "chat" or result.get("id") != chat:
                    skip("Cannot verify the chat read boundary")
                    return
                boundary = result.get("last_read_inbox_message_id", 0)
                if boundary >= message:
                    skip("Message already read")
                elif result.get("unread_count") == 1 and (result.get("last_message") or {}).get("id") == message:
                    read()
                else:
                    self.td.send("getChatHistory", lambda result: history(result, boundary),
                                 chat_id=chat, from_message_id=message, offset=0, limit=2, only_local=False)
            self.td.send("getChat", chat_result, chat_id=chat)

        if explicit_safe or str(chat) in rules:
            matched()
            return
        usernames = iter(rule[1:] for rule in rules if rule.startswith("@"))

        def resolve_next():
            username = next(usernames, None)
            if username is None:
                skip("Chat is not configured for dismissIsRead")
                return
            def resolved(result):
                if result.get("@type") == "chat" and result.get("id") == chat:
                    matched()
                else:
                    resolve_next()
            self.td.send("searchPublicChat", resolved, username=username)
        resolve_next()

    def action(self, request, done):
        try:
            if request.get("action") == "saveClipboard":
                self.clipboard.start(request.get("token"))
                done({"ok": True, "queued": True})
                return
            if request.get("action") == "demoVoiceReply":
                self.demo_voice_reply(request, done)
                return
            key = message_key(request.get("chatId"), request.get("messageId"))
            entry = self.entries.get(key)
            if not entry:
                raise ValueError("This notification is no longer active")
            chat, message = entry["message"]["chat_id"], entry["message"]["id"]
            op = request.get("action")
            def checked(result):
                if result.get("@type") == "error":
                    done({"ok": False, "error": result.get("message", "Telegram rejected the action")})
                else:
                    done({"ok": True})
            if op in ("dismissRead", "safeRead"):
                self.dismiss_read(entry, done, explicit_safe=op == "safeRead")
            elif op == "transcribe":
                self.transcribe(key, entry, done)
            elif op == "read":
                self.td.send("viewMessages", checked, chat_id=chat, message_ids=[message],
                             source={"@type": "messageSourceNotification"}, force_read=True)
            elif op == "reply":
                text = request.get("text", "")
                if not isinstance(text, str) or not text.strip() or len(text) > 4096:
                    raise ValueError("Reply must contain 1–4096 characters")
                quote = request.get("quote", True)  # Preserve older clients' message replies.
                if not isinstance(quote, bool):
                    raise ValueError("Quote mode must be a boolean")
                self.td.send("sendMessage", checked, chat_id=chat,
                             topic_id=entry["message"].get("topic_id"),
                             reply_to={"@type": "inputMessageReplyToMessage", "message_id": message} if quote else None,
                             input_message_content={"@type": "inputMessageText", "text": {
                                 "@type": "formattedText", "text": text, "entities": []}, "clear_draft": False})
            elif op == "voiceReply":
                self.voice_reply(request, entry, key, done)
            elif op == "open":
                self.td.send("getMessageLink", lambda result: self.open_link(entry, result, done),
                             chat_id=chat, message_id=message, media_timestamp=0, for_album=False, in_message_thread=False)
            else:
                raise ValueError("Unknown Telegram action")
        except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
            done({"ok": False, "error": str(error) if isinstance(error, ValueError) else "Voice draft unavailable or unreadable"})

    def open_link(self, entry, result, done):
        link = result.get("link", "") if result.get("@type") == "messageLink" else ""
        if link:
            self.launch_link(link, done)
            return
        chat = self.chats.get(entry["message"]["chat_id"], {}).get("type", {})
        if chat.get("@type") == "chatTypePrivate":
            self.td.send("getUser", lambda user: self.open_user(user, done), user_id=chat["user_id"])
        else:
            self.launch_link("tg://", done, "Opened Telegram; this chat has no direct desktop link")

    def open_user(self, user, done):
        from urllib.parse import urlencode
        usernames = user.get("usernames", {}).get("active_usernames", [])
        if usernames:
            self.launch_link("tg://resolve?" + urlencode({"domain": usernames[0]}), done)
        elif user.get("phone_number"):
            self.launch_link("tg://resolve?" + urlencode({"phone": user["phone_number"]}), done)
        else:
            self.launch_link("tg://", done, "Opened Telegram; this private chat has no direct desktop link")

    def launch_link(self, link, done, message=""):
        if not (link.startswith("https://t.me/") or link.startswith("tg://resolve?") or link == "tg://"):
            done({"ok": False, "error": "Unexpected Telegram message link"})
            return
        try:
            subprocess.Popen(["xdg-open", link], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            done({"ok": True, "message": message})
        except OSError:
            done({"ok": False, "error": "Could not open Telegram"})



def socket_request(request):
    if not isinstance(request, dict):
        raise ValueError("Telegram request must be an object")
    encoded = json.dumps(request).encode() + b"\n"
    if len(encoded) > 32768:
        raise ValueError("Telegram request too large")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(35)
        client.connect(str(socket_path()))
        try:
            client.sendall(encoded)
            with client.makefile("rb") as stream:
                response = json.loads(stream.readline(32769))
            if not isinstance(response, dict):
                raise ValueError("Invalid Telegram response")
            return response
        except (OSError, ValueError):
            if request.get("action") in ("voiceReply", "saveClipboard"):
                # Once bytes may have reached the bridge, an absent response is
                # not evidence that the send failed. Do not offer a blind retry.
                return {"ok":False, "uncertain":True, "error":"No confirmation from Telegram; check the chat before retrying"}
            raise


class ControlServer(socketserver.ThreadingUnixStreamServer):
    daemon_threads = True
    block_on_close = False

    def __init__(self, requests):
        self.requests = requests
        path = socket_path()
        private_dir(path.parent)
        path.unlink(missing_ok=True)  # Only after acquiring the session's exclusive flock.
        super().__init__(str(path), ControlHandler)
        os.chmod(path, 0o600)


class ControlHandler(socketserver.StreamRequestHandler):
    def handle(self):
        self.connection.settimeout(35)
        try:
            raw = self.rfile.readline(32769)
            if len(raw) > 32768:
                raise ValueError("Telegram request too large")
            request = json.loads(raw)
            if not isinstance(request, dict):
                raise ValueError("Telegram request must be an object")
            answer = queue.Queue(maxsize=1)
            self.server.requests.put_nowait((request, answer))
            response = answer.get(timeout=32)
        except queue.Empty:
            uncertain = request.get("action") in ("voiceReply", "saveClipboard")
            response = {"ok":False, "uncertain":uncertain, "error":"No confirmation from Telegram; check the chat before retrying"
                        if uncertain else "Telegram bridge did not accept the request"}
        except (ValueError, OSError, queue.Full):
            response = {"ok": False, "error": "Telegram bridge did not accept the request"}
        try:
            self.wfile.write(json.dumps(response).encode() + b"\n")
        except OSError:
            pass


def session_lock():
    lock = (private_dir(state_dir()) / "session.lock").open("a")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError as error:
        lock.close()
        raise ValueError("The Telegram session is already in use. Stop notjik-telegram.service before signing in.") from error
    return lock


class DesktopPresence:
    """Keep TDLib presence tied to a reachable, unlocked notification UI."""
    def __init__(self, td):
        self.td = td
        self.online = None
        self.pending = None
        self.generation = 0

    def sync(self, status, ready):
        desired = bool(ready and status.get("lockState") == "unlocked" and not status.get("paused", True))
        if self.pending == desired or (self.pending is None and self.online == desired):
            return
        self.generation += 1
        generation = self.generation
        self.pending = desired
        self.online = None

        def verified(result):
            if generation != self.generation:
                return
            self.pending = None
            self.online = desired if result.get("@type") == "optionValueBoolean" and result.get("value") is desired else None

        def changed(result):
            if generation != self.generation:
                return
            if result.get("@type") != "ok":
                verified({})
                return
            self.td.send("getOption", verified, name="online")

        self.td.send("setOption", changed, name="online", value={"@type": "optionValueBoolean", "value": desired})


def run(root, login=False):
    from notjik import ipc
    os.umask(0o077)
    with session_lock():
        td = TDLib()
        auth = Authorization(td, login)
        presence = DesktopPresence(td)
        bridge = Bridge(td, lambda notice: ipc(root, {"op": "send", "notice": notice}),
                        lambda ident: ipc(root, {"op": "telegramRemove", "id": ident}))
        requests = queue.Queue(maxsize=32)
        server = None
        stopped = False
        connected = False
        options_ready = set()
        configured = False
        heartbeat = 0
        cleanup = 0
        def stop(*_):
            nonlocal stopped
            stopped = True
        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, signal.default_int_handler if login else stop)
        td.send("getOption", name="version")  # Starts TDLib and emits updateAuthorizationState.
        try:
            while not stopped and not auth.closed:
                event = td.receive()
                bridge.poll_media()
                bridge.clipboard.poll()
                if auth.error:
                    if not login:
                        raise ValueError(auth.error)
                    print(auth.error, file=sys.stderr)
                    auth.error = ""
                    retry_state, auth.state = auth.state, None
                    auth.handle(retry_state)
                if event:
                    kind = event.get("@type")
                    if kind == "updateAuthorizationState":
                        auth.handle(event["authorization_state"])
                    elif kind == "updateConnectionState":
                        connected = event["state"]["@type"] == "connectionStateReady"
                        heartbeat = 0
                    elif auth.ready and not login:
                        bridge.handle(event)
                if auth.ready and login:
                    print("Telegram sign-in saved. Start notjik-telegram.service to receive notifications.")
                    break
                if auth.ready and not configured:
                    configured = True
                    bridge.recover_voice_uploads()
                    for name, value in (("notification_group_count_max", 25), ("notification_group_size_max", 10)):
                        def option_result(result, name=name):
                            if result.get("@type") != "ok":
                                raise ValueError("TDLib notification options were rejected")
                            options_ready.add(name)
                        td.send("setOption", option_result, name=name, value={"@type": "optionValueInteger", "value": value})
                    server = ControlServer(requests)
                    threading.Thread(target=server.serve_forever, daemon=True).start()
                while not requests.empty():
                    request, answer = requests.get_nowait()
                    if request.get("action") == "status":
                        answer.put({"ok": True, "authorized": auth.ready, "connected": connected, "online": presence.online,
                                    "active": len(bridge.entries), "delivery": dict(bridge.delivery)})
                    elif not auth.ready or not connected:
                        answer.put({"ok": False, "error": "Telegram is reconnecting"})
                    else:
                        bridge.action(request, answer.put_nowait)
                now = time.monotonic()
                if not login and now >= heartbeat:
                    heartbeat = now + 1
                    for key, entry in list(bridge.entries.items()):
                        if not entry["published"]:
                            bridge.show(key)
                    ready = auth.ready and connected and len(options_ready) == 2
                    active = ready and presence.online is True and all(entry["published"] for entry in bridge.entries.values())
                    try:
                        status = ipc(root, {"op": "telegramBridge", "active": active})
                    except (OSError, ValueError, subprocess.SubprocessError):
                        status = {}
                    if auth.ready:
                        presence.sync(status, ready)
                if auth.ready and not login and now >= cleanup:
                    cleanup = now + 3600
                    bridge.prune_render_cache()
                    # TDLib owns its cache and database; never unlink its files behind its back.
                    td.send("optimizeStorage", size=256 * 1024 * 1024, ttl=7 * 86400, count=10000,
                            immunity_delay=3600, file_types=[], chat_ids=[], exclude_chat_ids=[],
                            return_deleted_file_statistics=False, chat_limit=0)
        finally:
            if not login and auth.ready:
                presence.sync({}, False)
            if server:
                server.shutdown()
                server.server_close()
                socket_path().unlink(missing_ok=True)
            if not login:
                try:
                    ipc(root, {"op": "telegramBridge", "active": False})
                except (OSError, ValueError, subprocess.SubprocessError):
                    pass
            td.send("close")
            deadline = time.monotonic() + 5
            while not auth.closed and time.monotonic() < deadline:
                event = td.receive()
                if event and event.get("@type") == "updateAuthorizationState":
                    auth.closed = event["authorization_state"]["@type"] == "authorizationStateClosed"

        return 0 if not login or auth.ready else 130


def main(root, arguments=None):
    parser = argparse.ArgumentParser(prog="notjikctl telegram")
    parser.add_argument("operation", choices=("login", "run", "status", "action", "doctor", "save-clipboard"))
    args = parser.parse_args(arguments)
    if args.operation == "save-clipboard":
        from telegram_clipboard import save
        return save(root, state_dir(), socket_request)
    try:
        if args.operation in ("login", "run"):
            return run(root, args.operation == "login") or 0
        if args.operation == "doctor":
            api = importlib.import_module("tdjson")
            version = json.loads(api.td_execute(json.dumps({"@type": "getOption", "name": "version"}).encode()))
            print("TDLib:", version.get("value", "available"))
            print("Animated stickers:", "ready" if importlib.util.find_spec("rlottie_python") and shutil.which("ffmpeg") else "run telegram setup and install FFmpeg")
            print("Voice recording:", "ready" if shutil.which("ffmpeg") and shutil.which("ffprobe") else "install FFmpeg and ffprobe")
            print("Account configuration:", "present" if (state_dir() / "credentials.json").exists() else "not signed in")
            return 0
        if args.operation == "status":
            response = socket_request({"action": "status"})
        else:
            request = sys.stdin.buffer.readline(32769)
            if len(request) > 32768:
                raise ValueError("Telegram request too large")
            response = socket_request(json.loads(request))
        print(json.dumps(response), flush=True)
        return 0 if response.get("ok") else 1
    except KeyboardInterrupt:
        print("Telegram sign-in cancelled", file=sys.stderr)
        return 130
    except (OSError, ValueError, ImportError, EOFError) as error:
        message = "Telegram bridge is not running" if isinstance(error, (FileNotFoundError, ConnectionRefusedError)) else str(error)
        if args.operation == "action":
            print(json.dumps({"ok": False, "error": message}), flush=True)
        else:
            print("notjik telegram: " + message, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(Path(__file__).resolve().parent))
