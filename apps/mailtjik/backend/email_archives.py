#!/usr/bin/env python3
"""Select, search and restore annual email archives without a custom mail reader."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import uuid

from email_archive_annual import (Store, connect, decrypted, file_hash, private_directory,
                                 run, sha, ROOT)
from email_archive_run import save_json

DEFAULT_HOME = Path.home() / ".local/state/email-archive"


def terminal_text(value):
    return "".join(c if c.isprintable() else " " for c in str(value))


def policy_path():
    deployed = Path.home() / ".config/document-archive-policy.json"
    return deployed if deployed.exists() else Path(__file__).resolve().parent.parent / "home/dot_config/document-archive-policy.json"


def store_for(args):
    policy = json.loads(args.policy.read_text())
    return Store(args.workdir, policy["recipient"], policy["identity"], args.remote)


@contextmanager
def new_output(path):
    """Authenticated output appears atomically; never replace a user's file."""
    path = Path(path)
    temp = path.with_name("." + path.name + "." + uuid.uuid4().hex + ".partial")
    try:
        with temp.open("xb") as out:
            yield out
            out.flush()
            os.fsync(out.fileno())
        os.link(temp, path)  # fails if destination already exists
    finally:
        temp.unlink(missing_ok=True)


def download_plain(store, obj, destination):
    path = store.fetch(obj)
    digest = hashlib.sha256()
    with new_output(destination) as out:
        with decrypted(path, store.identity) as stream:
            while chunk := stream.read(1024 * 1024):
                out.write(chunk)
                digest.update(chunk)
        if digest.hexdigest() != obj["plaintext_sha256"]:
            raise RuntimeError("metadata plaintext hash mismatch")


def load_catalogue(args, store):
    if args.catalogue:
        return json.loads(args.catalogue.read_text())
    local = args.workdir / "catalogue.json"
    if not local.exists():
        raise SystemExit("Use catalogue fetch --object catalogues/GENERATION.json.zst.age first")
    return json.loads(local.read_text())


def selected_partitions(args, catalogue):
    pairs = None
    if args.selection_plan:
        plan = json.loads(args.selection_plan.read_text())
        if plan.get("version") != 1 or plan.get("mode") != "restore":
            raise ValueError("use a restore selection plan")
        pairs = {(s["year"], s["account"]) for s in plan["selections"]}
    return [p for p in catalogue["partitions"]
            if (not args.year or p["year"] in args.year)
            and (not args.account or p["account"] in args.account)
            and (pairs is None or (p["year"], p["account"]) in pairs)]


def partition_db(store, part):
    cache = private_directory(store.workdir / "indexes")
    target = cache / (part["index"]["plaintext_sha256"] + ".sqlite")
    if not target.exists():
        download_plain(store, part["index"], target)
    elif file_hash(target) != part["index"]["plaintext_sha256"]:
        raise RuntimeError("cached index was modified; remove it before refetching")
    return connect(target, True)


def retrieve(store, db, digest, kind, output):
    row = db.execute("SELECT l.*,o.path,o.cipher_sha256,o.cipher_bytes FROM locations l JOIN objects o ON o.id=l.object_id WHERE l.digest=? AND l.kind=? LIMIT 1", (digest, kind)).fetchone()
    if row is None:
        return False
    path = store.fetch(row)
    found = False
    with new_output(output) as out:
        with decrypted(path, store.identity) as stream:
            with tarfile.open(fileobj=stream, mode="r|") as archive:
                for member in archive:
                    if member.name != row["member"]:
                        continue
                    if found or member.size != row["length"] or member.offset_data != row["offset"]:
                        raise RuntimeError("message locator mismatch")
                    handle = archive.extractfile(member)
                    digest_check = hashlib.sha256()
                    while chunk := handle.read(1024 * 1024):
                        out.write(chunk)
                        digest_check.update(chunk)
                    if digest_check.hexdigest() != digest:
                        raise RuntimeError("restored content hash mismatch")
                    found = True
        if not found:
            raise RuntimeError("object did not contain the indexed message")
    return True


