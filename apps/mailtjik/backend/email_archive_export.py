#!/usr/bin/env python3
"""Stream encrypted mail views, raw messages, and attachments to Google Drive."""

from __future__ import annotations

import argparse
import email
import email.policy
import email.utils
import hashlib
import io
import json
import os
import queue
import re
import sqlite3
import subprocess
import sys
import tarfile
import threading
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import BinaryIO

import email_archive_index as index


HOME = Path.home()
DRIVE_MOUNT = HOME / ".mnt/google/private"
AGE_POLICY = Path("home/dot_config/document-archive-policy.json")
REMOTE_ROOT = "type-drive:private/archives/mail/structured"
CHUNK_BYTES = 512 * 1024 * 1024
MAX_CACHE_BYTES = 10 * 1024 * 1024 * 1024
RESUME_CACHE_BYTES = 6 * 1024 * 1024 * 1024


class MountBackpressure:
    """Pause producers when the managed mount's upload cache grows too large."""

    def __init__(self):
        self.allow = threading.Event()
        self.stopping = threading.Event()
        self.error: str | None = None
        self.thread = threading.Thread(target=self._monitor, daemon=True)
        self.thread.start()

    @staticmethod
    def _bytes(status: str) -> int | None:
        match = re.search(r"total size ([0-9.]+)(?: (B|[KMGT]i?B))?\b", status)
        if not match:
            return None
        value = float(match.group(1))
        unit = match.group(2) or "B"
        multiplier = {
            "B": 1, "KB": 1000, "KiB": 1024,
            "MB": 1000**2, "MiB": 1024**2,
            "GB": 1000**3, "GiB": 1024**3,
            "TB": 1000**4, "TiB": 1024**4,
        }[unit]
        return int(value * multiplier)

    def _monitor(self) -> None:
        while not self.stopping.is_set():
            try:
                result = subprocess.run(
                    ["systemctl", "--user", "show", "private-mount-giprivate.service",
                     "--property=StatusText", "--value"],
                    check=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                    timeout=8,
                )
                status = result.stdout.strip()
                total = self._bytes(status)
                if total is None:
                    self.error = "managed Drive mount status did not report cache size"
                    self.allow.clear()
                elif total >= MAX_CACHE_BYTES:
                    self.allow.clear()
                elif total <= RESUME_CACHE_BYTES:
                    self.allow.set()
                    self.error = None
            except (OSError, subprocess.SubprocessError) as error:
                self.error = str(error)
                self.allow.clear()
            self.stopping.wait(10)

    def wait(self) -> None:
        started = time.monotonic()
        while not self.allow.wait(1):
            if self.stopping.is_set():
                raise RuntimeError("Drive upload monitor stopped")
            if self.error and time.monotonic() - started > 60:
                raise RuntimeError(f"Drive upload status unavailable: {self.error}")

    def stop(self) -> None:
        self.stopping.set()
        self.thread.join(timeout=10)
        if self.error:
            raise RuntimeError(f"Drive upload status check failed: {self.error}")


BACKPRESSURE: MountBackpressure | None = None


