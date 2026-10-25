#!/usr/bin/env python3
"""Immutable, independently recoverable account/year mail archive generations.

Private SQLite is the job ledger. Objects become visible to it only after local
decryption validation and remote ciphertext readback. A catalogue is published
last. Source files and the completed input index are always read-only.
"""
from __future__ import annotations

import argparse
from collections import OrderedDict
from contextlib import contextmanager
from datetime import datetime, timezone
import email
import email.policy
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sqlite3
import subprocess
import tarfile
import time
import uuid

import email_archive_index as index
import email_archive_export as views
from email_archive_run import save_json, source_state

PART_BYTES = 128 * 1024 * 1024
ROOT = "type-drive:private/archives/mail/structured"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def file_hash(path):
    with Path(path).open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def year_for(original_date, provider_utc=None):
    normalized = index.normalize_date(original_date)
    if normalized:
        return normalized[:4], "header_date_utc"
    if provider_utc:
        try:
            date = datetime.fromisoformat(provider_utc.replace("Z", "+00:00"))
            if date.tzinfo is not None:
                return str(date.astimezone(timezone.utc).year).zfill(4), "provider_timestamp_utc"
        except (ValueError, OverflowError):
            pass
    return "unknown-date", "unknown"


def private_directory(path):
    path = Path(path)
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(path, 0o700)
    return path


def connect(path, readonly=False):
    db = sqlite3.connect(f"file:{path}?mode=ro" if readonly else str(path), uri=readonly)
    db.row_factory = sqlite3.Row
    return db


def safe_object_path(value):
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or not path.parts or ":" in value:
        raise ValueError("invalid archive object path")
    return str(path)


