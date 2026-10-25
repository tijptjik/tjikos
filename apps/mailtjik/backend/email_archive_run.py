#!/usr/bin/env python3
"""Run indexing and export sequentially with durable, private checkpoints."""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
from datetime import datetime, timezone
import uuid


def save_json(path: Path, value: dict) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    temporary.replace(path)
    directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def source_state(config: dict) -> list[dict]:
    result = []
    for name in config["source_paths"]:
        path = Path(name)
        stat = path.stat()
        if not path.is_file():
            raise RuntimeError("a configured archive is not a regular file")
        result.append({"path": str(path), "size": stat.st_size,
                       "mtime_ns": stat.st_mtime_ns})
    return result


def validate_index(path: Path, config: dict) -> dict:
    with sqlite3.connect(f"file:{path}?mode=ro", uri=True, timeout=30) as db:
        if db.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
            raise RuntimeError("index integrity check failed")
        counts = {name: db.execute(f"SELECT count(*) FROM {name}").fetchone()[0]
                  for name in ("messages", "memberships", "attachments")}
        for name, expected in config.get("expected_counts", {}).items():
            if counts.get(name) != expected:
                raise RuntimeError(f"index count differs from completed previous scan: {name}")
        if not counts["messages"]:
            raise RuntimeError("index contains no messages")
    return counts


def require_drive_mount(path: Path) -> None:
    result = subprocess.run(
        ["findmnt", "--json", "--mountpoint", str(path), "-o", "SOURCE,FSTYPE,OPTIONS"],
        check=True, capture_output=True, text=True,
    )
    mounts = json.loads(result.stdout).get("filesystems", [])
    if (len(mounts) != 1 or mounts[0]["source"] != "gi-private"
            or mounts[0]["fstype"] != "fuse.rclone"
            or "rw" not in mounts[0]["options"].split(",")):
        raise RuntimeError("the expected writable gi-private mount is unavailable")


def run(workdir: Path, config_path: Path) -> None:
    os.umask(0o077)
    workdir.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(workdir, 0o700)
    if str(workdir.resolve()).startswith(("/tmp/", "/var/tmp/", "/run/")):
        raise ValueError("use persistent storage for the archive working directory")
    with (workdir / "pipeline.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        config = json.loads(config_path.read_text())
        config_digest = hashlib.sha256(config_path.read_bytes()).hexdigest()
        sources = source_state(config)
        scripts = Path(__file__).resolve().parent
        marker = workdir / "index.complete.json"
        if marker.exists():
            checkpoint = json.loads(marker.read_text())
            if checkpoint["sources"] != sources:
                raise RuntimeError("source archives changed since indexing")
            if checkpoint["config_sha256"] != config_digest:
                raise RuntimeError("private configuration changed since indexing")
            data = Path(checkpoint["data_directory"])
            counts = validate_index(data / "email-index.sqlite", config)
        else:
            stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
            data = workdir / f"index-{stamp}-{uuid.uuid4().hex[:8]}"
            data.mkdir(mode=0o700)
            print("Starting index in persistent storage", flush=True)
            with (data / "index-result.json").open("w") as output:
                subprocess.run(
                    [sys.executable, str(scripts / "email_archive_index.py"),
                     "--workdir", str(data), "--config", str(config_path)],
                    check=True, stdout=output,
                )
            if source_state(config) != sources:
                raise RuntimeError("source archives changed while indexing")
            counts = validate_index(data / "email-index.sqlite", config)
            save_json(marker, {"sources": sources, "data_directory": str(data),
                               "counts": counts, "config_sha256": config_digest})
        print("Index complete: " + json.dumps(counts), flush=True)
        uploaded = workdir / "upload.complete.json"
        if uploaded.exists():
            print("Upload already completed; restoration validation remains separate", flush=True)
            return
        require_drive_mount(Path.home() / ".mnt/google/private")
        print("Starting encrypted export", flush=True)
        with (data / "export-result.json").open("w") as output:
            subprocess.run(
                [sys.executable, str(scripts / "email_archive_export.py"),
                 "--workdir", str(data), "--config", str(config_path)],
                check=True, stdout=output,
            )
        result = json.loads((data / "export-result.json").read_text())
        if result["raw_messages"] != counts["messages"]:
            raise RuntimeError("export count differs from index")
        save_json(uploaded, {"status": "uploaded_awaiting_restore_validation", **result})
        print("Upload completed and queue drained; restoration validation pending", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workdir", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)
    args = parser.parse_args()
    run(args.workdir.resolve(), args.config.resolve())