def short_hash(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8", "surrogateescape")).hexdigest()[:20]


def safe_filename(value: str | None) -> str:
    name = Path(value or "attachment.bin").name
    name = re.sub(r"[^A-Za-z0-9._ -]", "_", name).strip(" .")
    return (name or "attachment.bin")[:160]


def is_inline(part: email.message.Message) -> bool:
    disposition = (part.get_content_disposition() or "").casefold()
    return disposition == "inline" or (
        part.get_content_type().casefold().startswith("image/") and bool(part.get("Content-ID"))
    )


def is_attachment(part: email.message.Message) -> bool:
    attachment_body = not part.is_multipart() or part.get_content_type().casefold() == "message/rfc822"
    return attachment_body and not is_inline(part) and (
        part.get_content_disposition() == "attachment" or bool(part.get_filename())
    )


def extract_attachments(message: email.message.Message) -> list[tuple[str, str, bytes]]:
    found = []
    for part in message.walk():
        if not is_attachment(part):
            continue
        content = index.decoded_part_bytes(part)
        found.append((hashlib.sha256(content).hexdigest(), safe_filename(part.get_filename()), content))
    return found


def clean_reader_message(message: email.message.Message, attachment_refs: list[tuple[str, str, bytes]]) -> bytes:
    """Remove non-inline attachments from the reader view, leaving raw mail elsewhere."""
    placeholders = [
        f"Attachment stored separately: {name} (sha256 {digest}, {len(data)} bytes)"
        for digest, name, data in attachment_refs
    ]

    def prune(part: email.message.Message) -> None:
        if not part.is_multipart():
            return
        payload = part.get_payload()
        if not isinstance(payload, list):
            return
        kept = []
        for child in payload:
            if is_attachment(child):
                continue
            prune(child)
            kept.append(child)
        part.set_payload(kept)

    prune(message)
    if placeholders:
        note = email.message.EmailMessage(policy=email.policy.default)
        note.set_content("\n".join(placeholders))
        if is_attachment(message):
            for key in list(message.keys()):
                if key.casefold().startswith("content-") or key.casefold() == "mime-version":
                    del message[key]
            message.set_content("\n".join(placeholders))
        elif message.is_multipart() and message.get_content_subtype() == "mixed":
            message.attach(note)
        elif message.is_multipart():
            wrapper = email.message.EmailMessage(policy=email.policy.default)
            for key, value in message.items():
                if key.casefold() not in {"mime-version", "content-type", "content-transfer-encoding"}:
                    wrapper[key] = value
            wrapper.make_mixed()
            wrapper.attach(message)
            wrapper.attach(note)
            message = wrapper
        else:
            original = index.decoded_part_bytes(message)
            text = original.decode(message.get_content_charset() or "utf-8", "replace")
            message.set_content(text + "\n\n" + "\n".join(placeholders))
    return message.as_bytes(policy=email.policy.SMTP)


def mbox_record(message: email.message.Message, clean_bytes: bytes) -> bytes:
    sender = email.utils.parseaddr(str(message.get("From", "")))[1] or "MAILER-DAEMON"
    try:
        date = email.utils.parsedate_to_datetime(str(message.get("Date", "")))
        if date is None:
            raise ValueError("missing date")
        date_text = date.astimezone(timezone.utc).strftime("%a %b %d %H:%M:%S %Y")
    except (TypeError, ValueError, OverflowError):
        date_text = "Thu Jan 01 00:00:00 1970"
    body = re.sub(rb"(?m)^From ", b">From ", clean_bytes)
    if not body.endswith((b"\n", b"\r")):
        body += b"\r\n"
    return f"From {sender} {date_text}\n".encode("utf-8", "replace") + body


class EncryptedOutput:
    """zstd -> age -> managed Drive FUSE mount; plaintext is never staged."""

    def __init__(self, remote: str, recipient: str):
        prefix = "type-drive:private/"
        if not remote.startswith(prefix):
            raise ValueError("archive target must be below the managed Google Drive private remote")
        if not os.path.ismount(DRIVE_MOUNT):
            raise RuntimeError("managed Google Drive mount is unavailable")
        self.path = DRIVE_MOUNT / remote[len(prefix):]
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.file = self.path.open("xb")
        self.age = subprocess.Popen(
            ["age", "-r", recipient], stdin=subprocess.PIPE,
            stdout=self.file, stderr=subprocess.DEVNULL,
        )
        assert self.age.stdin is not None
        self.zstd = subprocess.Popen(
            ["zstd", "-T1", "-3", "-c"], stdin=subprocess.PIPE,
            stdout=self.age.stdin, stderr=subprocess.DEVNULL,
        )
        self.age.stdin.close()
        assert self.zstd.stdin is not None
        self.stdin = self.zstd.stdin

    def close(self) -> None:
        try:
            self.stdin.close()
        except BrokenPipeError:
            pass
        zstd_status = self.zstd.wait()
        age_status = self.age.wait()
        self.file.flush()
        self.file.close()
        if zstd_status or age_status:
            raise RuntimeError(f"encrypted stream failed (zstd={zstd_status}, age={age_status})")


class TarUpload:
    """Thread-owned, chunked tar.zst.age writer with a bounded memory queue."""

    def __init__(self, remote: str, recipient: str):
        self.remote = remote
        self.recipient = recipient
        self.events: queue.Queue[tuple[str, bytes] | None] = queue.Queue(maxsize=2)
        self.error: BaseException | None = None
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self) -> None:
        try:
            part = 0
            part_bytes = 0
            output = None
            archive = None

            def start_part() -> tuple[EncryptedOutput, tarfile.TarFile]:
                nonlocal part
                if BACKPRESSURE is not None:
                    BACKPRESSURE.wait()
                part += 1
                target = re.sub(r"\.tar\.zst\.age$", f"-{part:04d}.tar.zst.age", self.remote)
                encrypted = EncryptedOutput(target, self.recipient)
                stream = tarfile.open(fileobj=encrypted.stdin, mode="w|")
                return encrypted, stream

            while True:
                event = self.events.get()
                if event is None:
                    break
                name, content = event
                if archive is None or (part_bytes and part_bytes + len(content) > CHUNK_BYTES):
                    if archive is not None and output is not None:
                        archive.close()
                        output.close()
                    output, archive = start_part()
                    part_bytes = 0
                info = tarfile.TarInfo(name)
                info.size = len(content)
                info.mode = 0o600
                info.mtime = 0
                archive.addfile(info, io.BytesIO(content))
                part_bytes += len(content)
            if archive is None:
                output, archive = start_part()
            archive.close()
            assert output is not None
            output.close()
        except BaseException as error:
            self.error = error

    def add(self, name: str, content: bytes) -> None:
        if BACKPRESSURE is None:
            raise RuntimeError("Drive backpressure monitor is not active")
        BACKPRESSURE.wait()
        self._put((name, content))

    def _put(self, event: tuple[str, bytes] | None) -> None:
        while True:
            if self.error:
                raise RuntimeError("archive upload worker failed") from self.error
            try:
                self.events.put(event, timeout=0.5)
                return
            except queue.Full:
                continue

    def close(self) -> None:
        self._put(None)
        self.thread.join()
        if self.error:
            raise RuntimeError(f"archive upload failed for {self.remote}") from self.error


