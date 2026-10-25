#!/usr/bin/env python3
"""Build a private, deduplicated metadata index for retained mail archives.

The generated SQLite database and JSONL manifest contain sensitive mail
metadata. They are created in a caller-supplied private working directory and
must only be published inside an encrypted archive.
"""

from __future__ import annotations

import argparse
import email
import email.policy
import email.utils
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from datetime import timezone
from pathlib import Path
from typing import BinaryIO, Iterator


PRIORITY: tuple[str, ...] = ()

HEADER_PREFIX = re.compile(rb"^[!-9;-~]+:")
SKIP_SUFFIXES = (".msf", ".dat", ".sqlite", ".json", ".js", ".xml", ".html")


def clean_folder(source: Path, member: str) -> str:
    """Keep the source hierarchy while normalizing a mailbox label."""
    value = member.replace("\\", "/")
    for suffix in (".mbox", ".mbx"):
        if value.lower().endswith(suffix):
            value = value[: -len(suffix)]
    parts = [p for p in value.split("/") if p and p not in (".", "..")]
    # Remove generic archive-root directories but retain account/folder names.
    while parts and parts[0].lower() in {"mail", "imapmail", "local folders", "takeout"}:
        parts.pop(0)
    return "/".join(parts) or source.stem


def looks_like_message(prefix: bytes) -> bool:
    lines = prefix.splitlines()[:32]
    return bool(lines and any(HEADER_PREFIX.match(line) for line in lines[:16]))


class PrefixReader:
    """Replay a short sniffed prefix before continuing a non-seekable stream."""

    def __init__(self, prefix: bytes, stream: BinaryIO):
        self.prefix = bytearray(prefix)
        self.stream = stream

    def read(self, size: int = -1) -> bytes:
        if size is None or size < 0:
            prefix = bytes(self.prefix)
            self.prefix.clear()
            return prefix + self.stream.read()
        if not size:
            return b""
        take = min(size, len(self.prefix))
        result = bytes(self.prefix[:take])
        del self.prefix[:take]
        if take < size:
            result += self.stream.read(size - take)
        return result

    def readline(self, size: int = -1) -> bytes:
        if size == 0:
            return b""
        newline = self.prefix.find(b"\n")
        if newline >= 0 and (size < 0 or newline + 1 <= size):
            end = newline + 1
            result = bytes(self.prefix[:end])
            del self.prefix[:end]
            return result
        prefix = bytes(self.prefix)
        self.prefix.clear()
        if size >= 0:
            prefix = prefix[:size]
            if len(prefix) == size:
                return prefix
            tail = self.stream.readline(size - len(prefix))
        else:
            tail = self.stream.readline()
        return prefix + tail


def iter_message_blobs(stream: BinaryIO) -> Iterator[tuple[int, bytes]]:
    """Yield (uncompressed offset, source-serialized RFC message bytes).

    Mbox envelope lines are excluded. Bytes inside each record are retained
    exactly as serialized in the source mailbox, including mboxrd quoting.
    A spooled temporary file bounds memory for unusually large messages.
    """
    first_offset = 0
    first = stream.readline()
    if not first:
        return
    if not first.startswith(b"From "):
        prefix = first
        if not looks_like_message(prefix):
            return
        with tempfile.SpooledTemporaryFile(max_size=2 * 1024 * 1024) as current:
            current.write(first)
            while True:
                block = stream.read(1024 * 1024)
                if not block:
                    break
                current.write(block)
            current.seek(0)
            yield 0, current.read()
        return

    offset = len(first)
    current_offset = offset
    current = tempfile.SpooledTemporaryFile(max_size=2 * 1024 * 1024)
    while True:
        line = stream.readline()
        if not line:
            current.seek(0, os.SEEK_END)
            message_size = current.tell()
            current.seek(0)
            if not message_size:
                current.close()
            else:
                yield current_offset, current.read()
                current.close()
            break
        if line.startswith(b"From "):
            current.seek(0, os.SEEK_END)
            if current.tell():
                current.seek(0)
                yield current_offset, current.read()
                current.close()
            else:
                current.close()
            current_offset = offset + len(line)
            current = tempfile.SpooledTemporaryFile(max_size=2 * 1024 * 1024)
        else:
            current.write(line)
        offset += len(line)


