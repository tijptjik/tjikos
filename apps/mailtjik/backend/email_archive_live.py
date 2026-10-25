#!/usr/bin/env python3
"""Read-only live mail inventory and resumable selective capture.

Gmail uses desktop OAuth with PKCE and gmail.readonly. Generic IMAP uses TLS,
EXAMINE and BODY.PEEK; its exclusions are mailbox exclusions, not Gmail labels.
All connection files, tokens, inventories, plans and raw staging are private.
"""
from __future__ import annotations

import argparse
import base64
from datetime import datetime, timezone
import email
import email.message
import email.policy
import fcntl
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import imaplib
import io
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import sqlite3
import ssl
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import webbrowser

import email_archive_index as index
from email_archive_annual import connect, private_directory, sha, year_for, file_hash
from email_archive_run import save_json, source_state

SCOPE = "https://www.googleapis.com/auth/gmail.readonly"
HEADERS = ["Date", "From", "Sender", "To", "Cc", "Bcc", "Subject", "Message-ID"]


def request_json(url, data=None, headers=None):
    request = urllib.request.Request(url, data=data, headers=headers or {})
    for attempt in range(6):
        try:
            with urllib.request.urlopen(request, timeout=90) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            if error.code not in (429, 500, 502, 503, 504) or attempt == 5:
                raise
        except (TimeoutError, urllib.error.URLError):
            if attempt == 5:
                raise
        time.sleep(min(30, 2 ** attempt))


def oauth(connection, client_path):
    client = json.loads(client_path.read_text()).get("installed")
    if not client:
        raise ValueError("use a Google Desktop application OAuth client JSON")
    verifier = secrets.token_urlsafe(64)
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
    state = secrets.token_urlsafe(32)
    result = {}

    class Callback(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            query = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
            if query.get("state") != [state] or not query.get("code"):
                self.send_response(400)
                self.end_headers()
                self.wfile.write(b"Invalid OAuth callback")
                return
            result["code"] = query["code"][0]
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"Email archive authorization received. You may close this window.")

    with HTTPServer(("127.0.0.1", 0), Callback) as server:
        server.timeout = 2
        redirect = f"http://127.0.0.1:{server.server_port}/"
        query = urllib.parse.urlencode({"client_id": client["client_id"], "redirect_uri": redirect,
            "response_type": "code", "scope": SCOPE, "access_type": "offline", "prompt": "consent",
            "state": state, "code_challenge": challenge, "code_challenge_method": "S256"})
        url = "https://accounts.google.com/o/oauth2/v2/auth?" + query
        webbrowser.open(url)
        print("Complete read-only Gmail authorization in the browser (10 minute timeout).", flush=True)
        deadline = time.monotonic() + 600
        while not result and time.monotonic() < deadline:
            server.handle_request()
    if not result:
        raise TimeoutError("OAuth sign-in did not complete")
    token = request_json("https://oauth2.googleapis.com/token", urllib.parse.urlencode({
        "client_id": client["client_id"], "client_secret": client.get("client_secret", ""),
        "code": result["code"], "code_verifier": verifier, "redirect_uri": redirect,
        "grant_type": "authorization_code"}).encode())
    if not token.get("refresh_token"):
        raise RuntimeError("Google did not return a refresh token")
    save_json(connection, {"version": 1, "provider": "gmail", "id": uuid.uuid4().hex,
        "client": client, "token": token, "expires_at": time.time() + token.get("expires_in", 3600)})