class MboxUpload:
    def __init__(self, remote: str, recipient: str):
        self.remote = remote
        self.recipient = recipient
        self.part = 0
        self.part_bytes = 0
        self.current_remote: str | None = None
        self.output: EncryptedOutput | None = None
        self._start_part()

    def _start_part(self) -> None:
        if BACKPRESSURE is not None:
            BACKPRESSURE.wait()
        self.part += 1
        target = re.sub(r"\.mbox\.zst\.age$", f".part-{self.part:04d}.mbox.zst.age", self.remote)
        self.current_remote = target
        self.output = EncryptedOutput(target, self.recipient)
        self.part_bytes = 0

    def write(self, content: bytes) -> str:
        if BACKPRESSURE is None:
            raise RuntimeError("Drive backpressure monitor is not active")
        BACKPRESSURE.wait()
        if self.output is not None and self.part_bytes and self.part_bytes + len(content) > CHUNK_BYTES:
            self.output.close()
            self._start_part()
        assert self.output is not None
        self.output.stdin.write(content)
        self.part_bytes += len(content)
        assert self.current_remote is not None
        return self.current_remote

    def close(self) -> None:
        if self.output is not None:
            self.output.close()
            self.output = None


def account_slugs(db: sqlite3.Connection) -> dict[str, str]:
    result = {}
    for address, priority in db.execute(
        "SELECT address,priority FROM accounts WHERE kind='canonical' ORDER BY priority"
    ):
        result[str(address)] = f"account-{int(priority):02d}"
    result["unresolved"] = "unresolved"
    return result


def get_tar_writer(writers: dict[str, TarUpload], slug: str, kind: str,
                   run_root: str, recipient: str) -> TarUpload:
    if slug not in writers:
        remote = f"{run_root}/{slug}/{kind}.tar.zst.age"
        writers[slug] = TarUpload(remote, recipient)
    return writers[slug]


def canonical_membership(db: sqlite3.Connection, digest: str, folder: str,
                         source_paths: list[Path]) -> int | None:
    """Choose one reader-view membership among the sources being exported.

    The index may include duplicate provenance from both Drive and a staged
    local copy. If the exporter uses only the staged copy, an earlier Drive
    membership must not suppress its MBOX record.
    """
    sources = [str(path) for path in source_paths]
    if not sources:
        return None
    placeholders = ",".join("?" for _ in sources)
    row = db.execute(
        f"SELECT MIN(id) FROM memberships WHERE message_sha256=? "
        f"AND original_mailbox_path=? AND source_archive IN ({placeholders})",
        [digest, folder, *sources],
    ).fetchone()
    return int(row[0]) if row and row[0] is not None else None