def iter_archive_members(path: Path) -> Iterator[tuple[str, BinaryIO]]:
    suffix = path.name.lower()
    if suffix.endswith((".tar.gz", ".tgz", ".tar")):
        with tarfile.open(path, mode="r|" if suffix.endswith(".tar") else "r|gz") as archive:
            for item in archive:
                if not item.isfile():
                    continue
                handle = archive.extractfile(item)
                if handle is not None:
                    yield item.name, handle
                    handle.close()
    elif suffix.endswith(".zip"):
        with zipfile.ZipFile(path) as archive:
            for item in archive.infolist():
                if item.is_dir():
                    continue
                with archive.open(item, "r") as handle:
                    # The context manager lifetime must include consumption.
                    yield item.filename, handle
    elif suffix.endswith(".7z"):
        listing = subprocess.run(
            ["7z", "l", "-slt", str(path)], check=True, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        ).stdout
        entries: list[str] = []
        current: dict[str, str] = {}
        for line in listing.splitlines() + [""]:
            if not line.strip():
                if current.get("Folder") == "-" and current.get("Path"):
                    entries.append(current["Path"])
                current = {}
            elif " = " in line:
                key, value = line.split(" = ", 1)
                current[key] = value
        for member in entries:
            process = subprocess.Popen(
                ["7z", "x", "-so", str(path), member],
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            )
            assert process.stdout is not None
            yield member, process.stdout
            process.stdout.close()
            if process.wait() != 0:
                raise RuntimeError(f"7z extraction failed for {path.name}")
    else:
        with path.open("rb") as handle:
            yield path.name, handle


def addresses(message: email.message.Message, names: tuple[str, ...]) -> list[str]:
    found = []
    for name in names:
        for _, address in email.utils.getaddresses(message.get_all(name, [])):
            normalized = address.strip().casefold()
            if normalized and normalized not in found:
                found.append(normalized)
    return found


def normalize_date(value: str | None) -> str | None:
    if not value:
        return None
    try:
        parsed = email.utils.parsedate_to_datetime(value)
        if parsed is None:
            return None
        if parsed.tzinfo is None:
            return None
        return parsed.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")
    except (TypeError, ValueError, OverflowError):
        return None


def attribution(message: email.message.Message) -> tuple[str | None, str, float, list[str]]:
    relevant = addresses(message, ("From", "Sender", "To", "Cc", "Bcc"))
    selected = next((candidate for candidate in PRIORITY if candidate in relevant), None)
    if selected is None:
        return None, "unresolved_no_priority_address_in_headers", 0.0, relevant
    collisions = [candidate for candidate in PRIORITY if candidate in relevant]
    confidence = 0.99 if len(collisions) == 1 else 0.95
    reason = "header_address_priority" if len(collisions) == 1 else "priority_resolved_header_collision"
    return selected, reason, confidence, relevant


def decoded_part_bytes(part: email.message.Message) -> bytes:
    if part.get_content_type().casefold() == "message/rfc822":
        return part.as_bytes(policy=email.policy.SMTP)
    payload = part.get_payload(decode=True)
    if payload is not None:
        return payload
    value = part.get_payload()
    if isinstance(value, str):
        try:
            return value.encode(part.get_content_charset() or "utf-8", "replace")
        except LookupError:
            return value.encode("utf-8", "replace")
    return b""