@contextmanager
def decrypted(path, identity):
    age = subprocess.Popen(["age", "-d", "-i", str(identity), str(path)],
                           stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    zstd = subprocess.Popen(["zstd", "-d", "-c"], stdin=age.stdout,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    age.stdout.close()
    try:
        yield zstd.stdout
        # Complete both streams so authentication failures cannot be hidden by
        # tar's early EOF or fetching only the first member.
        while zstd.stdout.read(1024 * 1024):
            pass
        if zstd.wait() or age.wait():
            raise RuntimeError("decryption/decompression failed")
    finally:
        zstd.stdout.close()
        for process in (zstd, age):
            if process.poll() is None:
                process.terminate()
            process.wait()


class Store:
    def __init__(self, workdir, recipient, identity, remote=ROOT, drive_config=None):
        self.workdir = private_directory(workdir)
        self.cache = private_directory(self.workdir / "ciphertext")
        self.recipient = recipient
        self.identity = Path(identity).expanduser()
        self.remote = remote.rstrip("/")
        self.drive_config = str(Path(drive_config or "~/.config/private-storage/drive.conf").expanduser())
        if not self.identity.is_file():
            raise ValueError("recoverable age identity is unavailable")

    def rclone(self, *args, **kwargs):
        return subprocess.run(["rclone", "--config", self.drive_config, *args],
                              check=True, stderr=subprocess.DEVNULL, **kwargs)

    def remote_hash(self, key):
        process = subprocess.Popen(["rclone", "--config", self.drive_config, "cat",
                                    self.remote + "/" + safe_object_path(key)],
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        try:
            digest = hashlib.file_digest(process.stdout, "sha256").hexdigest()
        finally:
            process.stdout.close()
        if process.wait():
            raise RuntimeError("remote readback failed")
        return digest

    def upload(self, path, key):
        key = safe_object_path(key)
        digest = file_hash(path)
        self.rclone("copyto", "--immutable", str(path), self.remote + "/" + key,
                    stdout=subprocess.DEVNULL)
        if self.remote_hash(key) != digest:
            raise RuntimeError("remote ciphertext checksum mismatch")
        return digest

    def fetch(self, obj):
        key = safe_object_path(obj["path"])
        path = self.cache / (sha(key.encode()) + ".age")
        if not path.exists() or file_hash(path) != obj["cipher_sha256"]:
            temp = path.with_suffix(".partial")
            self.rclone("copyto", self.remote + "/" + key, str(temp),
                        stdout=subprocess.DEVNULL)
            if file_hash(temp) != obj["cipher_sha256"]:
                raise RuntimeError("download checksum mismatch")
            temp.replace(path)
        return path


class Part:
    def __init__(self, engine, account, year, kind, folder=""):
        self.engine = engine
        self.account, self.year, self.kind, self.folder = account, year, kind, folder
        self.id = uuid.uuid4().hex
        suffix = "mbox" if kind == "reader" else "tar"
        folder_component = "/" + sha(folder.encode())[:24] if folder else ""
        self.key = (f"{year}/{sha(account.encode())[:24]}/{engine.generation}/"
                    f"{kind}{folder_component}/{self.id}.{suffix}.zst.age")
        self.path = engine.store.cache / (self.id + ".partial")
        if shutil.disk_usage(self.path.parent).free < max(2 * PART_BYTES, engine.part_bytes * 2):
            raise RuntimeError("insufficient ciphertext staging space")
        self.file = self.path.open("xb")
        self.age = subprocess.Popen(["age", "-r", engine.store.recipient], stdin=subprocess.PIPE,
                                    stdout=self.file, stderr=subprocess.DEVNULL)
        self.zstd = subprocess.Popen(["zstd", "-T1", "-3", "-c"], stdin=subprocess.PIPE,
                                     stdout=self.age.stdin, stderr=subprocess.DEVNULL)
        self.age.stdin.close()
        self.stream = self.zstd.stdin
        self.tar = None if kind == "reader" else tarfile.open(fileobj=self.stream, mode="w|", format=tarfile.USTAR_FORMAT)
        self.size = 0
        self.records = []
        self.closed = False

    def add(self, digest, content):
        if self.tar is not None:
            member = digest + (".eml" if self.kind == "raw" else ".bin")
            offset = self.tar.offset + 512
            item = tarfile.TarInfo(member)
            item.size, item.mode, item.mtime = len(content), 0o600, 0
            self.tar.addfile(item, io.BytesIO(content))
        else:
            member, offset = "", self.size
            self.stream.write(content)
        self.records.append((digest, member, offset, len(content), sha(content)))
        self.size += len(content)

    def abort(self):
        if self.closed:
            return
        for process in (self.zstd, self.age):
            if process.poll() is None:
                process.terminate()
            process.wait()
        self.file.close()
        self.closed = True

    def seal(self):
        if self.tar:
            self.tar.close()
        self.stream.close()
        zstatus, astatus = self.zstd.wait(), self.age.wait()
        self.file.flush()
        os.fsync(self.file.fileno())
        self.file.close()
        self.closed = True
        if zstatus or astatus:
            raise RuntimeError("encrypted part creation failed")
        self.engine.status("validating", part_messages=len(self.records))
        with decrypted(self.path, self.engine.store.identity) as stream:
            if self.tar is not None:
                with tarfile.open(fileobj=stream, mode="r|") as archive:
                    seen = 0
                    for item, record in zip(archive, self.records, strict=True):
                        digest, member, offset, size, content_hash = record
                        if (item.name, item.offset_data, item.size) != (member, offset, size):
                            raise RuntimeError("tar locator mismatch")
                        blob = archive.extractfile(item).read()
                        if sha(blob) != content_hash:
                            raise RuntimeError("part content hash mismatch")
                        if self.kind == "raw":
                            email.message_from_bytes(blob, policy=email.policy.default)
                        seen += 1
                    if seen != len(self.records):
                        raise RuntimeError("part message count mismatch")
            else:
                for _, _, _, size, expected in self.records:
                    blob = stream.read(size)
                    if sha(blob) != expected:
                        raise RuntimeError("reader record checksum mismatch")
                    records = list(index.iter_message_blobs(io.BytesIO(blob)))
                    if len(records) != 1:
                        raise RuntimeError("reader MBOX reparsed count mismatch")
                    email.message_from_bytes(records[0][1], policy=email.policy.default)
                if stream.read(1):
                    raise RuntimeError("unexpected trailing MBOX content")
        self.engine.status("uploading", part_bytes=self.path.stat().st_size)
        ciphertext_hash = self.engine.store.upload(self.path, self.key)
        db = self.engine.db
        with db:
            db.execute("INSERT INTO objects VALUES(?,?,?,?,?,?,?,?,?,?)",
                       (self.id, self.key, self.account, self.year, self.kind, self.folder,
                        ciphertext_hash, self.path.stat().st_size, self.generation, len(self.records)))
            db.executemany("INSERT INTO locations VALUES(?,?,?,?,?,?,?,?,?)",
                           [(self.kind, self.account, self.year, self.folder, digest,
                             self.id, member, offset, size) for digest, member, offset, size, _ in self.records])
        if self.kind == "raw":
            self.path.replace(self.engine.store.cache / (self.id + ".age"))
        else:
            self.path.unlink()

    @property
    def generation(self):
        return self.engine.generation


SCHEMA = """
CREATE TABLE IF NOT EXISTS archive_years (
 sha256 TEXT PRIMARY KEY, account TEXT NOT NULL, year TEXT NOT NULL, year_basis TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS archive_partition ON archive_years(account,year);
CREATE TABLE IF NOT EXISTS objects (
 id TEXT PRIMARY KEY,path TEXT UNIQUE,account TEXT,year TEXT,kind TEXT,folder TEXT,
 cipher_sha256 TEXT,cipher_bytes INTEGER,generation TEXT,records INTEGER);
CREATE TABLE IF NOT EXISTS locations (
 kind TEXT,account TEXT,year TEXT,folder TEXT,digest TEXT,object_id TEXT,
 member TEXT,offset INTEGER,length INTEGER,
 PRIMARY KEY(kind,account,year,folder,digest));
CREATE INDEX IF NOT EXISTS locations_digest ON locations(digest,kind);
CREATE TABLE IF NOT EXISTS source_checkpoints (source TEXT PRIMARY KEY,size INTEGER,mtime_ns INTEGER);
CREATE TABLE IF NOT EXISTS reader_decisions (
 sha256 TEXT,folder TEXT,included INTEGER,reason TEXT,output_folder TEXT,PRIMARY KEY(sha256,folder));
CREATE TABLE IF NOT EXISTS provider_date_evidence (
 sha256 TEXT,provider_utc TEXT,source_identity TEXT,
 PRIMARY KEY(sha256,provider_utc,source_identity));
CREATE TABLE IF NOT EXISTS generation_policy (
 generation TEXT PRIMARY KEY,policy_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS archive_settings (key TEXT PRIMARY KEY,value TEXT NOT NULL);
"""


class Engine:
    def __init__(self, store, db, generation, part_bytes=PART_BYTES):
        self.store, self.db, self.generation = store, db, generation
        self.part_bytes = part_bytes
        self.writers = OrderedDict()
        self.pending = set()
        self.started = time.time()
        self.processed = 0
        self.last_status = 0

    def status(self, stage, **values):
        now = time.time()
        if stage == "scanning" and now - self.last_status < 5:
            return
        self.last_status = now
        save_json(self.store.workdir / "status.json", {
            "version": 1, "stage": stage, "generation": self.generation,
            "processed_occurrences": self.processed, "elapsed_seconds": int(now - self.started),
            "updated_utc": datetime.now(timezone.utc).isoformat(), **values})

    def put(self, kind, account, year, digest, content, folder=""):
        key = (kind, account, year, folder)
        locator = (*key, digest)
        if locator in self.pending or self.db.execute(
                "SELECT 1 FROM locations WHERE kind=? AND account=? AND year=? AND folder=? AND digest=?",
                locator).fetchone():
            return
        writer = self.writers.get(key)
        if writer and writer.size and writer.size + len(content) > self.part_bytes:
            self.close(key)
            writer = None
        if writer is None:
            if len(self.writers) >= 32:
                self.close(next(iter(self.writers)))
            writer = self.writers[key] = Part(self, account, year, kind, folder)
        self.writers.move_to_end(key)
        writer.add(digest, content)
        self.pending.add(locator)

    def close(self, key):
        writer = self.writers[key]
        writer.seal()
        for digest, *_ in writer.records:
            self.pending.discard((*key, digest))
        del self.writers[key]

    def flush(self):
        for key in list(self.writers):
            self.close(key)

    def abort(self):
        for writer in self.writers.values():
            writer.abort()


def reader_folders(db, digest):
    folders = {r[0] for r in db.execute(
        "SELECT original_mailbox_path FROM memberships WHERE message_sha256=?", (digest,))}
    # Flag-only memberships never suppress the residual Archive view.
    flags = {"unread", "starred", "important"}
    ordinary = {f for f in folders if not views.is_all_mail(f)
                and f.casefold().split("/")[-1] not in flags}
    result = set()
    for folder in sorted(folders):
        is_all = views.is_all_mail(folder)
        included = not is_all or not ordinary
        db.execute("INSERT OR REPLACE INTO reader_decisions VALUES(?,?,?,?,?)",
                   (digest, folder, int(included), "residual_all_mail" if is_all else "source_membership", views.mapped_folder(folder)))
        if is_all and not included:
            # Later provenance can move a message out of the residual view.
            # The old encrypted object stays intact; current locators select
            # only the records belonging to this catalogue generation.
            db.execute("DELETE FROM locations WHERE kind='reader' AND digest=? AND folder='Gmail Archive'", (digest,))
        if included:
            result.add(views.mapped_folder(folder))
    return sorted(result)


def prepare_ledger(workdir, source_db, base_ledger=None):
    target = workdir / "ledger.sqlite"
    if not target.exists():
        temp = target.with_suffix(".partial")
        temp.unlink(missing_ok=True)
        with connect(base_ledger or source_db, True) as source, connect(temp) as dest:
            source.backup(dest)
        temp.replace(target)
    db = connect(target)
    db.executescript(SCHEMA)
    db.execute("PRAGMA synchronous=FULL")
    if base_ledger and not (workdir / "merged.json").exists():
        with connect(source_db, True) as incoming:
            expected = [tuple(r) for r in db.execute("SELECT * FROM accounts ORDER BY priority")]
            actual = [tuple(r) for r in incoming.execute("SELECT * FROM accounts ORDER BY priority")]
            if expected != actual:
                raise RuntimeError("canonical account priority changed between generations")
            with db:
                for table in ("messages", "attachments", "attachment_occurrences", "thunderbird_account_config", "aliases"):
                    columns = [r[1] for r in incoming.execute(f"PRAGMA table_info({table})")]
                    names = ','.join(columns)
                    placeholders = ','.join('?' for _ in columns)
                    db.executemany(f"INSERT OR IGNORE INTO {table}({names}) VALUES({placeholders})", incoming.execute(f"SELECT {names} FROM {table}"))
                for row in incoming.execute("SELECT message_sha256,source_archive,original_mailbox_path,locator,mailbox_offset,message_ordinal FROM memberships"):
                    changed = db.execute("INSERT OR IGNORE INTO memberships(message_sha256,source_archive,original_mailbox_path,locator,mailbox_offset,message_ordinal) VALUES(?,?,?,?,?,?)", tuple(row)).rowcount
                    if changed and db.execute("SELECT 1 FROM sqlite_master WHERE name='derived_objects'").fetchone():
                        db.execute("DELETE FROM derived_objects WHERE id IN (SELECT object_id FROM locations WHERE kind='raw' AND digest=?)", (row[0],))
        save_json(workdir / "merged.json", {"input": str(source_db), "base": str(base_ledger)})
    with connect(source_db, True) as incoming:
        provider_dates = dict(incoming.execute("SELECT sha256,provider_utc FROM provider_dates")) if incoming.execute("SELECT 1 FROM sqlite_master WHERE name='provider_dates'").fetchone() else {}
        if provider_dates:
            db.executemany("INSERT OR IGNORE INTO provider_date_evidence VALUES(?,?,?)", incoming.execute("SELECT sha256,provider_utc,source_identity FROM provider_dates"))
    for row in db.execute("SELECT sha256,account,original_date FROM messages WHERE sha256 NOT IN (SELECT sha256 FROM archive_years)").fetchall():
        year, basis = year_for(row["original_date"], provider_dates.get(row["sha256"]))
        db.execute("INSERT INTO archive_years VALUES(?,?,?,?)",
                   (row["sha256"], row["account"] or "unresolved", year, basis))
    db.commit()
    return db


def scan_sources(engine, sources):
    db = engine.db
    for source in sources:
        stat = source.stat()
        checkpoint = db.execute("SELECT size,mtime_ns FROM source_checkpoints WHERE source=?", (str(source),)).fetchone()
        if checkpoint:
            if tuple(checkpoint) != (stat.st_size, stat.st_mtime_ns):
                raise RuntimeError("source changed after export checkpoint")
            continue
        for member, stream in index.iter_archive_members(source):
            if index.skip_member(member):
                # Drain sequential extractors, including 7z, before advancing.
                while stream.read(1024 * 1024):
                    pass
                continue
            sniff = stream.read(512)
            if not (sniff.startswith(b"From ") or index.looks_like_message(sniff)):
                while stream.read(1024 * 1024):
                    pass
                continue
            replay = index.PrefixReader(sniff, stream)
            for offset, raw in index.iter_message_blobs(replay):
                digest = sha(raw)
                row = db.execute("SELECT account,year FROM archive_years WHERE sha256=?", (digest,)).fetchone()
                if row is None:
                    raise RuntimeError("source message absent from completed index")
                account, year = row
                engine.put("raw", account, year, digest, raw)
                engine.processed += 1
                engine.status("scanning")
        engine.flush()
        if (source.stat().st_size, source.stat().st_mtime_ns) != (stat.st_size, stat.st_mtime_ns):
            raise RuntimeError("source changed during export")
        with db:
            db.execute("INSERT INTO source_checkpoints VALUES(?,?,?)", (str(source), stat.st_size, stat.st_mtime_ns))


def derive_views(engine):
    """Process one year/account at a time to keep reader writers bounded."""
    db = engine.db
    db.execute("CREATE TABLE IF NOT EXISTS derived_objects (id TEXT PRIMARY KEY)")
    db.commit()
    partitions = db.execute("SELECT DISTINCT account,year FROM archive_years ORDER BY year,account").fetchall()
    for account, year in partitions:
        objects = db.execute("SELECT * FROM objects WHERE kind='raw' AND account=? AND year=? ORDER BY path", (account, year)).fetchall()
        completed = []
        for obj in objects:
            if db.execute("SELECT 1 FROM derived_objects WHERE id=?", (obj["id"],)).fetchone():
                continue
            cached = engine.store.cache / (obj["id"] + ".age")
            path = cached if cached.exists() and file_hash(cached) == obj["cipher_sha256"] else engine.store.fetch(obj)
            with decrypted(path, engine.store.identity) as stream:
                with tarfile.open(fileobj=stream, mode="r|") as archive:
                    for member in archive:
                        raw = archive.extractfile(member).read()
                        digest = sha(raw)
                        if member.name != digest + ".eml":
                            raise RuntimeError("raw object member/hash mismatch")
                        parsed = email.message_from_bytes(raw, policy=email.policy.default)
                        attachments = views.extract_attachments(parsed)
                        for attachment_hash, _, content in attachments:
                            engine.put("attachments", account, year, attachment_hash, content)
                        cleaned = views.reader_view(parsed, attachments)
                        record = views.mbox_record(parsed, cleaned)
                        for folder in reader_folders(db, digest):
                            engine.put("reader", account, year, digest, record, folder)
                        engine.status("reader_views")
            completed.append((obj["id"], path))
        engine.flush()
        with db:
            db.executemany("INSERT INTO derived_objects VALUES(?)", [(ident,) for ident, _ in completed])
        for _, path in completed:
            path.unlink(missing_ok=True)


def encrypt_file(store, source, key):
    plaintext_hash = file_hash(source)
    key = key.removesuffix(".zst.age") + "." + plaintext_hash + ".zst.age"
    receipts = private_directory(store.workdir / "receipts")
    receipt = receipts / (sha(key.encode()) + ".json")
    if receipt.exists():
        return json.loads(receipt.read_text())
    target = store.cache / (sha(key.encode()) + ".age")
    if not target.exists():
        partial = target.with_suffix(".partial")
        with partial.open("wb") as output:
            age = subprocess.Popen(["age", "-r", store.recipient], stdin=subprocess.PIPE,
                                   stdout=output, stderr=subprocess.DEVNULL)
            result = subprocess.run(["zstd", "-T1", "-3", "-c", str(source)],
                                    stdout=age.stdin, stderr=subprocess.DEVNULL)
            age.stdin.close()
            status = age.wait()
            output.flush()
            os.fsync(output.fileno())
        if result.returncode or status:
            raise RuntimeError("metadata encryption failed")
        partial.replace(target)
    with decrypted(target, store.identity) as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != plaintext_hash:
            raise RuntimeError("encrypted metadata validation failed")
    digest = store.upload(target, key)
    result = {"path": key, "cipher_sha256": digest, "cipher_bytes": target.stat().st_size,
              "plaintext_sha256": plaintext_hash}
    save_json(receipt, result)
    target.unlink()
    return result


def partition_index(engine, account, year, target):
    source = engine.db
    target.unlink(missing_ok=True)
    dest = connect(target)
    dest.execute("ATTACH DATABASE ? AS ledger", (str(engine.store.workdir / "ledger.sqlite"),))
    queries = {
        "messages": "SELECT m.* FROM ledger.messages m JOIN ledger.archive_years y ON y.sha256=m.sha256 WHERE y.account=? AND y.year=?",
        "archive_years": "SELECT * FROM ledger.archive_years WHERE account=? AND year=?",
        "memberships": "SELECT x.* FROM ledger.memberships x JOIN messages m ON m.sha256=x.message_sha256",
        "attachment_occurrences": "SELECT x.* FROM ledger.attachment_occurrences x JOIN messages m ON m.sha256=x.message_sha256",
        "attachments": "SELECT * FROM ledger.attachments WHERE sha256 IN (SELECT attachment_sha256 FROM attachment_occurrences)",
        "locations": "SELECT * FROM ledger.locations WHERE account=? AND year=?",
        "objects": "SELECT * FROM ledger.objects WHERE id IN (SELECT object_id FROM locations)",
        "reader_decisions": "SELECT x.* FROM ledger.reader_decisions x JOIN messages m ON m.sha256=x.sha256",
        "accounts": "SELECT * FROM ledger.accounts",
        "aliases": "SELECT * FROM ledger.aliases",
        "thunderbird_account_config": "SELECT * FROM ledger.thunderbird_account_config",
        "provider_date_evidence": "SELECT x.* FROM ledger.provider_date_evidence x JOIN messages m ON m.sha256=x.sha256",
        "generation_policy": "SELECT * FROM ledger.generation_policy",
    }
    for table, query in queries.items():
        dest.execute(f"CREATE TABLE {table} AS {query}", (account, year) if "?" in query else ())
    dest.executescript("""
      CREATE UNIQUE INDEX message_hash ON messages(sha256);
      CREATE INDEX message_date ON messages(normalized_utc);
      CREATE INDEX message_id ON messages(message_id);
      CREATE INDEX locator_hash ON locations(digest,kind);
      CREATE INDEX membership_hash ON memberships(message_sha256);
      CREATE VIRTUAL TABLE message_search USING fts5(sha256 UNINDEXED,subject,addresses);
      INSERT INTO message_search SELECT sha256,subject,
        coalesce(from_header,'')||' '||coalesce(to_header,'')||' '||coalesce(cc_header,'')
        ||' '||coalesce(bcc_header,'')||' '||coalesce(sender_header,'') FROM messages;
    """)
    dest.commit()
    if dest.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
        raise RuntimeError("partition index integrity failed")
    manifest = target.with_suffix(".jsonl")
    with manifest.open("w") as out:
        for table in queries:
            for row in dest.execute(f"SELECT * FROM {table}"):
                out.write(json.dumps({"record_type": table, **dict(row)}, ensure_ascii=True) + "\n")
    count = dest.execute("SELECT count(*) FROM messages").fetchone()[0]
    dest.close()
    return manifest, count


def publish(engine):
    db, store = engine.db, engine.store
    missing = db.execute("SELECT count(*) FROM messages m WHERE NOT EXISTS (SELECT 1 FROM locations l WHERE l.digest=m.sha256 AND l.kind='raw')").fetchone()[0]
    if missing:
        raise RuntimeError(f"raw coverage incomplete: {missing} messages")
    missing_attachments = db.execute("""SELECT count(*) FROM attachment_occurrences a
        JOIN archive_years y ON y.sha256=a.message_sha256
        WHERE NOT EXISTS (SELECT 1 FROM locations l WHERE l.kind='attachments'
          AND l.digest=a.attachment_sha256 AND l.account=y.account AND l.year=y.year)""").fetchone()[0]
    if missing_attachments:
        raise RuntimeError(f"attachment coverage incomplete: {missing_attachments} occurrences")
    missing_views = db.execute("""SELECT count(*) FROM reader_decisions r
        WHERE r.included=1 AND NOT EXISTS (SELECT 1 FROM locations l
          WHERE l.kind='reader' AND l.digest=r.sha256 AND
            l.folder=r.output_folder)""").fetchone()[0]
    if missing_views:
        raise RuntimeError(f"reader coverage incomplete: {missing_views} memberships")
    engine.status("publishing_indexes")
    catalogue = {"version": 2, "generation": engine.generation,
                 "remote": store.remote, "part_target_bytes": engine.part_bytes,
                 "created_utc": datetime.now(timezone.utc).isoformat(), "partitions": []}
    metadata = private_directory(store.workdir / "metadata")
    for account, year in db.execute("SELECT DISTINCT account,year FROM archive_years ORDER BY year,account"):
        slug = sha(account.encode())[:24]
        base = f"{year}/{slug}/{engine.generation}"
        target = metadata / f"{year}-{slug}.sqlite"
        manifest, count = partition_index(engine, account, year, target)
        catalogue["partitions"].append({"account": account, "year": year, "messages": count,
            "index": encrypt_file(store, target, base + "/index.sqlite.zst.age"),
            "manifest": encrypt_file(store, manifest, base + "/manifest.jsonl.zst.age")})
    # Recoverable full ledger carries the raw registry and all provenance for
    # future delta imports. Partition indexes remain independently searchable.
    db.commit()
    catalogue["ledger"] = encrypt_file(store, store.workdir / "ledger.sqlite",
                                       f"catalogues/{engine.generation}.ledger.sqlite.zst.age")
    local = store.workdir / "catalogue.json"
    save_json(local, catalogue)
    pointer = encrypt_file(store, local, f"catalogues/{engine.generation}.json.zst.age")
    save_json(store.workdir / "published.json", {"version": 2, "catalogue": pointer,
        "generation": engine.generation, "messages": db.execute("SELECT count(*) FROM messages").fetchone()[0],
        "objects": db.execute("SELECT count(*) FROM objects").fetchone()[0],
        "status": "verified_local_decryption_and_remote_ciphertext_readback"})
    engine.status("complete", partitions=len(catalogue["partitions"]))


def run(workdir, checkpoint, config_path, policy_path, remote=ROOT, base_ledger=None, wait_checkpoint=False):
    os.umask(0o077)
    workdir = private_directory(workdir)
    with (workdir / "annual.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if wait_checkpoint:
            deadline = time.monotonic() + 24 * 3600
            while not checkpoint.exists():
                save_json(workdir / "status.json", {"stage": "waiting_for_completed_index", "updated_utc": datetime.now(timezone.utc).isoformat()})
                if time.monotonic() > deadline:
                    raise TimeoutError("index checkpoint did not arrive within 24 hours")
                time.sleep(30)
        config = json.loads(config_path.read_text())
        completed = json.loads(checkpoint.read_text())
        if completed["sources"] != source_state(config) or completed["config_sha256"] != file_hash(config_path):
            raise RuntimeError("input checkpoint no longer matches its sources/configuration")
        input_db = Path(completed["data_directory"]) / "email-index.sqlite"
        policy = json.loads(policy_path.read_text())
        state_path = workdir / "job.json"
        identity = {"checkpoint_sha256": file_hash(checkpoint), "remote": remote,
                    "recipient": policy["recipient"],
                    "base_ledger_sha256": file_hash(base_ledger) if base_ledger else None}
        if state_path.exists():
            state = json.loads(state_path.read_text())
            if any(state[k] != v for k, v in identity.items()):
                raise RuntimeError("job input or destination changed; use a new generation")
        else:
            state = {**identity, "generation": datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-") + uuid.uuid4().hex[:12]}
            save_json(state_path, state)
        if (workdir / "published.json").exists():
            return
        store = Store(workdir, policy["recipient"], policy["identity"], remote)
        db = prepare_ledger(workdir, input_db, base_ledger)
        for key, value in (("remote", remote), ("recipient", policy["recipient"])):
            previous = db.execute("SELECT value FROM archive_settings WHERE key=?", (key,)).fetchone()
            if previous and previous[0] != value:
                raise RuntimeError("base generation belongs to a different archive destination or recipient")
            db.execute("INSERT OR IGNORE INTO archive_settings VALUES(?,?)", (key, value))
        selection_path = input_db.parent / "selection-policy.json"
        with db:
            db.execute("INSERT OR IGNORE INTO generation_policy VALUES(?,?)", (state["generation"],
                selection_path.read_text() if selection_path.exists() else json.dumps({"mode": "historical", "exclusions": [], "checkpoint": completed})))
        engine = Engine(store, db, state["generation"])
        try:
            scan_sources(engine, [Path(p) for p in config["source_paths"]])
            derive_views(engine)
            if completed["sources"] != source_state(config):
                raise RuntimeError("source state changed before publication")
            publish(engine)
        except BaseException as error:
            engine.status("interrupted", error_type=type(error).__name__)
            engine.abort()
            raise
        finally:
            db.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workdir", required=True, type=Path)
    parser.add_argument("--checkpoint", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--policy", required=True, type=Path)
    parser.add_argument("--remote", default=ROOT)
    parser.add_argument("--base-ledger", type=Path, help="published previous generation's private ledger")
    parser.add_argument("--wait-checkpoint", action="store_true")
    args = parser.parse_args()
    run(args.workdir, args.checkpoint, args.config, args.policy, args.remote, args.base_ledger, args.wait_checkpoint)
