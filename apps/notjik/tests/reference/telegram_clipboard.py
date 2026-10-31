"""Private clipboard snapshots and confirmed delivery to the account's Saved Messages."""
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import subprocess
import threading
import time
from urllib.parse import unquote, urlsplit

MAX_BYTES = 20 * 1024 * 1024
MAX_FILES = 10
APP = "Telegram Saved Messages"


def notice(publish, success, body=""):
    try:
        publish({"app": APP, "title": "Saved to Telegram" if success else "Could not save to Telegram",
                 "body": body, "timeout": 1500 if success else 5000,
                 "urgency": "normal", "transient": True})
    except (OSError, ValueError, subprocess.SubprocessError):
        pass


def clipboard(mime=None):
    command = ["wl-paste", "--no-newline", "--type", mime] if mime else ["wl-paste", "--list-types"]
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as process:
        timer = threading.Timer(5, process.kill)
        timer.start()
        try:
            data = process.stdout.read(MAX_BYTES + 1)
            if len(data) > MAX_BYTES:
                process.kill()
                raise ValueError("Clipboard exceeds the 20 MiB limit")
            status = process.wait()
        finally:
            timer.cancel()
    if status:
        raise ValueError("Clipboard is empty or unavailable")
    return data


def snapshot(root):
    types = clipboard().decode("utf-8").splitlines()
    files, text = [], None
    if "text/uri-list" in types or "x-special/gnome-copied-files" in types:
        mime = "text/uri-list" if "text/uri-list" in types else "x-special/gnome-copied-files"
        lines = clipboard(mime).decode("utf-8").splitlines()
        if mime == "x-special/gnome-copied-files":
            lines = lines[1:]
        for line in lines:
            if not line or line.startswith("#"):
                continue
            uri = urlsplit(line)
            if uri.scheme != "file" or uri.netloc not in ("", "localhost"):
                raise ValueError("Only local files can be sent from a file selection")
            path = Path(unquote(uri.path))
            descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
            if not stat.S_ISREG(os.fstat(descriptor).st_mode):
                os.close(descriptor)
                raise ValueError("Copy files, not folders, to send them to Telegram")
            with os.fdopen(descriptor, "rb") as stream:
                data = stream.read(MAX_BYTES + 1)
            files.append((path.name, data))
            if len(files) > MAX_FILES or sum(len(data) for _, data in files) > MAX_BYTES:
                raise ValueError("Copy at most 10 files totaling 20 MiB")
    else:
        images = {"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif"}
        mime = next((kind for kind in images if kind in types), None)
        if mime:
            files = [("clipboard." + images[mime], clipboard(mime))]
        else:
            mime = next((kind for kind in ("text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING") if kind in types), None)
            if not mime:
                raise ValueError("Clipboard has no supported text, image, or files")
            data = clipboard(mime)
            text = data.decode("utf-8")
            if not text.strip():
                raise ValueError("Clipboard is empty")
            # Keep long text intact as an attachment rather than truncate it.
            if len(text.encode("utf-16-le")) // 2 > 4096:
                files, text = [("clipboard.txt", data)], None
    if text is None and not files:
        raise ValueError("Clipboard file selection is empty")
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    token = secrets.token_hex(16)
    directory = root / token
    directory.mkdir(mode=0o700)
    try:
        items = [{"text": text}] if text is not None else []
        for index, (name, data) in enumerate(files):
            folder = directory / str(index)
            folder.mkdir(mode=0o700)
            path = folder / name
            with path.open("xb") as stream:
                os.chmod(path, 0o600)
                stream.write(data)
            items.append({"file": str(path.relative_to(directory))})
        manifest = directory / "items.json"
        with manifest.open("x") as stream:
            os.chmod(manifest, 0o600)
            json.dump(items, stream)
        return token
    except Exception:
        shutil.rmtree(directory)
        raise