def attachment_details(message: email.message.Message) -> tuple[list[dict[str, object]], int, int, int]:
    attachments: list[dict[str, object]] = []
    text_bytes = 0
    inline_images = 0
    part_index = 0
    for part in message.walk():
        if part.is_multipart() and part.get_content_type().casefold() != "message/rfc822":
            continue
        payload = decoded_part_bytes(part)
        disposition = (part.get_content_disposition() or "").casefold()
        filename = part.get_filename()
        mime_type = part.get_content_type().casefold()
        content_id = part.get("Content-ID")
        is_inline_image = mime_type.startswith("image/") and bool(content_id)
        is_inline = disposition == "inline" or is_inline_image
        is_attachment = disposition == "attachment" or bool(filename)
        if is_inline_image:
            inline_images += 1
        if is_attachment and not is_inline:
            part_index += 1
            attachments.append({
                "part_index": part_index,
                "filename": str(filename) if filename else None,
                "mime_type": mime_type,
                "content_disposition": disposition or None,
                "content_id": str(content_id) if content_id else None,
                "is_inline": False,
                "byte_size": len(payload),
                "sha256": hashlib.sha256(payload).hexdigest(),
            })
        elif mime_type in ("text/plain", "text/html"):
            text_bytes += len(payload)
    return attachments, text_bytes, inline_images, sum(int(item["byte_size"]) for item in attachments)


def parse_thunderbird_prefs(db: sqlite3.Connection, source: Path, member: str,
                            stream: BinaryIO) -> None:
    data = stream.read().decode("utf-8", "replace")
    values: dict[str, object] = {}
    preference = re.compile(r'user_pref\(("(?:\\.|[^"\\])*")\s*,\s*("(?:\\.|[^"\\])*"|true|false|-?\d+(?:\.\d+)?)\);')
    for line in data.splitlines():
        match = preference.match(line)
        if not match:
            continue
        try:
            key = json.loads(match.group(1))
            raw_value = match.group(2)
            value = json.loads(raw_value) if raw_value.startswith('"') else raw_value
            values[str(key)] = value
        except (json.JSONDecodeError, TypeError):
            continue
    account_ids = str(values.get("mail.accountmanager.accounts", "")).split(",")
    for account_id in filter(None, account_ids):
        server_id = values.get(f"mail.account.{account_id}.server")
        identities = str(values.get(f"mail.account.{account_id}.identities", "")).split(",")
        for identity_id in filter(None, identities):
            identity_address = str(values.get(f"mail.identity.{identity_id}.useremail", "")).strip().casefold() or None
            reply_to = str(values.get(f"mail.identity.{identity_id}.reply_to", "")).strip() or None
            full_name = str(values.get(f"mail.identity.{identity_id}.fullName", "")).strip() or None
            smtp_server_id = str(values.get(f"mail.identity.{identity_id}.smtpServer", "")).strip() or None
            db.execute("""
                INSERT OR IGNORE INTO thunderbird_account_config(
                    source_archive,prefs_path,account_id,server_id,server_type,hostname,
                    username,mailbox_path,mailbox_path_relative,smtp_server_id,smtp_hostname,
                    smtp_username,identity_address,reply_to,full_name
                ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
            """, (
                str(source), member, account_id, str(server_id) if server_id else None,
                str(values.get(f"mail.server.{server_id}.type", "")) or None if server_id else None,
                str(values.get(f"mail.server.{server_id}.hostname", "")) or None if server_id else None,
                str(values.get(f"mail.server.{server_id}.userName", "")) or None if server_id else None,
                str(values.get(f"mail.server.{server_id}.directory", "")) or None if server_id else None,
                str(values.get(f"mail.server.{server_id}.directory-rel", "")) or None if server_id else None,
                smtp_server_id,
                str(values.get(f"mail.smtpserver.{smtp_server_id}.hostname", "")) or None if smtp_server_id else None,
                str(values.get(f"mail.smtpserver.{smtp_server_id}.username", "")) or None if smtp_server_id else None,
                identity_address, reply_to, full_name,
            ))
            if identity_address and identity_address not in PRIORITY:
                db.execute("""
                    INSERT OR IGNORE INTO aliases(address,canonical_account,provenance,confidence)
                    VALUES(?,?,?,?)
                """, (identity_address, "unresolved", f"Thunderbird identity in {member}", 0.0))