class Gmail:
    def __init__(self, path):
        self.path = path
        self.config = json.loads(path.read_text())
        self.id = self.config["id"]

    def get(self, route, params=None):
        if time.time() > self.config.get("expires_at", 0) - 60:
            client = self.config["client"]
            token = request_json("https://oauth2.googleapis.com/token", urllib.parse.urlencode({
                "client_id": client["client_id"], "client_secret": client.get("client_secret", ""),
                "refresh_token": self.config["token"]["refresh_token"], "grant_type": "refresh_token"}).encode())
            self.config["token"].update(token)
            self.config["expires_at"] = time.time() + token.get("expires_in", 3600)
            save_json(self.path, self.config)
        url = "https://gmail.googleapis.com/gmail/v1/users/me/" + route
        if params:
            url += "?" + urllib.parse.urlencode(params, doseq=True)
        return request_json(url, headers={"Authorization": "Bearer " + self.config["token"]["access_token"]})

    def labels(self):
        return [{"id": x["id"], "name": x["name"]} for x in self.get("labels")["labels"]]

    def ids(self):
        page = None
        while True:
            params = {"maxResults": 500, "includeSpamTrash": "true"}
            if page:
                params["pageToken"] = page
            result = self.get("messages", params)
            for item in result.get("messages", []):
                yield item["id"]
            page = result.get("nextPageToken")
            if not page:
                break

    def metadata(self, ident):
        data = self.get("messages/" + urllib.parse.quote(ident, safe=""),
                        {"format": "metadata", "metadataHeaders": HEADERS})
        msg = email.message.EmailMessage()
        for h in data.get("payload", {}).get("headers", []):
            # Parser tolerates repeated headers found in real messages.
            msg._headers.append((h["name"], h["value"]))
        provider_date = datetime.fromtimestamp(int(data["internalDate"]) / 1000, timezone.utc).isoformat()
        return msg, data.get("labelIds", []), provider_date, data.get("sizeEstimate", 0), data

    def raw(self, ident):
        data = self.get("messages/" + urllib.parse.quote(ident, safe=""), {"format": "raw"})
        encoded = data["raw"]
        return base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4))

    def close(self):
        pass


class IMAP:
    """Generic IMAP mailbox selection, with UIDVALIDITY protected checkpoints."""
    def __init__(self, path):
        self.config = json.loads(path.read_text())
        self.id = self.config["id"]
        self.client = imaplib.IMAP4_SSL(self.config["host"], self.config.get("port", 993),
                                       ssl_context=ssl.create_default_context(), timeout=90)
        password = Path(self.config["secret_file"]).expanduser().read_text().rstrip("\r\n")
        if self.config.get("auth") == "oauth2":
            self.client.authenticate("XOAUTH2", lambda _: f"user={self.config['username']}\x01auth=Bearer {password}\x01\x01".encode())
        else:
            self.client.login(self.config["username"], password)
        self.current = None

    def labels(self):
        # Explicit mailbox names avoid guessing server hierarchy/UTF7 names.
        return [{"id": name, "name": name} for name in self.config["mailboxes"]]

    def select(self, mailbox):
        status, _ = self.client.select('"' + mailbox.replace('\\', '\\\\').replace('"', '\\"') + '"', readonly=True)
        if status != "OK":
            raise RuntimeError("IMAP read-only mailbox selection failed")
        values = self.client.response("UIDVALIDITY")[1]
        if not values or not values[0]:
            raise RuntimeError("IMAP UIDVALIDITY unavailable")
        self.current = mailbox
        return values[0].decode()

    def ids(self):
        for mailbox in self.config["mailboxes"]:
            validity = self.select(mailbox)
            status, values = self.client.uid("search", None, "ALL")
            if status != "OK":
                raise RuntimeError("IMAP UID inventory failed")
            for uid in values[0].decode().split():
                yield json.dumps([mailbox, validity, uid], separators=(",", ":"))

    def fetch(self, ident, spec):
        mailbox, validity, uid = json.loads(ident)
        if self.select(mailbox) != validity:
            raise RuntimeError("IMAP UIDVALIDITY changed; create a new inventory")
        status, data = self.client.uid("fetch", uid, spec)
        if status != "OK":
            raise RuntimeError("IMAP UID fetch failed")
        for item in data:
            if isinstance(item, tuple):
                return item
        raise RuntimeError("IMAP message disappeared during capture")

    def metadata(self, ident):
        response, raw = self.fetch(ident, "(UID RFC822.SIZE INTERNALDATE BODY.PEEK[HEADER.FIELDS (DATE FROM SENDER TO CC BCC SUBJECT MESSAGE-ID)])")
        message = email.message_from_bytes(raw, policy=email.policy.default)
        found = re.search(rb'INTERNALDATE "([^"]+)"', response)
        date = datetime.strptime(found[1].decode(), "%d-%b-%Y %H:%M:%S %z").astimezone(timezone.utc).isoformat() if found else None
        size = re.search(rb"RFC822.SIZE (\d+)", response)
        return message, [json.loads(ident)[0]], date, int(size[1]) if size else 0, {"uid_identity": ident}

    def raw(self, ident):
        return self.fetch(ident, "(UID BODY.PEEK[])")[1]

    def close(self):
        self.client.logout()