def membership_id(db: sqlite3.Connection, digest: str, source: str, folder: str,
                  offset: int, ordinal: int) -> int | None:
    row = db.execute("""
        SELECT id FROM memberships WHERE message_sha256=? AND source_archive=?
          AND original_mailbox_path=? AND mailbox_offset=? AND message_ordinal=?
    """, (digest, source, folder, offset, ordinal)).fetchone()
    return int(row[0]) if row else None


def all_mail_has_other_folder(db: sqlite3.Connection, digest: str, account: str, folder: str) -> bool:
    row = db.execute("""
        SELECT 1 FROM memberships x JOIN messages m ON m.sha256=x.message_sha256
        WHERE x.message_sha256=? AND m.account=? AND x.original_mailbox_path<>?
          AND lower(x.original_mailbox_path) NOT LIKE '%all mail%'
        LIMIT 1
    """, (digest, account, folder)).fetchone()
    return row is not None


def is_all_mail(folder: str) -> bool:
    label = folder.casefold().split("/")[-1]
    normalized = re.sub(r"[ _-]+", " ", label).strip()
    return normalized.startswith("all mail") or normalized == "allmail"


def mapped_folder(folder: str) -> str:
    if is_all_mail(folder):
        return "Gmail Archive"
    return folder or "Unfiled"


def add_export_record(db: sqlite3.Connection, membership: int, included: bool,
                      account: str, output_path: str | None, reason: str) -> None:
    db.execute("""
        INSERT OR REPLACE INTO mailbox_exports(
            membership_id,included_in_reader_view,account,output_path,decision
        ) VALUES(?,?,?,?,?)
    """, (membership, int(included), account, output_path, reason))


def reader_view(message: email.message.Message, attachments: list[tuple[str, str, bytes]]) -> bytes:
    # Parser returns a fresh object for each message, so pruning is local to this view.
    return clean_reader_message(message, attachments)