class SavedClipboard:
    def __init__(self, td, publish, root):
        self.td, self.publish, self.root = td, publish, root
        self.jobs = {}
        self.pending = {}

    def start(self, token):
        if not isinstance(token, str) or not re.fullmatch(r"[0-9a-f]{32}", token):
            raise ValueError("Invalid clipboard snapshot")
        if token in self.jobs:
            raise ValueError("This clipboard snapshot is already being sent")
        directory = self.root / token
        items = json.loads((directory / "items.json").read_text())
        if not isinstance(items, list) or not 1 <= len(items) <= MAX_FILES:
            raise ValueError("Invalid clipboard snapshot")
        contents = []
        for item in items:
            if isinstance(item.get("text"), str):
                text = item["text"]
                if not text.strip() or len(text.encode("utf-16-le")) // 2 > 4096:
                    raise ValueError("Invalid clipboard text")
                contents.append({"@type": "inputMessageText", "text": {
                    "@type": "formattedText", "text": text, "entities": []}, "clear_draft": False})
            else:
                path = (directory / item["file"]).resolve()
                if not path.is_relative_to(directory.resolve()) or not path.is_file() or path.stat().st_size > MAX_BYTES:
                    raise ValueError("Invalid clipboard attachment")
                # Documents preserve original image bytes and copied filenames.
                contents.append({"@type": "inputMessageDocument", "document": {
                    "@type": "inputDocument", "document": {"@type": "inputFileLocal", "path": str(path)},
                    "disable_content_type_detection": False}})
        job = {"contents": contents, "directory": directory, "deadline": time.monotonic() + 90,
               "warned": False, "sent": 0}
        self.jobs[token] = job

        def me(user):
            if user.get("@type") != "user" or not user.get("id"):
                self.fail(token, "Could not identify your Telegram account")
                return

            def destination(chat):
                if (chat.get("@type") != "chat" or chat.get("type", {}).get("@type") != "chatTypePrivate"
                        or chat["type"].get("user_id") != user["id"]):
                    self.fail(token, "Could not verify Saved Messages")
                    return
                job["chat"] = chat["id"]
                self.send_next(token)
            self.td.send("createPrivateChat", destination, user_id=user["id"], force=True)
        self.td.send("getMe", me)

    def fail(self, token, body):
        job = self.jobs.pop(token, None)
        if job:
            prefix = f"{job['sent']} file(s) saved. " if job["sent"] else ""
            notice(self.publish, False, prefix + body)
        # Keep private snapshots when delivery is uncertain; never retry blindly.

    def send_next(self, token):
        job = self.jobs[token]
        if not job["contents"]:
            self.jobs.pop(token)
            shutil.rmtree(job["directory"], ignore_errors=True)
            notice(self.publish, True)
            return
        job["deadline"] = time.monotonic() + 90
        content = job["contents"].pop(0)

        def accepted(message):
            if message.get("@type") != "message" or message.get("chat_id") != job["chat"]:
                self.fail(token, "Telegram did not confirm the send; check Saved Messages before retrying")
            elif (message.get("sending_state") or {}).get("@type") == "messageSendingStateFailed":
                self.fail(token, "Telegram could not send this item")
            elif message.get("sending_state"):
                self.pending[(job["chat"], message["id"])] = token
            else:
                job["sent"] += 1
                self.send_next(token)
        self.td.send("sendMessage", accepted, chat_id=job["chat"], input_message_content=content)

    def handle(self, event):
        kind = event.get("@type")
        if kind not in ("updateMessageSendSucceeded", "updateMessageSendFailed"):
            return
        token = self.pending.pop((event["message"]["chat_id"], event["old_message_id"]), None)
        if token is None:
            return
        if kind == "updateMessageSendFailed":
            self.fail(token, "Telegram could not send this item; check Saved Messages before retrying")
        else:
            self.jobs[token]["sent"] += 1
            self.send_next(token)

    def poll(self):
        for job in self.jobs.values():
            if not job["warned"] and time.monotonic() > job["deadline"]:
                job["warned"] = True
                notice(self.publish, False, "Send is still pending; check Saved Messages before retrying")


def save(root, state, request):
    from notjik import ipc
    publish = lambda value: ipc(root, {"op": "send", "notice": value})
    token = None
    try:
        status = request({"action": "status"})
        if not status.get("authorized") or not status.get("connected"):
            raise ValueError("Telegram is not connected")
        token = snapshot(state / "clipboard-outbox")
        response = request({"action": "saveClipboard", "token": token})
        if not response.get("ok"):
            if not response.get("uncertain"):
                shutil.rmtree(state / "clipboard-outbox" / token, ignore_errors=True)
            raise ValueError(response.get("error", "Telegram could not accept the clipboard"))
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        body = "Run notjikctl telegram login, then start notjik-telegram.service" if isinstance(
            error, (FileNotFoundError, ConnectionRefusedError)) else str(error)
        notice(publish, False, body)
        return 1