def provider(path):
    config = json.loads(path.read_text())
    return Gmail(path) if config["provider"] == "gmail" else IMAP(path)


def inventory_db(workdir):
    db = connect(workdir / "inventory.sqlite")
    db.executescript("""
      CREATE TABLE IF NOT EXISTS metadata(id TEXT PRIMARY KEY, account TEXT, year TEXT,
        year_basis TEXT, provider_date TEXT, labels TEXT, estimated_bytes INTEGER,
        evidence TEXT, scan TEXT);
      CREATE TABLE IF NOT EXISTS captures(id TEXT PRIMARY KEY,raw_sha256 TEXT,path TEXT);
    """)
    return db


def metadata_row(source, ident, scan):
    msg, labels, date, size, evidence = source.metadata(ident)
    account = index.attribution(msg)[0] or "unresolved"
    year, basis = year_for(str(msg.get("Date", "")), date)
    return (ident, account, year, basis, date, json.dumps(labels), size, json.dumps(evidence), scan)


def inventory(source, db, workdir, priority, include_spam_trash=False):
    scan = uuid.uuid4().hex
    profile = source.get("profile") if isinstance(source, Gmail) else {
        "emailAddress": source.config["username"], "host": source.config["host"]}
    start_history = profile["historyId"] if isinstance(source, Gmail) else None
    count = 0
    for ident in source.ids():
        row = metadata_row(source, ident, scan)
        db.execute("INSERT OR REPLACE INTO metadata VALUES(?,?,?,?,?,?,?,?,?)", row)
        count += 1
        if count % 100 == 0:
            db.commit()
            save_json(workdir / "status.json", {"stage": "metadata_inventory", "messages": count})
    db.commit()
    if isinstance(source, Gmail):
        # Replay changes after the full enumeration, including deletes and
        # label changes. A stale cursor fails without declaring coverage.
        page = None
        changed = set()
        while True:
            params = {"startHistoryId": start_history, "maxResults": 500}
            if page:
                params["pageToken"] = page
            result = source.get("history", params)
            for event in result.get("history", []):
                changed.update(x["id"] for x in event.get("messages", []))
            page = result.get("nextPageToken")
            if not page:
                break
        for ident in changed:
            try:
                db.execute("INSERT OR REPLACE INTO metadata VALUES(?,?,?,?,?,?,?,?,?)", metadata_row(source, ident, scan))
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    raise
                db.execute("DELETE FROM metadata WHERE id=?", (ident,))
        db.commit()
    labels = source.labels()
    counts = {}
    for row in db.execute("SELECT year,account,labels FROM metadata WHERE scan=?", (scan,)):
        if isinstance(source, Gmail) and not include_spam_trash and set(json.loads(row["labels"])) & {"SPAM", "TRASH"}:
            continue
        key = (row["year"], row["account"])
        counts[key] = counts.get(key, 0) + 1
    if isinstance(source, Gmail) and not include_spam_trash:
        labels = [label for label in labels if label["id"] not in {"SPAM", "TRASH"}]
    partitions = [{"year": year, "account": account, "messages": count}
                  for (year, account), count in sorted(counts.items())]
    for item in partitions:
        item["labels"] = labels
    result = {"version": 1, "mode": "live", "inventory_id": scan, "connection": source.id,
              "priority_addresses": list(priority), "include_spam_trash": include_spam_trash,
              "authenticated_mailbox": profile["emailAddress"],
              "provider": "gmail" if isinstance(source, Gmail) else "imap",
              "partitions": partitions}
    save_json(workdir / "inventory.json", result)
    save_json(workdir / "status.json", {"stage": "inventory_complete", "messages": count})
    return result