def rewrite_manifest(db: sqlite3.Connection, path: Path) -> int:
    count = 0
    with path.open("w", encoding="utf-8", newline="\n") as output:
        for address, priority, kind in db.execute(
            "SELECT address,priority,kind FROM accounts ORDER BY priority"
        ):
            output.write(json.dumps({
                "record_type": "account", "address": address,
                "priority": priority, "kind": kind,
            }, ensure_ascii=False, separators=(",", ":")) + "\n")
            count += 1
        for row in db.execute("""
            SELECT address,canonical_account,provenance,confidence FROM aliases ORDER BY address
        """):
            output.write(json.dumps({
                "record_type": "alias", "address": row[0],
                "canonical_account": row[1], "provenance": row[2],
                "confidence": row[3],
            }, ensure_ascii=False, separators=(",", ":")) + "\n")
            count += 1
        for row in db.execute("""
            SELECT source_archive,prefs_path,account_id,server_id,server_type,hostname,
                   username,mailbox_path,mailbox_path_relative,smtp_server_id,smtp_hostname,
                   smtp_username,identity_address,reply_to,full_name
            FROM thunderbird_account_config ORDER BY source_archive,prefs_path,account_id
        """):
            output.write(json.dumps({
                "record_type": "thunderbird_account_config",
                "source_archive": row[0], "prefs_path": row[1], "account_id": row[2],
                "server_id": row[3], "server_type": row[4], "hostname": row[5],
                "username": row[6], "mailbox_path": row[7],
                "mailbox_path_relative": row[8], "smtp_server_id": row[9],
                "smtp_hostname": row[10], "smtp_username": row[11],
                "identity_address": row[12], "reply_to": row[13], "full_name": row[14],
            }, ensure_ascii=False, separators=(",", ":")) + "\n")
            count += 1
        rows = db.execute("""
            SELECT m.sha256,m.byte_size,m.subject,m.original_date,m.normalized_utc,
                   m.from_header,m.to_header,m.cc_header,m.bcc_header,m.sender_header,
                   m.from_addresses,m.to_addresses,m.cc_addresses,m.bcc_addresses,
                   m.sender_addresses,m.message_id,m.has_body,m.text_body_bytes,
                   m.attachment_count,m.attachment_bytes,m.inline_image_count,
                   m.account,m.attribution_reason,m.attribution_confidence,
                   x.id,x.source_archive,x.original_mailbox_path,x.locator,
                   x.mailbox_offset,x.message_ordinal,e.included_in_reader_view,
                   e.output_path,e.decision
            FROM memberships x JOIN messages m ON m.sha256=x.message_sha256
            LEFT JOIN mailbox_exports e ON e.membership_id=x.id
            ORDER BY m.account,x.source_archive,x.original_mailbox_path,x.message_ordinal
        """)
        for row in rows:
            (sha,size,subject,original_date,normalized_utc,from_header,to_header,cc_header,
             bcc_header,sender_header,from_v,to_v,cc_v,bcc_v,sender_v,message_id,has_body,
             text_bytes,attachment_count,attachment_bytes,inline_count,account,reason,
             confidence,membership,source,mailbox_path,locator,offset,ordinal,included,
             output_path,decision) = row
            record = {
                "record_type": "message",
                "account": account or "unresolved",
                "attribution_reason": reason,
                "attribution_confidence": confidence,
                "subject": subject,
                "original_date": original_date,
                "normalized_utc_date": normalized_utc,
                "from_header": from_header,
                "to_header": to_header,
                "cc_header": cc_header,
                "bcc_header": bcc_header,
                "sender_header": sender_header,
                "from": json.loads(from_v or "[]"),
                "to": json.loads(to_v or "[]"),
                "cc": json.loads(cc_v or "[]"),
                "bcc": json.loads(bcc_v or "[]"),
                "sender": json.loads(sender_v or "[]"),
                "message_id": message_id,
                "original_archive_path": source,
                "original_mailbox_path": mailbox_path,
                "archive_mailbox_path": mapped_folder(mailbox_path),
                "message_locator": json.loads(locator),
                "byte_size": size,
                "sha256": sha,
                "has_body": bool(has_body),
                "text_body_bytes": text_bytes,
                "attachment_count": attachment_count,
                "attachment_bytes": attachment_bytes,
                "inline_image_count": inline_count,
                "included_in_reader_view": None if included is None else bool(included),
                "reader_view_path": output_path,
                "reader_view_decision": decision,
            }
            output.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
            count += 1
        for row in db.execute("""
            SELECT o.message_sha256,o.part_index,o.attachment_sha256,o.filename,
                   o.mime_type,o.content_disposition,o.content_id,o.is_inline,a.byte_size
            FROM attachment_occurrences o JOIN attachments a ON a.sha256=o.attachment_sha256
            ORDER BY o.message_sha256,o.part_index
        """):
            (message_hash,part_index,attachment_hash,filename,mime,disposition,cid,inline,size)=row
            output.write(json.dumps({
                "record_type":"attachment","message_sha256":message_hash,
                "part_index":part_index,"attachment_sha256":attachment_hash,
                "filename":filename,"mime_type":mime,"content_disposition":disposition,
                "content_id":cid,"is_inline":bool(inline),"byte_size":size,
            },ensure_ascii=False,separators=(",",":"))+"\n")
            count += 1
    os.chmod(path, 0o600)
    return count


def export_index(db_path: Path, manifest: Path, recipient: str, run_root: str) -> None:
    remote = f"{run_root}/index.tar.zst.age"
    output = EncryptedOutput(remote, recipient)
    try:
        with tarfile.open(fileobj=output.stdin, mode="w|") as archive:
            for path, name in ((db_path, "index/email-index.sqlite"), (manifest, "index/manifest.jsonl")):
                info = archive.gettarinfo(str(path), arcname=name)
                info.mode = 0o600
                with path.open("rb") as stream:
                    archive.addfile(info, stream)
            readme = (
                "Encrypted email archive index v1\n"
                "Decrypt with age using the recoverable identity, then extract with tar --zstd.\n"
                "SQLite index uses FTS5 over subject and addresses; it does not index message bodies.\n"
                "MBOX views are standard mbox files after age decryption and zstd decompression.\n"
                "Non-inline attachments are in separate compressed attachment archives.\n"
            ).encode()
            info = tarfile.TarInfo("README.txt")
            info.size = len(readme)
            info.mode = 0o600
            info.mtime = 0
            archive.addfile(info, io.BytesIO(readme))
    finally:
        output.close()