def catalogue_fetch(args, store):
    from email_archive_annual import safe_object_path
    key = safe_object_path(args.object)
    if not key.startswith("catalogues/") or ".json." not in key or not key.endswith(".zst.age"):
        raise ValueError("expected a catalogue JSON object")
    temp = store.cache / (uuid.uuid4().hex + ".age")
    store.rclone("copyto", store.remote + "/" + key, str(temp), stdout=subprocess.DEVNULL)
    with decrypted(temp, store.identity) as stream:
        data = json.load(stream)
    if data.get("version") != 2:
        raise ValueError("unsupported catalogue format")
    # Retain each downloaded generation; replacing this private discovery
    # cache never replaces archived data.
    save_json(args.workdir / "catalogue.json", data)
    temp.unlink()
    print(f"Catalogue ready: {len(data['partitions'])} account/year collections")


def start_worker(args):
    args.workdir = private_directory(args.workdir.resolve())
    unit = "email-archives-" + sha(str(args.workdir).encode())[:12]
    code = private_directory(args.workdir / "code")
    for name in ("email_archive_annual.py", "email_archive_index.py", "email_archive_export.py", "email_archive_run.py"):
        destination = code / name
        if destination.exists() and destination.read_bytes() != Path(__file__).with_name(name).read_bytes():
            raise RuntimeError("worker code changed; stop worker explicitly before updating its pinned snapshot")
        if not destination.exists():
            shutil.copyfile(Path(__file__).with_name(name), destination)
    command = [sys.executable, str(code / "email_archive_annual.py"), "--workdir", str(args.workdir),
               "--checkpoint", str(args.checkpoint.resolve()), "--config", str(args.config.resolve()),
               "--policy", str(args.policy.resolve()), "--remote", args.remote]
    if args.base_ledger:
        command += ["--base-ledger", str(args.base_ledger.resolve())]
    if args.wait_checkpoint:
        command += ["--wait-checkpoint"]
    subprocess.run(["systemd-run", "--user", "--unit", unit, "--collect",
                    "--property=UMask=0077", "--property=Nice=10",
                    "--property=StandardOutput=append:" + str(args.workdir / "worker.log"),
                    "--property=StandardError=append:" + str(args.workdir / "worker.log"), *command], check=True)
    print("Worker started: " + unit)