def selection_map(plan):
    return {(s["year"], s["account"]): set(s["exclude_label_ids"]) for s in plan["selections"]}


def allowed(row, selections, include_spam_trash):
    excluded = selections.get((row["year"], row["account"]))
    if excluded is None:
        return False
    labels = set(json.loads(row["labels"]))
    return not labels.intersection(excluded | (set() if include_spam_trash else {"SPAM", "TRASH"}))


def plan_rows(db, inventory_info, plan):
    if plan["inventory_id"] != inventory_info["inventory_id"] or plan["connection"] != inventory_info["connection"]:
        raise ValueError("selection plan belongs to a different inventory/connection")
    selections = selection_map(plan)
    return [row for row in db.execute("SELECT * FROM metadata WHERE scan=?", (plan["inventory_id"],))
            if allowed(row, selections, inventory_info["include_spam_trash"])]


def capture(source, db, workdir, info, plan):
    rows = plan_rows(db, info, plan)
    capture_dir = private_directory(workdir / "captures")
    selections = selection_map(plan)
    selected = []
    for n, row in enumerate(rows, 1):
        # Recheck labels immediately before fetching any body: All Mail cannot
        # reintroduce excluded messages. Attribution always uses FULL priority.
        fresh = metadata_row(source, row["id"], info["inventory_id"])
        current = dict(zip(("id", "account", "year", "year_basis", "provider_date", "labels", "estimated_bytes", "evidence", "scan"), fresh))
        if not allowed(current, selections, info["include_spam_trash"]):
            continue
        previous = db.execute("SELECT * FROM captures WHERE id=?", (row["id"],)).fetchone()
        if previous and Path(previous["path"]).exists() and file_hash(previous["path"]) == previous["raw_sha256"]:
            path, digest = Path(previous["path"]), previous["raw_sha256"]
        else:
            if shutil.disk_usage(capture_dir).free < max(1024**3, current["estimated_bytes"] * 3):
                raise RuntimeError("insufficient private capture staging space")
            raw = source.raw(row["id"])
            digest = sha(raw)
            path = capture_dir / (digest + ".eml")
            if not path.exists():
                temp = path.with_suffix(".partial")
                with temp.open("wb") as out:
                    out.write(raw)
                    out.flush()
                    os.fsync(out.fileno())
                temp.replace(path)
            with db:
                db.execute("INSERT OR REPLACE INTO captures VALUES(?,?,?)", (row["id"], digest, str(path)))
        current.update(raw_sha256=digest, path=str(path))
        raw_message = email.message_from_bytes(path.read_bytes(), policy=email.policy.default)
        raw_account = index.attribution(raw_message)[0] or "unresolved"
        raw_year = year_for(str(raw_message.get("Date", "")), current["provider_date"])[0]
        if (raw_account, raw_year) != (current["account"], current["year"]):
            raise RuntimeError("provider metadata differs from raw headers; refresh inventory")
        selected.append(current)
        save_json(workdir / "status.json", {"stage": "capturing", "completed": n, "selected_candidates": len(rows)})
    # A second metadata pass records changes that arrived during body capture.
    # This is explicitly a capture interval, never an atomic provider snapshot.
    retained = []
    for row in selected:
        fresh = metadata_row(source, row["id"], info["inventory_id"])
        row.update(dict(zip(("id", "account", "year", "year_basis", "provider_date", "labels", "estimated_bytes", "evidence", "scan"), fresh)))
        if allowed(row, selections, info["include_spam_trash"]):
            retained.append(row)
    package_capture(db, workdir, info, plan, retained, source.labels())