def wait_for_mount_idle(timeout: int = 3600) -> str:
    deadline = time.monotonic() + timeout
    consecutive = 0
    latest = ""
    while time.monotonic() < deadline:
        result = subprocess.run(
            ["systemctl", "--user", "show", "private-mount-giprivate.service",
             "--property=StatusText", "--value"],
            check=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            timeout=8,
        )
        latest = result.stdout.strip()
        total = MountBackpressure._bytes(latest)
        idle = total == 0 and "in use 0" in latest and "to upload 0" in latest and "uploading 0" in latest
        consecutive = consecutive + 1 if idle else 0
        if consecutive >= 2:
            return latest
        time.sleep(5)
    raise TimeoutError(f"managed Drive mount did not drain within {timeout}s; latest status: {latest}")


def export(config_path: Path, workdir: Path, recipient: str, run_root: str) -> dict[str, int]:
    global BACKPRESSURE
    BACKPRESSURE = MountBackpressure()
    configuration = json.loads(config_path.read_text(encoding="utf-8"))
    sources = [Path(value) for value in configuration["source_paths"]]
    db_path = workdir / "email-index.sqlite"
    manifest_path = workdir / "manifest.jsonl"
    db = sqlite3.connect(db_path)
    db.execute("PRAGMA foreign_keys=ON")
    source_names = [str(path) for path in sources]
    if not source_names:
        db.close()
        raise ValueError("export config must contain at least one source archive")
    placeholders = ",".join("?" for _ in source_names)
    missing_messages = db.execute(
        "SELECT count(*) FROM messages m WHERE NOT EXISTS ("
        "SELECT 1 FROM memberships x WHERE x.message_sha256=m.sha256 "
        f"AND x.source_archive IN ({placeholders}))",
        source_names,
    ).fetchone()[0]
    if missing_messages:
        db.close()
        raise RuntimeError(
            f"export source selection omits {missing_messages} indexed raw messages"
        )
    db.execute("""
        CREATE TABLE IF NOT EXISTS mailbox_exports (
            membership_id INTEGER PRIMARY KEY REFERENCES memberships(id),
            included_in_reader_view INTEGER NOT NULL,
            account TEXT NOT NULL,
            output_path TEXT,
            decision TEXT NOT NULL
        )
    """)
    db.commit()
    slugs = account_slugs(db)
    raw_writers: dict[str, TarUpload] = {}
    attachment_writers: dict[str, TarUpload] = {}
    raw_seen: dict[str, set[str]] = {slug: set() for slug in slugs.values()}
    attachment_seen: dict[str, set[str]] = {slug: set() for slug in slugs.values()}
    counters = {"raw_messages": 0, "attachments": 0, "mailbox_memberships": 0, "mailbox_suppressed": 0}
    try:
        for source in sources:
            for member, stream in index.iter_archive_members(source):
                if Path(member).name.casefold() == "prefs.js" or index.skip_member(member):
                    stream.close()
                    continue
                sniff = stream.read(512)
                try:
                    can_seek = stream.seekable()
                except (AttributeError, OSError):
                    can_seek = False
                if can_seek:
                    stream.seek(0)
                    replay: BinaryIO = stream
                else:
                    replay = index.PrefixReader(sniff, stream)  # type: ignore[assignment]
                if not (sniff.startswith(b"From ") or index.looks_like_message(sniff)):
                    stream.close()
                    continue
                folder = index.clean_folder(source, member)
                source_name = str(source)
                source_key = short_hash(source_name)
                member_key = short_hash(member)
                mbox_sinks: dict[str, MboxUpload] = {}
                mailbox_seen: dict[str, set[str]] = {}
                ordinal = 0
                for offset, raw in index.iter_message_blobs(replay):
                    ordinal += 1
                    digest = hashlib.sha256(raw).hexdigest()
                    row = db.execute("SELECT account,byte_size FROM messages WHERE sha256=?", (digest,)).fetchone()
                    if row is None or int(row[1]) != len(raw):
                        raise RuntimeError("source message no longer matches the indexed snapshot")
                    account = str(row[0]) if row[0] else "unresolved"
                    slug = slugs.get(account, "unresolved")

                    if digest not in raw_seen[slug]:
                        raw_seen[slug].add(digest)
                        get_tar_writer(raw_writers, slug, "raw-messages", run_root, recipient).add(
                            f"raw/{digest}.eml", raw
                        )
                        counters["raw_messages"] += 1

                    parsed = email.message_from_bytes(raw, policy=email.policy.default)
                    attachments = extract_attachments(parsed)
                    for attachment_hash, filename, content in attachments:
                        expected = db.execute("SELECT byte_size FROM attachments WHERE sha256=?", (attachment_hash,)).fetchone()
                        if expected is None or int(expected[0]) != len(content):
                            raise RuntimeError("attachment bytes do not match the indexed source")
                        if attachment_hash not in attachment_seen[slug]:
                            attachment_seen[slug].add(attachment_hash)
                            get_tar_writer(attachment_writers, slug, "attachments", run_root, recipient).add(
                                f"attachments/{attachment_hash}/{filename}", content
                            )
                            counters["attachments"] += 1

                    current_id = membership_id(db, digest, source_name, folder, offset, ordinal)
                    if current_id is None:
                        raise RuntimeError("source mailbox membership is absent from the index")
                    first_id = canonical_membership(db, digest, folder, sources)
                    if first_id != current_id:
                        add_export_record(db, current_id, False, account, None, "exact duplicate membership; retained in index")
                        continue
                    if is_all_mail(folder) and all_mail_has_other_folder(db, digest, account, folder):
                        add_export_record(db, current_id, False, account, None, "duplicate present in another Gmail folder")
                        counters["mailbox_suppressed"] += 1
                        continue

                    mapped = mapped_folder(folder)
                    mailbox_key = short_hash(mapped)
                    remote = f"{run_root}/{slug}/mailboxes/{source_key}/{member_key}-{mailbox_key}.mbox.zst.age"
                    if slug not in mbox_sinks:
                        mbox_sinks[slug] = MboxUpload(remote, recipient)
                        mailbox_seen[slug] = set()
                    if digest in mailbox_seen[slug]:
                        add_export_record(db, current_id, False, account, None, "exact duplicate in mailbox")
                        continue
                    mailbox_seen[slug].add(digest)
                    clean = reader_view(parsed, attachments)
                    output_path = mbox_sinks[slug].write(mbox_record(parsed, clean))
                    add_export_record(db, current_id, True, account, output_path, "included")
                    counters["mailbox_memberships"] += 1

                for sink in mbox_sinks.values():
                    sink.close()
                if replay is not stream and hasattr(replay, "close"):
                    replay.close()
                stream.close()
                if ordinal and ordinal % 10000 == 0:
                    db.commit()
                    print(json.dumps({"exported_message_records": ordinal, **counters}), file=sys.stderr, flush=True)
            db.commit()
        for writer in raw_writers.values():
            writer.close()
        for writer in attachment_writers.values():
            writer.close()
        db.commit()
        expected_raw_messages = db.execute("SELECT count(*) FROM messages").fetchone()[0]
        if counters["raw_messages"] != expected_raw_messages:
            raise RuntimeError(
                f"exported {counters['raw_messages']} unique raw messages; "
                f"index contains {expected_raw_messages}"
            )
        manifest_count = rewrite_manifest(db, manifest_path)
        db.execute("ANALYZE")
        db.commit()
        export_index(db_path, manifest_path, recipient, run_root)
        counters["manifest_rows"] = manifest_count
        BACKPRESSURE.stop()
        wait_for_mount_idle()
        return counters
    finally:
        db.close()
        if BACKPRESSURE is not None and not BACKPRESSURE.stopping.is_set():
            BACKPRESSURE.stop()


def main() -> int:
    os.umask(0o077)
    parser = argparse.ArgumentParser()
    parser.add_argument("--workdir", type=Path, default=Path("/tmp/email-archive-work"))
    parser.add_argument("--config", type=Path,
                        help="private JSON config containing export source paths")
    args = parser.parse_args()
    workdir = args.workdir
    if (workdir.parent / "export.hold").exists():
        raise SystemExit("Export held: archive layout revision pending; completed index retained")
    config_path = args.config or (workdir / "config.json")
    recipient = json.loads(AGE_POLICY.read_text(encoding="utf-8"))["recipient"]
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8]
    run_root = f"{REMOTE_ROOT}/{run_id}"
    result = export(config_path, workdir, recipient, run_root)
    print(json.dumps({"run_id": run_id, "remote_root": run_root, **result}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
