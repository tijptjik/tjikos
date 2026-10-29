#!/usr/bin/env python3
"""Export selected Google Drive trees and back them up to Restic on si."""
from __future__ import annotations

import datetime as dt
import contextlib
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import stat
import subprocess
import sys
import time


HOME = Path.home()
CONFIG = HOME / ".config/drive-restic"
STATE = HOME / ".local/state/drive-restic"
RCLONE_CONFIG = CONFIG / "drive.conf"
RESTIC_PASSWORD = CONFIG / "restic-password"
REPOSITORY = Path("/mnt/storage/private/backups/google-drive")
MOUNT_ROOT = Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")) / "drive-restic"
EXPORT_FORMATS = "docx,xlsx,pptx,svg"
SOURCES = (
    ("private-archives", "type-drive:private/archives", "archives"),
    ("gsuite", "type-drive:GSUITE", "gsuite"),
)


class BackupError(RuntimeError):
    pass


def run(args: list[str], *, capture: bool = False):
    return subprocess.run(
        [str(item) for item in args], check=True,
        capture_output=capture, text=capture,
    )


def safe_file(path: Path) -> None:
    if path.is_symlink():
        raise BackupError(f"Credential path must not be a symlink: {path.name}")
    try:
        info = path.stat()
    except OSError as error:
        raise BackupError(f"Required credential is unavailable: {path.name}") from error
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise BackupError(f"Credential must be a regular owner-only file: {path.name}")
    if not path.read_text().strip():
        raise BackupError(f"Credential is empty: {path.name}")


def rclone(args: list[str], *, capture: bool = False):
    return run(["rclone", "--config", RCLONE_CONFIG,
                "--drive-export-formats", EXPORT_FORMATS, *args], capture=capture)


def restic(args: list[str], *, capture: bool = False):
    return run(["restic", "-r", REPOSITORY, "-p", RESTIC_PASSWORD,
                "--retry-lock", "15m", *args], capture=capture)


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("w", encoding="utf-8") as stream:
        os.chmod(temporary, 0o600)
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


def remote_size(source: str) -> dict[str, int]:
    result = rclone(["size", "--json", source], capture=True)
    try:
        size = json.loads(result.stdout)
        count, byte_count = int(size["count"]), int(size["bytes"])
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise BackupError(f"Could not inventory Drive source {source}") from error
    if count <= 0:
        raise BackupError(f"Drive source is empty; refusing an empty backup: {source}")
    return {"count": count, "bytes": byte_count}


def remote_entries(source: str) -> int:
    result = rclone(["lsf", "--max-depth", "1", source], capture=True)
    entries = [line for line in result.stdout.splitlines() if line]
    if not entries:
        raise BackupError(f"Drive source is empty; refusing an empty backup: {source}")
    return len(entries)


def mounted(target: Path) -> bool:
    return os.path.ismount(target)


@contextlib.contextmanager
def drive_mount(source: str, name: str):
    target = MOUNT_ROOT / name
    if target.is_symlink() or (target.exists() and not target.is_dir()):
        raise BackupError(f"Mount path must be a real directory: {target}")
    if mounted(target):
        result = run(["findmnt", "--noheadings", "--raw", "--mountpoint", target,
                      "--output", "FSTYPE"], capture=True)
        if result.stdout.strip() != "fuse.rclone":
            raise BackupError(f"Unexpected filesystem already mounted at {target}")
        run(["fusermount3", "-u", target])
    target.mkdir(parents=True, exist_ok=True, mode=0o700)
    process = subprocess.Popen([
        "rclone", "--config", str(RCLONE_CONFIG), "--drive-export-formats", EXPORT_FORMATS,
        "mount", "--read-only", "--vfs-cache-mode", "off", source, str(target),
    ])
    try:
        for _ in range(120):
            if process.poll() is not None:
                raise BackupError(f"rclone mount exited before becoming ready: {source}")
            if mounted(target):
                break
            time.sleep(0.5)
        else:
            raise BackupError(f"Timed out waiting for Drive mount: {source}")
        yield target
    finally:
        if mounted(target):
            subprocess.run(["fusermount3", "-u", str(target)], check=False)
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.send_signal(signal.SIGKILL)
                process.wait()
        if mounted(target):
            subprocess.run(["fusermount3", "-uz", str(target)], check=False)
        if mounted(target):
            raise BackupError(f"Could not unmount Drive source: {source}")
        target.rmdir()


def backup() -> None:
    if socket.gethostname().split(".")[0] != "si":
        raise BackupError("This backup must run on si")
    safe_file(RCLONE_CONFIG)
    safe_file(RESTIC_PASSWORD)
    if MOUNT_ROOT.is_symlink() or REPOSITORY.is_symlink():
        raise BackupError("Backup repository and mount paths must not be symlinks")
    MOUNT_ROOT.mkdir(parents=True, exist_ok=True, mode=0o700)
    REPOSITORY.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if not (REPOSITORY / "config").is_file():
        if REPOSITORY.exists() and any(REPOSITORY.iterdir()):
            raise BackupError("Restic target exists but is not an initialized repository")
        restic(["init"])

    receipts = []
    for tag, source, name in SOURCES:
        entries = remote_entries(source)
        with drive_mount(source, name) as mounted_source:
            if not any(mounted_source.iterdir()):
                raise BackupError(f"Mounted Drive source is empty: {source}")
            print(f"BACKUP {source} → {REPOSITORY}", flush=True)
            restic(["backup", "--host", "si",
                    "--tag", "google-drive", "--tag", tag, mounted_source])
        receipts.append({"tag": tag, "source": source,
                         "top_level_entries": entries,
                         "google_native_export_formats": EXPORT_FORMATS})

    restic(["check"])
    for tag, _, _ in SOURCES:
        restic(["forget", "--host", "si", "--tag", tag,
                "--group-by", "host,tags", "--keep-daily", "1",
                "--keep-weekly", "1", "--keep-monthly", "1", "--prune"])
    write_json(STATE / "last-success.json", {
        "completed": dt.datetime.now(dt.timezone.utc).isoformat(),
        "repository": str(REPOSITORY), "sources": receipts,
    })
    print("DONE Google Drive exports backed up, checked, and retained", flush=True)


def main() -> int:
    STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        with (STATE / "operation.lock").open("a") as lock:
            os.chmod(lock.fileno(), 0o600)
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                raise BackupError("A Drive Restic backup is already running") from error
            backup()
    except (BackupError, OSError, subprocess.CalledProcessError) as error:
        write_json(STATE / "last-error.json", {
            "time": dt.datetime.now(dt.timezone.utc).isoformat(),
            "error": str(error),
        })
        print(f"FAILED {error}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