def package_capture(db, workdir, info, plan, rows, labels):
    if not rows:
        raise ValueError("no messages remain in this selection")
    job = private_directory(workdir / ("import-" + uuid.uuid4().hex))
    archive_path = job / "capture.tar"
    unique = {row["raw_sha256"]: row for row in rows}
    needed = sum(Path(row["path"]).stat().st_size for row in unique.values())
    if shutil.disk_usage(job).free < needed + 1024**3:
        raise RuntimeError("insufficient space for immutable capture source")
    input_db = index.create_database(job / "email-index.sqlite")
    input_db.execute("CREATE TABLE provider_dates(sha256 TEXT PRIMARY KEY,provider_utc TEXT,source_identity TEXT)")
    label_names = {l["id"]: l["name"] for l in labels}
    with tarfile.open(archive_path, "w", format=tarfile.USTAR_FORMAT) as archive:
        for digest, row in unique.items():
            path = Path(row["path"])
            raw = path.read_bytes()
            if sha(raw) != digest:
                raise RuntimeError("captured raw message hash mismatch")
            member = digest + ".eml"
            item = tarfile.TarInfo(member)
            item.size, item.mode = len(raw), 0o600
            archive.addfile(item, io.BytesIO(raw))
            index.process_message(input_db, archive_path, member, 0, 1, raw, "provider_raw")
            input_db.execute("DELETE FROM memberships WHERE message_sha256=?", (digest,))
            input_db.execute("INSERT INTO provider_dates VALUES(?,?,?)", (digest, row["provider_date"], info["connection"]))
    for row in rows:
        folders = [label_names.get(x, x) for x in json.loads(row["labels"])]
        if info["provider"] == "gmail":
            folders.append("All Mail")
        for folder in folders or ["Unfiled"]:
            locator = json.dumps({"member": row["raw_sha256"] + ".eml", "provider_id": row["id"],
                "connection": info["connection"], "provider_date": row["provider_date"],
                "inventory_id": info["inventory_id"], "labels": json.loads(row["labels"])})
            input_db.execute("INSERT OR IGNORE INTO memberships(message_sha256,source_archive,original_mailbox_path,locator,mailbox_offset,message_ordinal) VALUES(?,?,?,?,0,1)",
                             (row["raw_sha256"], str(archive_path), folder, locator))
    input_db.commit()
    counts = {name: input_db.execute(f"SELECT count(*) FROM {name}").fetchone()[0]
              for name in ("messages", "memberships", "attachments")}
    input_db.close()
    config = {"priority_addresses": info["priority_addresses"], "source_paths": [str(archive_path)]}
    save_json(job / "config.json", config)
    save_json(job / "selection-policy.json", {"inventory": info, "plan": plan,
        "coverage": "selected provider IDs captured over an interval; rerun inventory for late arrivals"})
    save_json(job / "index.complete.json", {"sources": source_state(config), "data_directory": str(job),
        "config_sha256": file_hash(job / "config.json"), "counts": counts})
    save_json(workdir / "capture.complete.json", {"import_directory": str(job), "counts": counts})
    save_json(workdir / "status.json", {"stage": "captured_awaiting_encrypted_export", **counts})
    print("Private import checkpoint ready; run email-archives start using capture.complete.json paths.")