def create_database(path: Path) -> sqlite3.Connection:
    db = sqlite3.connect(path)
    db.execute("PRAGMA journal_mode=DELETE")
    db.execute("PRAGMA synchronous=FULL")
    db.executescript("""
        CREATE TABLE accounts (
            address TEXT PRIMARY KEY,
            priority INTEGER NOT NULL,
            kind TEXT NOT NULL
        );
        CREATE TABLE aliases (
            address TEXT PRIMARY KEY,
            canonical_account TEXT NOT NULL REFERENCES accounts(address),
            provenance TEXT NOT NULL,
            confidence REAL NOT NULL
        );
        CREATE TABLE messages (
            sha256 TEXT PRIMARY KEY,
            byte_size INTEGER NOT NULL,
            subject TEXT,
            original_date TEXT,
            normalized_utc TEXT,
            from_header TEXT,
            to_header TEXT,
            cc_header TEXT,
            bcc_header TEXT,
            sender_header TEXT,
            from_addresses TEXT,
            to_addresses TEXT,
            cc_addresses TEXT,
            bcc_addresses TEXT,
            sender_addresses TEXT,
            message_id TEXT,
            has_body INTEGER NOT NULL,
            text_body_bytes INTEGER NOT NULL,
            attachment_count INTEGER NOT NULL,
            attachment_bytes INTEGER NOT NULL,
            inline_image_count INTEGER NOT NULL,
            account TEXT,
            attribution_reason TEXT NOT NULL,
            attribution_confidence REAL NOT NULL
        );
        CREATE INDEX messages_date_idx ON messages(normalized_utc);
        CREATE INDEX messages_subject_idx ON messages(subject);
        CREATE INDEX messages_message_id_idx ON messages(message_id);
        CREATE INDEX messages_account_idx ON messages(account);
        CREATE TABLE memberships (
            id INTEGER PRIMARY KEY,
            message_sha256 TEXT NOT NULL REFERENCES messages(sha256),
            source_archive TEXT NOT NULL,
            original_mailbox_path TEXT NOT NULL,
            locator TEXT NOT NULL,
            mailbox_offset INTEGER NOT NULL,
            message_ordinal INTEGER NOT NULL,
            UNIQUE(message_sha256, source_archive, original_mailbox_path, locator)
        );
        CREATE INDEX memberships_hash_idx ON memberships(message_sha256);
        CREATE INDEX memberships_source_idx ON memberships(source_archive, original_mailbox_path);
        CREATE TABLE attachments (
            sha256 TEXT PRIMARY KEY,
            byte_size INTEGER NOT NULL,
            mime_type TEXT,
            occurrence_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE attachment_occurrences (
            message_sha256 TEXT NOT NULL REFERENCES messages(sha256),
            part_index INTEGER NOT NULL,
            attachment_sha256 TEXT NOT NULL REFERENCES attachments(sha256),
            filename TEXT,
            mime_type TEXT NOT NULL,
            content_disposition TEXT,
            content_id TEXT,
            is_inline INTEGER NOT NULL,
            PRIMARY KEY(message_sha256,part_index)
        );
        CREATE INDEX attachment_occurrence_hash_idx ON attachment_occurrences(attachment_sha256);
        CREATE TABLE thunderbird_account_config (
            source_archive TEXT NOT NULL,
            prefs_path TEXT NOT NULL,
            account_id TEXT NOT NULL,
            server_id TEXT,
            server_type TEXT,
            hostname TEXT,
            username TEXT,
            mailbox_path TEXT,
            mailbox_path_relative TEXT,
            smtp_server_id TEXT,
            smtp_hostname TEXT,
            smtp_username TEXT,
            identity_address TEXT,
            reply_to TEXT,
            full_name TEXT,
            PRIMARY KEY(source_archive,prefs_path,account_id,identity_address)
        );
        CREATE VIRTUAL TABLE message_search USING fts5(subject, addresses);
        CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
            INSERT INTO message_search(rowid, subject, addresses)
            VALUES (new.rowid, coalesce(new.subject,''),
                    coalesce(new.from_addresses,'') || ' ' || coalesce(new.to_addresses,'') || ' ' ||
                    coalesce(new.cc_addresses,'') || ' ' || coalesce(new.bcc_addresses,'') || ' ' ||
                    coalesce(new.sender_addresses,''));
        END;
    """)
    for position, address in enumerate(PRIORITY, start=1):
        db.execute("INSERT INTO accounts(address,priority,kind) VALUES(?,?,?)", (address, position, "canonical"))
        db.execute("INSERT INTO aliases(address,canonical_account,provenance,confidence) VALUES(?,?,?,?)",
                   (address, address, "user-confirmed canonical account", 1.0))
    db.execute("INSERT INTO accounts(address,priority,kind) VALUES(?,?,?)", ("unresolved", 999, "unresolved"))
    return db