def main(argv=None):
    os.umask(0o077)
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] == "live":
        import email_archive_live
        email_archive_live.main(argv[1:])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workdir", type=Path, default=DEFAULT_HOME / "annual")
    parser.add_argument("--policy", type=Path, default=policy_path())
    parser.add_argument("--remote", default=ROOT)
    parser.add_argument("--catalogue", type=Path)
    parser.add_argument("--selection-plan", type=Path)
    parser.add_argument("--year", action="append", help="repeat to select several years")
    parser.add_argument("--account", action="append", help="repeat to select several canonical addresses")
    commands = parser.add_subparsers(dest="command", required=True)
    cat = commands.add_parser("catalogue", help="list remote catalogue generations")
    cat.add_argument("--object", help="download/decrypt this catalogue object")
    commands.add_parser("list", help="list account/year coverage")
    commands.add_parser("status", help="read durable worker status, without mail contents")
    commands.add_parser("watch", help="follow worker progress; Ctrl-C detaches")
    for verb in ("start", "export"):
        sub = commands.add_parser(verb, help="start durable worker" if verb == "start" else "run export in foreground")
        sub.add_argument("--checkpoint", required=True, type=Path)
        sub.add_argument("--config", required=True, type=Path)
        sub.add_argument("--base-ledger", type=Path)
        sub.add_argument("--wait-checkpoint", action="store_true")
    search = commands.add_parser("search")
    search.add_argument("query", help="SQLite FTS5 query over subject and addresses")
    search.add_argument("--limit", type=int, default=30)
    for verb in ("fetch", "attachment"):
        sub = commands.add_parser(verb, help="restore one raw EML" if verb == "fetch" else "restore attachment by content hash")
        sub.add_argument("sha256")
        sub.add_argument("--output", required=True, type=Path)
    restore = commands.add_parser("restore", help="restore standard MBOX parts for selected years/accounts")
    restore.add_argument("--output", required=True, type=Path)
    selector = commands.add_parser("select", help="interactive year/address/excluded-label tree")
    selector.add_argument("--inventory", type=Path, help="live inventory selection tree JSON")
    selector.add_argument("--output", required=True, type=Path)
    live = commands.add_parser("live", help="Gmail/IMAP onboarding, inventory and selective capture")
    live.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.command in ("status", "watch"):
        path = args.workdir / "status.json"
        if args.command == "watch":
            import time
            previous = None
            try:
                while True:
                    current = path.read_text() if path.exists() else "No worker status yet"
                    if current != previous:
                        print(current, flush=True)
                        previous = current
                    if path.exists() and json.loads(current).get("stage") in ("complete", "interrupted"):
                        break
                    time.sleep(2)
            except KeyboardInterrupt:
                print("Detached; worker continues")
        else:
            print(path.read_text() if path.exists() else "No worker status yet")
        return
    if args.command == "live":
        import email_archive_live
        email_archive_live.main(args.arguments)
        return
    if args.command == "start":
        start_worker(args)
        return
    if args.command == "export":
        run(args.workdir, args.checkpoint, args.config, args.policy, args.remote, args.base_ledger, args.wait_checkpoint)
        return
    store = store_for(args)
    if args.command == "catalogue":
        if args.object:
            catalogue_fetch(args, store)
        else:
            result = store.rclone("lsf", store.remote + "/catalogues", "--files-only", capture_output=True, text=True)
            for name in result.stdout.splitlines():
                if ".json." in name and name.endswith(".zst.age"):
                    print(terminal_text("catalogues/" + name))
        return
    if args.command == "select" and args.inventory:
        tree = args.inventory
    else:
        catalogue = load_catalogue(args, store)
        partitions = selected_partitions(args, catalogue)
        if args.command == "select":
            tree = store.workdir / "selection-tree.json"
            save_json(tree, {"version": 1, "mode": "restore", "partitions": [
                {"year": p["year"], "account": p["account"], "messages": p["messages"], "labels": []}
                for p in partitions]})
    if args.command == "select":
        executable = shutil.which("email-archives-tree")
        if not executable:
            raise SystemExit("Build/install the Bubble Tea frontend with scripts/install-email-archives.sh")
        subprocess.run([executable, "--inventory", str(tree), "--output", str(args.output)], check=True)
        return
    if args.command == "list":
        for part in partitions:
            print(f"{part['year']:12} {terminal_text(part['account']):30} {part['messages']:9} messages")
        return
    for part in partitions:
        with partition_db(store, part) as db:
            if args.command == "search":
                rows = db.execute("SELECT m.sha256,m.normalized_utc,m.subject FROM message_search s JOIN messages m ON m.sha256=s.sha256 WHERE message_search MATCH ? LIMIT ?", (args.query, args.limit))
                for row in rows:
                    print(json.dumps({"account": part["account"], "year": part["year"], **dict(row)}, ensure_ascii=True))
            elif args.command in ("fetch", "attachment"):
                if retrieve(store, db, args.sha256, "raw" if args.command == "fetch" else "attachments", args.output):
                    print("Restored and SHA-256 verified")
                    return
            elif args.command == "restore":
                outdir = private_directory(args.output / part["year"] / sha(part["account"].encode())[:24])
                folders = {}
                for obj in db.execute("SELECT * FROM objects WHERE kind='reader' AND id IN (SELECT object_id FROM locations WHERE kind='reader') ORDER BY path"):
                    folder = sha(obj["folder"].encode())[:24]
                    folders[folder] = obj["folder"]
                    target = private_directory(outdir / folder) / (obj["id"] + ".mbox")
                    with new_output(target) as output:
                        with decrypted(store.fetch(obj), store.identity) as stream:
                            position = 0
                            for loc in db.execute("SELECT offset,length FROM locations WHERE object_id=? ORDER BY offset", (obj["id"],)):
                                skip = loc["offset"] - position
                                if skip < 0:
                                    raise RuntimeError("overlapping reader locators")
                                while skip:
                                    data = stream.read(min(skip, 1024 * 1024))
                                    if not data:
                                        raise RuntimeError("truncated reader part")
                                    skip -= len(data)
                                remaining = loc["length"]
                                while remaining:
                                    data = stream.read(min(remaining, 1024 * 1024))
                                    if not data:
                                        raise RuntimeError("truncated reader record")
                                    output.write(data)
                                    remaining -= len(data)
                                position = loc["offset"] + loc["length"]
                with new_output(outdir / "folders.json") as output:
                    output.write((json.dumps(folders, ensure_ascii=True, indent=2) + "\n").encode())
    if args.command in ("fetch", "attachment"):
        raise SystemExit("Hash not found in selected account/year collections")


if __name__ == "__main__":
    main()