def main(argv=None):
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workdir", type=Path, required=True)
    parser.add_argument("--connection", type=Path, required=True)
    sub = parser.add_subparsers(dest="command", required=True)
    auth = sub.add_parser("auth-gmail")
    auth.add_argument("--client", required=True, type=Path)
    imap = sub.add_parser("connect-imap")
    imap.add_argument("--host", required=True)
    imap.add_argument("--username", required=True)
    imap.add_argument("--secret-file", required=True, type=Path)
    imap.add_argument("--mailbox", action="append", required=True)
    imap.add_argument("--auth", choices=("password", "oauth2"), default="password")
    inv = sub.add_parser("inventory")
    inv.add_argument("--config", required=True, type=Path, help="private priority_addresses configuration")
    inv.add_argument("--include-spam-trash", action="store_true")
    for verb in ("preview", "capture", "start-capture"):
        command = sub.add_parser(verb)
        command.add_argument("--plan", required=True, type=Path)
    args = parser.parse_args(argv)
    workdir = private_directory(args.workdir.resolve())
    if args.command == "start-capture":
        unit = "email-capture-" + sha(str(workdir).encode())[:12]
        code = private_directory(workdir / ("worker-code-" + uuid.uuid4().hex))
        for name in ("email_archive_live.py", "email_archive_annual.py", "email_archive_index.py", "email_archive_export.py", "email_archive_run.py"):
            shutil.copyfile(Path(__file__).with_name(name), code / name)
        subprocess.run(["systemd-run", "--user", "--collect", "--unit", unit,
            "--property=UMask=0077", "--property=Nice=10",
            "--property=StandardOutput=append:" + str(workdir / "capture.log"),
            "--property=StandardError=append:" + str(workdir / "capture.log"),
            sys.executable, str(code / "email_archive_live.py"), "--workdir", str(workdir),
            "--connection", str(args.connection.resolve()), "capture", "--plan", str(args.plan.resolve())], check=True)
        print("Capture worker started: " + unit)
        return
    if args.command == "connect-imap":
        if args.connection.exists() or not args.secret_file.is_file():
            raise ValueError("use a new connection path and an existing private secret file")
        save_json(args.connection, {"version": 1, "provider": "imap", "id": uuid.uuid4().hex,
            "host": args.host, "username": args.username, "secret_file": str(args.secret_file.resolve()),
            "auth": args.auth, "mailboxes": args.mailbox})
        print("IMAP connection saved. Exclusions apply to mailboxes; generic IMAP has no cross-folder Gmail label identity.")
        return
    if args.command == "auth-gmail":
        if args.connection.exists():
            raise ValueError("connection already exists; choose a new private path")
        oauth(args.connection, args.client)
        print("Read-only Gmail connection saved privately")
        return
    with (workdir / "live.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        connection_config = json.loads(args.connection.read_text())
        binding = {"connection": connection_config["id"], "provider": connection_config["provider"]}
        binding_path = workdir / "connection-binding.json"
        if binding_path.exists() and json.loads(binding_path.read_text()) != binding:
            raise ValueError("this capture directory belongs to a different mailbox connection")
        if not binding_path.exists():
            save_json(binding_path, binding)
        db = inventory_db(workdir)
        if args.command == "inventory":
            config = json.loads(args.config.read_text())
            index.PRIORITY = tuple(str(a).strip().casefold() for a in config["priority_addresses"])
            if not index.PRIORITY or len(set(index.PRIORITY)) != len(index.PRIORITY):
                raise ValueError("priority_addresses must be nonempty and unique")
        else:
            info = json.loads((workdir / "inventory.json").read_text())
            index.PRIORITY = tuple(info["priority_addresses"])
            plan = json.loads(args.plan.read_text())
            if args.command == "preview":
                rows = plan_rows(db, info, plan)
                print(json.dumps({"selected_provider_ids": len(rows), "estimated_bytes": sum(r["estimated_bytes"] for r in rows),
                                  "raw_deduplication": "after_body_download", "body_downloads_started": False}))
                return
        source = provider(args.connection)
        try:
            if args.command == "inventory":
                try:
                    inventory(source, db, workdir, index.PRIORITY, args.include_spam_trash)
                except urllib.error.HTTPError as error:
                    if not isinstance(source, Gmail) or error.code != 404:
                        raise
                    # Expired history or a racing deletion: full re-enumeration
                    # reconciles provider IDs instead of trusting a stale cursor.
                    inventory(source, db, workdir, index.PRIORITY, args.include_spam_trash)
            else:
                if source.id != info["connection"]:
                    raise ValueError("wrong provider connection for this inventory")
                capture(source, db, workdir, info, plan)
        except BaseException as error:
            save_json(workdir / "status.json", {"stage": "interrupted", "error_type": type(error).__name__})
            raise
        finally:
            source.close()
            db.close()


if __name__ == "__main__":
    main()