def process_message(db: sqlite3.Connection, source: Path, member: str, offset: int,
                    ordinal: int, raw: bytes, locator: str) -> bool:
    if not raw:
        return False
    message = email.message_from_bytes(raw, policy=email.policy.default)
    digest = hashlib.sha256(raw).hexdigest()
    account, reason, confidence, _ = attribution(message)
    from_values = addresses(message, ("From",))
    sender_values = addresses(message, ("Sender",))
    to_values = addresses(message, ("To",))
    cc_values = addresses(message, ("Cc",))
    bcc_values = addresses(message, ("Bcc",))
    original_date = message.get("Date")
    is_new_message = db.execute("SELECT 1 FROM messages WHERE sha256=?", (digest,)).fetchone() is None
    if is_new_message:
        attachments, text_body_bytes, inline_image_count, attachment_bytes = attachment_details(message)
    else:
        attachments, text_body_bytes, inline_image_count, attachment_bytes = [], 0, 0, 0
    has_body = bool(text_body_bytes or inline_image_count)
    inserted = db.execute("""
        INSERT OR IGNORE INTO messages(
            sha256,byte_size,subject,original_date,normalized_utc,
            from_header,to_header,cc_header,bcc_header,sender_header,
            from_addresses,to_addresses,cc_addresses,bcc_addresses,sender_addresses,message_id,
            has_body,text_body_bytes,attachment_count,attachment_bytes,inline_image_count,
            account,attribution_reason,attribution_confidence
        ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
    """, (
        digest, len(raw), str(message.get("Subject", "")) or None,
        str(original_date) if original_date else None, normalize_date(str(original_date) if original_date else None),
        str(message.get("From", "")) or None, str(message.get("To", "")) or None,
        str(message.get("Cc", "")) or None, str(message.get("Bcc", "")) or None,
        str(message.get("Sender", "")) or None,
        json.dumps(from_values, ensure_ascii=False), json.dumps(to_values, ensure_ascii=False),
        json.dumps(cc_values, ensure_ascii=False), json.dumps(bcc_values, ensure_ascii=False),
        json.dumps(sender_values, ensure_ascii=False), str(message.get("Message-ID", "")) or None,
        int(has_body), text_body_bytes, len(attachments), attachment_bytes, inline_image_count,
        account, reason, confidence,
    )).rowcount == 1
    if inserted:
        for item in attachments:
            attachment_hash = str(item["sha256"])
            db.execute("""
                INSERT INTO attachments(sha256,byte_size,mime_type,occurrence_count)
                VALUES(?,?,?,1)
                ON CONFLICT(sha256) DO UPDATE SET occurrence_count=occurrence_count+1
            """, (attachment_hash, item["byte_size"], item["mime_type"]))
            db.execute("""
                INSERT INTO attachment_occurrences(
                    message_sha256,part_index,attachment_sha256,filename,mime_type,
                    content_disposition,content_id,is_inline
                ) VALUES(?,?,?,?,?,?,?,?)
            """, (digest, item["part_index"], attachment_hash, item["filename"],
                  item["mime_type"], item["content_disposition"], item["content_id"],
                  int(bool(item["is_inline"]))))
    source_name = str(source)
    folder = clean_folder(source, member)
    locator_obj = {"member": member, "offset": offset, "ordinal": ordinal, "locator": locator}
    db.execute("""
        INSERT OR IGNORE INTO memberships(
            message_sha256,source_archive,original_mailbox_path,locator,mailbox_offset,message_ordinal
        ) VALUES(?,?,?,?,?,?)
    """, (digest, source_name, folder, json.dumps(locator_obj, ensure_ascii=False), offset, ordinal))
    return True


def skip_member(name: str) -> bool:
    lower = name.lower()
    return lower.endswith(SKIP_SUFFIXES) or "/.sbd/" in lower or lower.endswith(".sqlite-wal")


def scan(db: sqlite3.Connection, sources: list[Path]) -> tuple[int, int]:
    memberships = 0
    members_scanned = 0
    for source in sources:
        if not source.is_file():
            raise FileNotFoundError(source)
        for member, stream in iter_archive_members(source):
            members_scanned += 1
            if Path(member).name.casefold() == "prefs.js":
                parse_thunderbird_prefs(db, source, member, stream)
                stream.close()
                continue
            if skip_member(member):
                continue
            sniff = stream.read(512)
            try:
                can_seek = stream.seekable()
            except (AttributeError, OSError):
                can_seek = False
            if can_seek:
                stream.seek(0)
            # Replay only the sniff prefix for sequential tar/7z members.
            if can_seek:
                replay: BinaryIO = stream
            else:
                replay = PrefixReader(sniff, stream)  # type: ignore[assignment]
            if sniff.startswith(b"From ") or looks_like_message(sniff):
                folder = clean_folder(source, member)
                ordinal = 0
                for offset, raw in iter_message_blobs(replay):
                    ordinal += 1
                    locator = f"{member}#offset={offset}&ordinal={ordinal}"
                    memberships += process_message(db, source, member, offset, ordinal, raw, locator)
            if replay is not stream and hasattr(replay, "close"):
                replay.close()
            if hasattr(stream, "close"):
                stream.close()
            if members_scanned % 1000 == 0:
                db.commit()
                print(f"scanned archive members={members_scanned} message memberships={memberships}", file=sys.stderr, flush=True)
        db.commit()
    return members_scanned, memberships


def export_manifest(db: sqlite3.Connection, path: Path) -> int:
    count = 0
    with path.open("w", encoding="utf-8", newline="\n") as output:
        for row in db.execute("""
            SELECT m.sha256,m.byte_size,m.subject,m.original_date,m.normalized_utc,
                   m.from_header,m.to_header,m.cc_header,m.bcc_header,m.sender_header,
                   m.from_addresses,m.to_addresses,m.cc_addresses,m.bcc_addresses,
                   m.sender_addresses,m.message_id,m.has_body,m.text_body_bytes,
                   m.attachment_count,m.attachment_bytes,m.inline_image_count,
                   m.account,m.attribution_reason,
                   m.attribution_confidence, x.source_archive,x.original_mailbox_path,
                   x.locator,x.mailbox_offset,x.message_ordinal
            FROM memberships x JOIN messages m ON m.sha256=x.message_sha256
            ORDER BY m.account,x.source_archive,x.original_mailbox_path,x.message_ordinal
        """):
            (sha, size, subject, original_date, normalized_utc, from_header,
             to_header, cc_header, bcc_header, sender_header, from_v, to_v, cc_v,
             bcc_v, sender_v, message_id, has_body, text_bytes, attachment_count,
             attachment_bytes, inline_count, account, reason, confidence, source,
             mailbox_path, locator, offset, ordinal) = row
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
                "message_locator": json.loads(locator),
                "byte_size": size,
                "sha256": sha,
                "has_body": bool(has_body),
                "text_body_bytes": text_bytes,
                "attachment_count": attachment_count,
                "attachment_bytes": attachment_bytes,
                "inline_image_count": inline_count,
            }
            output.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
            count += 1
        for row in db.execute("""
            SELECT o.message_sha256,o.part_index,o.attachment_sha256,o.filename,
                   o.mime_type,o.content_disposition,o.content_id,o.is_inline,
                   a.byte_size
            FROM attachment_occurrences o JOIN attachments a ON a.sha256=o.attachment_sha256
            ORDER BY o.message_sha256,o.part_index
        """):
            (message_hash, part_index, attachment_hash, filename, mime_type,
             disposition, content_id, is_inline, size) = row
            record = {
                "record_type": "attachment",
                "message_sha256": message_hash,
                "part_index": part_index,
                "attachment_sha256": attachment_hash,
                "filename": filename,
                "mime_type": mime_type,
                "content_disposition": disposition,
                "content_id": content_id,
                "is_inline": bool(is_inline),
                "byte_size": size,
            }
            output.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
            count += 1
    os.chmod(path, 0o600)
    return count


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--workdir", type=Path, required=True,
                        help="private local working directory; chmod 0700 is enforced")
    parser.add_argument("--config", type=Path, required=True,
                        help="private JSON config containing the user-confirmed account priority list")
    parser.add_argument("--sources", nargs="*", type=Path,
                        help="optional source override; otherwise use private config source_paths")
    args = parser.parse_args()
    os.umask(0o077)
    global PRIORITY
    configuration = json.loads(args.config.read_text(encoding="utf-8"))
    PRIORITY = tuple(str(value).strip().casefold() for value in configuration["priority_addresses"])
    sources = args.sources if args.sources is not None else [Path(value) for value in configuration["source_paths"]]
    if not PRIORITY or len(set(PRIORITY)) != len(PRIORITY):
        parser.error("config priority_addresses must be non-empty and unique")
    if not sources:
        parser.error("config source_paths must be non-empty")
    args.workdir.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(args.workdir, 0o700)
    db_path = args.workdir / "email-index.sqlite"
    manifest_path = args.workdir / "manifest.jsonl"
    for path in (db_path, manifest_path):
        path.unlink(missing_ok=True)
    db = create_database(db_path)
    os.chmod(db_path, 0o600)
    try:
        members, memberships = scan(db, sources)
        unique_messages = db.execute("SELECT count(*) FROM messages").fetchone()[0]
        unique_memberships = db.execute("SELECT count(*) FROM memberships").fetchone()[0]
        db.execute("ANALYZE")
        db.commit()
        manifest_count = export_manifest(db, manifest_path)
        coverage = {}
        for position, address in enumerate(PRIORITY, start=1):
            coverage[f"account_{position:02d}"] = db.execute(
                "SELECT count(*) FROM messages WHERE account=?", (address,)
            ).fetchone()[0]
        coverage["unresolved"] = db.execute(
            "SELECT count(*) FROM messages WHERE account IS NULL"
        ).fetchone()[0]
        print(json.dumps({
            "archive_members_scanned": members,
            "message_memberships_seen": memberships,
            "unique_raw_messages": unique_messages,
            "source_memberships_indexed": unique_memberships,
            "jsonl_rows": manifest_count,
            "unique_message_account_coverage": coverage,
            "unresolved_unique_messages": db.execute("SELECT count(*) FROM messages WHERE account IS NULL").fetchone()[0],
        }, indent=2))
    finally:
        db.close()
    os.chmod(db_path, 0o600)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
