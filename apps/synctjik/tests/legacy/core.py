from __future__ import annotations

import contextlib
import datetime as dt
import difflib
import fcntl
import fnmatch
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import socket
import sqlite3
import stat
import subprocess
import tarfile
import tempfile
import time
import uuid


class ArchiveError(RuntimeError):
    pass


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def file_hash(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def atomic_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temp = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    try:
        with open(temp, "xb") as stream:
            os.chmod(temp, 0o600)
            stream.write(canonical(value) + b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temp, path)
    finally:
        temp.unlink(missing_ok=True)


def read_json(path):
    return json.loads(path.read_text())


def safe_name(name):
    p = PurePosixPath(name)
    if (
        not name
        or p.is_absolute()
        or any(x in ("", ".", "..") for x in name.split("/"))
    ):
        raise ArchiveError(f"Unsafe relative path: {name!r}")
    return name


def host_name(value):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", value):
        raise ArchiveError("Invalid machine name")
    return value


def excluded(name, policy):
    parts = PurePosixPath(name).parts
    if policy.get("include") and not any(
        name == p or name.startswith(p + "/") for p in policy["include"]
    ):
        return True
    return any(
        fnmatch.fnmatchcase(name, p)
        or any(fnmatch.fnmatchcase(part, p) for part in parts)
        for p in policy.get("exclude", [])
    )


def selected_file(name, patterns):
    """Match root-relative segments; a trailing ** includes descendants."""
    parts = PurePosixPath(name).parts
    for pattern in patterns:
        wanted = PurePosixPath(pattern).parts
        recursive = wanted[-1:] == ("**",)
        fixed = wanted[:-1] if recursive else wanted
        if (recursive and len(parts) >= len(fixed)) or len(parts) == len(fixed):
            if all(fnmatch.fnmatchcase(part, glob) for part, glob in zip(parts, fixed)):
                return True
    return False


def selected_directory(name, patterns):
    """Whether any allowlisted file could be below this root-relative directory."""
    parts = PurePosixPath(name).parts
    for pattern in patterns:
        wanted = PurePosixPath(pattern).parts
        recursive = wanted[-1:] == ("**",)
        fixed = wanted[:-1] if recursive else wanted
        if (recursive or len(parts) < len(fixed)) and all(
            fnmatch.fnmatchcase(part, glob) for part, glob in zip(parts, fixed)
        ):
            return True
    return False


def scan(root, policy=None, file_entry=None, compact_telegram=False):
    """Content and permissions, without following symlinks or recording sockets."""
    policy = policy or {}
    if root.is_symlink():
        raise ArchiveError(f"Folder root is a symlink: {root}")
    if not root.exists():
        return {}
    required, compacted = {}, {}
    if policy.get("telegram_themes"):
        if (
            policy.get("mode") not in ("device", "desktop", "master-backup")
            or not policy.get("encrypted", True)
            or not policy.get("include_files")
        ):
            raise ArchiveError("Telegram themes require an encrypted allowlisted backup")
        from .telegram import TelegramSettingsError, theme_recovery
        try:
            required, compacted = theme_recovery(root)
        except TelegramSettingsError as exc:
            raise ArchiveError(str(exc)) from exc
        policy = policy | {"include_files": sorted(set(policy["include_files"]) | set(required))}
    result = {}

    def walk(directory):
        for path in sorted(directory.iterdir()):
            name = path.relative_to(root).as_posix()
            if excluded(name, policy):
                continue
            st = path.lstat()
            mode = stat.S_IMODE(st.st_mode) & 0o777
            if policy.get("include_files") and not stat.S_ISDIR(st.st_mode):
                if not selected_file(name, policy["include_files"]):
                    continue
            if stat.S_ISLNK(st.st_mode):
                result[name] = {
                    "type": "link",
                    "target": os.readlink(path),
                    "mode": mode,
                }
            elif stat.S_ISDIR(st.st_mode):
                if policy.get("include_files") and not selected_directory(
                    name, policy["include_files"]
                ):
                    continue
                result[name] = {"type": "dir", "mode": mode}
                walk(path)
                if policy.get("include_files") and not any(
                    child.startswith(name + "/") for child in result if child != name
                ):
                    del result[name]
            elif stat.S_ISREG(st.st_mode):
                # SQLite WAL and journals belong to the online database snapshot.
                if any(
                    name.endswith(s)
                    and (root / name[: -len(s)]).is_file()
                    and is_sqlite(root / name[: -len(s)])
                    for s in ("-wal", "-shm", "-journal")
                ):
                    continue
                result[name] = {
                    "type": "file",
                    "mode": mode,
                    "size": st.st_size,
                    "sha256": file_hash(path) if file_entry is None else "",
                }
                if file_entry is not None:
                    result[name].update(file_entry(path, st))
            elif not stat.S_ISSOCK(st.st_mode):
                raise ArchiveError(f"Unsupported special file: {path}")

    walk(root)
    for name, fingerprint in required.items():
        if result.get(name, {}).get("sha256") != fingerprint:
            raise ArchiveError("Telegram settings/theme changed during selection or was excluded")
    if compact_telegram:
        for name, data in compacted.items():
            result[name].update(size=len(data), sha256=hashlib.sha256(data).hexdigest())
    return result


def is_sqlite(path):
    with open(path, "rb") as stream:
        return stream.read(16) == b"SQLite format 3\0"


def copy_regular(source, target):
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as src:
        if not stat.S_ISREG(os.fstat(src.fileno()).st_mode):
            raise ArchiveError(f"Source stopped being a regular file: {source}")
        with open(target, "xb") as dst:
            shutil.copyfileobj(src, dst)


def sqlite_copy(source, target, recovery=None):
    """Snapshot committed WAL; optionally remove history from the copy only."""
    if source.resolve() == target.resolve() or target.exists():
        raise ArchiveError("SQLite snapshot target must be a new file")
    recovery = recovery or {}
    if set(recovery) - {"omit_tables", "max_bytes"}:
        raise ArchiveError("Unknown SQLite recovery option")
    omitted = recovery.get("omit_tables", [])
    if not isinstance(omitted, list) or any(
        not isinstance(name, str)
        or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name)
        or name.startswith("sqlite_")
        for name in omitted
    ):
        raise ArchiveError("Invalid SQLite recovery table names")
    limit = recovery.get("max_bytes")
    if limit is not None and (type(limit) is not int or limit <= 0):
        raise ArchiveError("Invalid SQLite recovery size limit")
    start = time.monotonic()

    def progress(*_):
        if time.monotonic() - start > 120:
            raise ArchiveError(f"SQLite snapshot timed out: {source.name}")

    with contextlib.closing(
        sqlite3.connect(source.as_uri() + "?mode=ro", uri=True, timeout=5)
    ) as src:
        with contextlib.closing(sqlite3.connect(target)) as dst:
            src.backup(dst, pages=1024, progress=progress)
            dst.execute("PRAGMA journal_mode=DELETE")
            if omitted:
                tables = {
                    row[0] for row in dst.execute(
                        "SELECT name FROM sqlite_schema WHERE type='table'"
                    )
                }
                if set(omitted) - tables:
                    raise ArchiveError("SQLite recovery schema changed: missing configured tables")
                triggers = {
                    row[0] for row in dst.execute(
                        "SELECT tbl_name FROM sqlite_schema WHERE type='trigger'"
                    )
                }
                if triggers.intersection(omitted):
                    raise ArchiveError("SQLite recovery refuses deletion triggers")
                # Do not cascade into retained configuration/credentials.
                dst.execute("PRAGMA foreign_keys=OFF")
                for name in sorted(set(omitted)):
                    dst.execute(f'DELETE FROM "{name}"')
                    if "sqlite_sequence" in tables:
                        dst.execute("DELETE FROM sqlite_sequence WHERE name=?", (name,))
                dst.commit()
                if dst.execute("PRAGMA foreign_key_check").fetchone() is not None:
                    raise ArchiveError("SQLite recovery would leave broken foreign keys")
                # Compact only the disposable snapshot; never vacuum the source.
                dst.execute("VACUUM")
            if dst.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
                raise ArchiveError(f"SQLite validation failed: {source.name}")
    if limit is not None and target.stat().st_size > limit:
        raise ArchiveError("SQLite recovery snapshot exceeds its size limit; review schema before upload")


def sqlite_recovery(policy, name):
    options = policy.get("sqlite_recovery", {}).get(name)
    if options is not None and (
        policy.get("mode") not in ("device", "desktop", "master-backup")
        or not policy.get("encrypted", True)
        or name not in policy.get("sqlite", [])
    ):
        raise ArchiveError("SQLite recovery requires encrypted backup-only mode and a required SQLite file")
    return options


def check_tree(entries):
    for name, entry in entries.items():
        safe_name(name)
        if entry["type"] not in ("dir", "file", "link"):
            raise ArchiveError("Unsupported manifest entry")
        for parent in PurePosixPath(name).parents:
            if str(parent) == ".":
                break
            if entries.get(str(parent), {}).get("type") != "dir":
                raise ArchiveError(
                    f"File/directory conflict at {parent}; resolve the whole subtree"
                )


def assert_no_writers(root):
    """Do not swap a directory out from under an app with a writable file open."""
    prefix = str(root.absolute()) + "/"
    for process in Path("/proc").iterdir():
        if not process.name.isdigit() or process.name == str(os.getpid()):
            continue
        try:
            if process.stat().st_uid != os.getuid():
                continue
            for fd in (process / "fd").iterdir():
                try:
                    if not os.readlink(fd).startswith(prefix):
                        continue
                    info = (process / "fdinfo" / fd.name).read_text()
                    flags = next(
                        line.split()[1]
                        for line in info.splitlines()
                        if line.startswith("flags:")
                    )
                    if int(flags, 8) & os.O_ACCMODE:
                        raise ArchiveError(
                            f"Close the app writing {root} before sync (PID {process.name})"
                        )
                except (FileNotFoundError, PermissionError, ProcessLookupError):
                    continue
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue


def stage_tree(source, dest, policy):
    if not source.is_dir() or source.is_symlink():
        raise ArchiveError(f"Missing or symlinked source folder: {source}")
    before = scan(source, policy)
    dest.mkdir(mode=0o700)
    sqlite_names = set()
    for name, entry in before.items():
        p, q = source / name, dest / name
        if entry["type"] == "dir":
            q.mkdir(parents=True, exist_ok=True)
        elif entry["type"] == "link":
            q.symlink_to(entry["target"])
        else:
            if is_sqlite(p):
                sqlite_names.add(name)
                sqlite_copy(p, q, sqlite_recovery(policy, name))
            else:
                copy_regular(p, q)
            os.chmod(q, entry["mode"])
    for name, entry in reversed(list(before.items())):
        if entry["type"] == "dir":
            os.chmod(dest / name, entry["mode"])
    after = scan(source, policy)
    stable_before = {k: v for k, v in before.items() if k not in sqlite_names}
    stable_after = {k: v for k, v in after.items() if k not in sqlite_names}
    if stable_before != stable_after or set(before) != set(after):
        raise ArchiveError(
            f"Files changed during backup: {source}; retry with its app closed"
        )
    compacted = {}
    if policy.get("telegram_themes"):
        from .telegram import TelegramSettingsError, theme_recovery
        try:
            copied, compacted = theme_recovery(dest)
        except TelegramSettingsError as exc:
            raise ArchiveError(str(exc)) from exc
        if any(before.get(name, {}).get("sha256") != checksum for name, checksum in copied.items()):
            raise ArchiveError("Telegram settings/theme changed while copying")
        for name, data in compacted.items():
            (dest / name).write_bytes(data)
    staged = scan(dest)
    if {k: v for k, v in staged.items() if k not in sqlite_names and k not in compacted} != {
        k: v for k, v in stable_before.items() if k not in compacted
    }:
        raise ArchiveError(f"Staging verification failed: {source}")
    for name, data in compacted.items():
        if (
            staged[name]["sha256"] != hashlib.sha256(data).hexdigest()
            or staged[name]["mode"] != before[name]["mode"]
        ):
            raise ArchiveError("Telegram theme recovery staging failed")
    for name in set(policy.get("sqlite", [])) | set(policy.get("sqlite_recovery", {})):
        if name not in sqlite_names:
            raise ArchiveError(f"Required SQLite database missing or invalid: {name}")
    for name in policy.get("include", []):
        if name not in staged:
            raise ArchiveError(f"Required backup file missing: {name}")
    return staged, sorted(sqlite_names)


def backup_policy(policy):
    # Assignment and retention changes do not change the archived contents.
    result = {
        k: v for k, v in policy.items() if k not in ("devices", "retention", "review")
    }
    result["encrypted"] = policy.get("encrypted", True)
    return result


def fingerprint_tree(source, policy, cache_path, temp):
    """Reuse regular-file hashes; always snapshot SQLite, including uncheckpointed WAL."""
    if not source.is_dir() or source.is_symlink():
        raise ArchiveError(f"Missing or symlinked source folder: {source}")
    try:
        cached = read_json(cache_path)
        cached = cached.get("files", {}) if cached.get("version") == 1 else {}
        if not isinstance(cached, dict):
            cached = {}
    except (FileNotFoundError, ValueError, AttributeError):
        cached = {}
    updated, sqlite_names = {}, set()

    def signature(st):
        return [
            st.st_dev,
            st.st_ino,
            st.st_size,
            st.st_mtime_ns,
            st.st_ctime_ns,
            st.st_mode,
        ]

    def entry(path, st):
        name = path.relative_to(source).as_posix()
        if is_sqlite(path):
            sqlite_names.add(name)
            snapshot = temp / (digest(name) + ".sqlite")
            sqlite_copy(path, snapshot, sqlite_recovery(policy, name))
            return {"size": snapshot.stat().st_size, "sha256": file_hash(snapshot)}
        prior = cached.get(name, {})
        if (
            isinstance(prior, dict)
            and prior.get("stat") == signature(st)
            and re.fullmatch(r"[0-9a-f]{64}", str(prior.get("sha256", "")))
        ):
            checksum = prior["sha256"]
        else:
            checksum = file_hash(path)
        if signature(path.lstat()) != signature(st):
            raise ArchiveError(f"File changed during change check: {path}")
        updated[name] = {"stat": signature(st), "sha256": checksum}
        return {"sha256": checksum}

    root_mode = stat.S_IMODE(source.stat().st_mode) & 0o777
    entries = scan(source, policy, file_entry=entry, compact_telegram=True)
    for name in set(policy.get("sqlite", [])) | set(policy.get("sqlite_recovery", {})):
        if name not in sqlite_names:
            raise ArchiveError(f"Required SQLite database missing or invalid: {name}")
    for name in policy.get("include", []):
        if name not in entries:
            raise ArchiveError(f"Required backup file missing: {name}")
    atomic_json(cache_path, {"version": 1, "files": updated})
    return entries, root_mode


def pack(tree, entries, output):
    with open(output, "wb") as out:
        proc = subprocess.Popen(
            ["zstd", "-q", "-3", "-T0"], stdin=subprocess.PIPE, stdout=out
        )
        try:
            with tarfile.open(fileobj=proc.stdin, mode="w|") as tar:
                for name, e in entries.items():
                    info = tarfile.TarInfo(name)
                    info.mode = e["mode"]
                    if e["type"] == "dir":
                        info.type = tarfile.DIRTYPE
                        tar.addfile(info)
                    elif e["type"] == "link":
                        info.type = tarfile.SYMTYPE
                        info.linkname = e["target"]
                        tar.addfile(info)
                    else:
                        info.size = e["size"]
                        with open(tree / name, "rb") as stream:
                            tar.addfile(info, stream)
        finally:
            proc.stdin.close()
        if proc.wait() != 0:
            raise ArchiveError("Compression failed")


def unpack(archive, entries, dest):
    """Validate all members against the authenticated manifest before writing."""
    check_tree(entries)
    raw = dest.with_name(dest.name + ".tar")
    subprocess.run(["zstd", "-q", "-d", str(archive), "-o", str(raw)], check=True)
    try:
        with tarfile.open(raw) as tar:
            members = tar.getmembers()
            names = [m.name for m in members]
            if len(names) != len(set(names)) or set(names) != set(entries):
                raise ArchiveError("Archive members differ from manifest")
            for m in members:
                safe_name(m.name)
                e = entries[m.name]
                kind = (
                    "dir"
                    if m.isdir()
                    else "file"
                    if m.isfile()
                    else "link"
                    if m.issym()
                    else None
                )
                if (
                    kind != e["type"]
                    or (kind == "file" and m.size != e["size"])
                    or (kind == "link" and m.linkname != e["target"])
                ):
                    raise ArchiveError("Archive member differs from manifest")
            dest.mkdir(mode=0o700)
            for m in sorted(members, key=lambda m: (m.name.count("/"), m.name)):
                p = dest / m.name
                if m.isdir():
                    p.mkdir()
                elif m.isfile():
                    with tar.extractfile(m) as source, open(p, "xb") as output:
                        shutil.copyfileobj(source, output)
                    p.chmod(entries[m.name]["mode"])
                else:
                    p.symlink_to(m.linkname)
            for m in reversed(members):
                if m.isdir():
                    (dest / m.name).chmod(entries[m.name]["mode"])
        if scan(dest) != entries:
            raise ArchiveError("Restored files fail manifest verification")
    finally:
        raw.unlink(missing_ok=True)


def merge_entries(base, local, incoming, master=False, resolutions=None):
    resolutions = resolutions or {}
    desired, changes, conflicts = {}, [], []
    for name in sorted(set(base) | set(local) | set(incoming)):
        b, local_entry, r = base.get(name), local.get(name), incoming.get(name)
        if name in resolutions:
            if resolutions[name] not in ("local", "source"):
                raise ArchiveError("Resolution must be local or source")
            choice = local_entry if resolutions[name] == "local" else r
        elif master or local_entry == r or local_entry == b:
            choice = r
        elif r == b:
            choice = local_entry
        else:
            decision = resolutions.get(name)
            if decision not in ("local", "source"):
                conflicts.append(name)
                choice = local_entry
            else:
                choice = local_entry if decision == "local" else r
        if choice is not None:
            desired[name] = choice
        if choice != local_entry:
            changes.append(
                {
                    "path": name,
                    "action": "delete"
                    if choice is None
                    else "add"
                    if local_entry is None
                    else "replace",
                }
            )
    # Keep a directory deleted upstream when it still contains local additions.
    # A file-vs-directory replacement is a real conflict, never silently erased.
    for name in list(desired):
        for parent in PurePosixPath(name).parents:
            p = str(parent)
            if p == ".":
                break
            if p not in desired and local.get(p, {}).get("type") == "dir":
                desired[p] = local[p]
                changes = [x for x in changes if x["path"] != p]
    for name in desired:
        for parent in PurePosixPath(name).parents:
            if str(parent) == ".":
                break
            if desired.get(str(parent), {}).get("type") != "dir":
                conflicts.extend([str(parent), name])
    return desired, changes, sorted(set(conflicts))


def retention_keep(refs, latest, policy, current=None):
    """Latest, previous ISO week, previous calendar month; at most three."""
    if policy not in ("latest", "daily-weekly-monthly"):
        raise ArchiveError("Unknown retention policy")
    keep = {latest}
    if policy == "latest":
        return keep
    current = current or dt.datetime.now(dt.timezone.utc)
    monday = current.date() - dt.timedelta(days=current.weekday())
    month = current.date().replace(day=1)
    previous_month = (month - dt.timedelta(days=1)).replace(day=1)
    dated = sorted(
        (
            dt.datetime.fromisoformat(r["created_at"]).astimezone(dt.timezone.utc),
            r["snapshot"],
        )
        for r in refs
    )
    for start, end in (
        (monday - dt.timedelta(days=7), monday),
        (previous_month, month),
    ):
        candidates = [snap for date, snap in dated if start <= date.date() < end]
        if candidates:
            keep.add(candidates[-1])
    return keep


def managed_policy(policy):
    return {
        k: v
        for k, v in policy.items()
        if k not in ("encrypted", "retention", "devices")
    }


class Store:
    def __init__(self, root):
        self.root = root.rstrip("/")
        self.local = ":" not in self.root

    def get(self, key, dest):
        safe_name(key)
        if self.local:
            shutil.copyfile(Path(self.root) / key, dest)
        else:
            subprocess.run(
                ["rclone", "copyto", self.root + "/" + key, str(dest)], check=True
            )

    def put(self, path, key, immutable=True):
        safe_name(key)
        if self.local:
            p = Path(self.root) / key
            p.parent.mkdir(parents=True, exist_ok=True)
            if immutable and p.exists():
                raise ArchiveError("Snapshot already exists")
            shutil.copyfile(path, p)
        else:
            subprocess.run(
                ["rclone", "copyto", str(path), self.root + "/" + key]
                + (["--immutable"] if immutable else []),
                check=True,
            )

    def remove_snapshot(self, prefix, snapshot):
        safe_name(prefix)
        if not re.fullmatch(r"[0-9TZ.]+-[0-9a-f]+", snapshot):
            raise ArchiveError("Invalid snapshot identifier")
        key = prefix + "/" + snapshot
        if self.local:
            shutil.rmtree(Path(self.root) / key)
        else:
            subprocess.run(["rclone", "purge", self.root + "/" + key], check=True)

    def list(self, prefix):
        if self.local:
            p = Path(self.root) / prefix
            return sorted(x.name for x in p.iterdir()) if p.exists() else []
        result = subprocess.run(
            ["rclone", "lsf", self.root + "/" + prefix, "--max-depth", "1"],
            check=True,
            capture_output=True,
            text=True,
        )
        return [x.rstrip("/") for x in result.stdout.splitlines()]

    def completed(self, prefix):
        if self.local:
            root = Path(self.root) / prefix
            return sorted(p.parent.name for p in root.glob("*/complete.json"))
        result = subprocess.run(
            [
                "rclone",
                "lsf",
                self.root + "/" + prefix,
                "--recursive",
                "--files-only",
                "--include",
                "complete.json",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        return sorted(
            line.split("/")[0]
            for line in result.stdout.splitlines()
            if line.count("/") == 1 and line.endswith("/complete.json")
        )


class Archives:
    def __init__(self, config, home=None, machine=None, state=None, report=None):
        self.config = config
        self.report = report or (lambda phase, folder, note="": None)
        self.home = Path(home or Path.home()).resolve()
        self.machine = host_name(machine or socket.gethostname().split(".")[0])
        self.master = host_name(config["master"])
        self.state = Path(state or self.home / ".local/state/device-archives")
        self.state.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.state.chmod(0o700)
        self.store = Store(config["remote"])

    @contextlib.contextmanager
    def locked(self):
        with open(self.state / "lock", "a") as stream:
            try:
                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as exc:
                raise ArchiveError("Another archive operation is running") from exc
            yield

    def policy(self, folder):
        safe_name(folder)
        if folder not in self.config["folders"]:
            raise ArchiveError(f"Folder not configured: {folder}")
        return self.config["folders"][folder]

    def devices(self, selectors):
        if not isinstance(selectors, list) or not selectors:
            raise ArchiveError("Device selectors must be a nonempty list")
        groups = self.config.get("device_groups", {})
        result = set()
        for selector in selectors:
            if selector in groups:
                members = groups[selector]
                if not isinstance(members, list) or not members:
                    raise ArchiveError("Device groups must contain hostnames")
                result.update(host_name(member) for member in members)
            else:
                result.add(host_name(selector))
        return sorted(result)

    def device(self, selector):
        devices = self.devices([selector])
        if len(devices) != 1:
            raise ArchiveError("Choose one device, not a multi-device group")
        return devices[0]

    def eligible(self, folder, machine=None):
        machine = machine or self.machine
        p = self.policy(folder)
        if "devices" in p and machine not in self.devices(p["devices"]):
            return False
        return p["mode"] not in ("desktop", "master-backup") or machine == self.master

    def prefix(self, folder, machine):
        self.policy(folder)
        return host_name(machine) + "/" + folder

    def encrypt(self, source, dest):
        subprocess.run(
            ["age", "-r", self.config["recipient"], "-o", str(dest), str(source)],
            check=True,
        )

    def decrypt(self, source, dest):
        subprocess.run(
            [
                "age",
                "-d",
                "-i",
                str(Path(self.config["identity"]).expanduser()),
                "-o",
                str(dest),
                str(source),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )

    def baseline_path(self, folder, source):
        return self.state / "sync" / source / (digest(folder) + ".json")

    def ready(self):
        if self.machine == self.master:
            return
        required = self.config.get("initial_sync_required")
        if required is not None and (
            not required or self.machine not in self.devices(required)
        ):
            return
        missing = [
            f
            for f, p in self.config["folders"].items()
            if p["mode"] in ("sync", "master")
            and self.eligible(f)
            and not self.baseline_path(f, self.master).exists()
        ]
        if missing:
            raise ArchiveError(
                "First backup blocked: sync these folders from fi first: "
                + ", ".join(missing)
            )

    def backup(self, folder, force=False):
        p = self.policy(folder)
        if not self.eligible(folder):
            raise ArchiveError(f"{folder} is not assigned to {self.machine}")
        self.ready()
        snapshot = (
            dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ-")
            + uuid.uuid4().hex[:12]
        )
        prefix = self.prefix(folder, self.machine)
        with tempfile.TemporaryDirectory(prefix="snapshot-", dir=self.state) as tmp:
            tmp = Path(tmp)
            last_path = self.state / "manifests" / (digest(folder) + ".json")
            receipt_path = self.state / "backups" / (digest(folder) + ".json")
            previous = read_json(last_path) if last_path.exists() else None
            receipt = read_json(receipt_path) if receipt_path.exists() else None
            self.report(
                "CHECK", folder, "compare content hashes with last verified backup"
            )
            entries, root_mode = fingerprint_tree(
                self.home / folder,
                p,
                self.state / "hash-cache" / (digest(folder) + ".json"),
                tmp,
            )
            self.report("METRICS", folder, {"entries": len(entries)})
            same = (
                not force
                and previous is not None
                and receipt is not None
                and previous["machine"] == self.machine
                and previous["folder"] == folder
                and previous["snapshot"] == receipt["snapshot"]
                and previous["entries"] == entries
                and previous["root_mode"] == root_mode
                and backup_policy(previous["policy"]) == backup_policy(p)
                and (
                    not p.get("encrypted", True)
                    or receipt.get("recipient", self.config["recipient"])
                    == self.config["recipient"]
                )
            )
            if same:
                # Local receipts alone must not hide a missing or replaced remote backup.
                self.report("CHECK", folder, "confirm existing remote snapshot")
                try:
                    self.store.get(
                        f"{prefix}/latest.json", tmp / "existing-latest.json"
                    )
                    self.store.get(
                        f"{prefix}/{receipt['snapshot']}/complete.json",
                        tmp / "existing-complete.json",
                    )
                    same = (
                        read_json(tmp / "existing-latest.json")
                        == receipt
                        == read_json(tmp / "existing-complete.json")
                    )
                    suffix = ".age" if receipt.get("encrypted", True) else ""
                    same = same and {
                        "manifest.json" + suffix,
                        "tree.tar.zst" + suffix,
                    }.issubset(self.store.list(f"{prefix}/{receipt['snapshot']}"))
                except (FileNotFoundError, subprocess.CalledProcessError):
                    same = False
            if same:
                if "retention" in p:
                    self.report("PRUNE", folder, p["retention"])
                    self.prune(folder, apply=True, verified=receipt)
                atomic_json(
                    self.state / "checks" / (digest(folder) + ".json"),
                    {
                        "checked_at": now(),
                        "snapshot": receipt["snapshot"],
                        "unchanged": True,
                    },
                )
                self.report("SKIP", folder, {"entries": len(entries), "size": None})
                return receipt | {"skipped": True}
            tree = tmp / "tree"
            self.report("STAGE", folder, "copy files and snapshot SQLite")
            entries, dbs = stage_tree(self.home / folder, tree, p)
            previous_entries = previous["entries"] if previous else {}
            manifest = {
                "version": 2,
                "machine": self.machine,
                "folder": folder,
                "snapshot": snapshot,
                "created_at": now(),
                "policy": p,
                "root_mode": stat.S_IMODE((self.home / folder).stat().st_mode) & 0o777,
                "entries": entries,
                "sqlite": dbs,
                "previous_snapshot": previous["snapshot"] if previous else None,
                "changes": {
                    "added": sorted(set(entries) - set(previous_entries)),
                    "removed": sorted(set(previous_entries) - set(entries)),
                    "modified": sorted(
                        k
                        for k in set(entries) & set(previous_entries)
                        if entries[k] != previous_entries[k]
                    ),
                },
            }
            atomic_json(tmp / "manifest.json", manifest)
            self.report("PACK", folder, f"{len(entries)} entries · {len(dbs)} SQLite")
            pack(tree, entries, tmp / "tree.tar.zst")
            suffix = ".age" if p.get("encrypted", True) else ""
            if suffix:
                self.report("ENCRYPT", folder, "age")
                for name in ("manifest.json", "tree.tar.zst"):
                    self.encrypt(tmp / name, tmp / (name + suffix))
            ref = {
                "version": 2,
                "machine": self.machine,
                "folder": folder,
                "snapshot": snapshot,
                "created_at": manifest["created_at"],
                "encrypted": bool(suffix),
                "recipient": self.config["recipient"] if suffix else None,
                "manifest_sha256": file_hash(tmp / ("manifest.json" + suffix)),
                "archive_sha256": file_hash(tmp / ("tree.tar.zst" + suffix)),
            }
            self.report(
                "METRICS",
                folder,
                {"size": (tmp / ("tree.tar.zst" + suffix)).stat().st_size},
            )
            # No latest pointer or local success receipt until remote readback is verified.
            for name, checksum in (
                ("manifest.json" + suffix, ref["manifest_sha256"]),
                ("tree.tar.zst" + suffix, ref["archive_sha256"]),
            ):
                key = f"{prefix}/{snapshot}/{name}"
                size = (tmp / name).stat().st_size / (1024 * 1024)
                self.report("UPLOAD", folder, f"{name} · {size:.1f} MiB")
                self.store.put(tmp / name, key)
                self.report("CHECK", folder, f"download {name} · {size:.1f} MiB")
                self.store.get(key, tmp / ("readback-" + name))
                if file_hash(tmp / ("readback-" + name)) != checksum:
                    raise ArchiveError("Remote readback checksum mismatch")
            self.report("VERIFY", folder, "extract and check every file")
            decode = self.decrypt if suffix else shutil.copyfile
            decode(tmp / ("readback-tree.tar.zst" + suffix), tmp / "verify.tar.zst")
            unpack(tmp / "verify.tar.zst", entries, tmp / "verified")
            decode(tmp / ("readback-manifest.json" + suffix), tmp / "verify.json")
            if read_json(tmp / "verify.json") != manifest:
                raise ArchiveError("Remote manifest differs")
            self.report("PUBLISH", folder, "completion and latest pointers")
            atomic_json(tmp / "ref.json", ref)
            self.store.put(tmp / "ref.json", f"{prefix}/{snapshot}/complete.json")
            self.store.put(tmp / "ref.json", f"{prefix}/latest.json", immutable=False)
            self.store.get(f"{prefix}/latest.json", tmp / "latest.json")
            if read_json(tmp / "latest.json") != ref:
                raise ArchiveError("Latest pointer verification failed")
            atomic_json(self.state / "backups" / (digest(folder) + ".json"), ref)
            atomic_json(last_path, manifest)
            if "retention" in p:
                self.report("PRUNE", folder, p["retention"])
                self.prune(folder, apply=True, verified=ref)
            self.report(
                "DONE",
                folder,
                {
                    "entries": len(entries),
                    "size": (tmp / ("tree.tar.zst" + suffix)).stat().st_size,
                },
            )
            return ref

    def prune(self, folder, apply=False, verified=None):
        if not self.eligible(folder):
            raise ArchiveError("Cannot prune another machine's backups")
        policy = self.policy(folder)["retention"]
        prefix = self.prefix(folder, self.machine)
        with tempfile.TemporaryDirectory(dir=self.state) as tmp:
            tmp = Path(tmp)
            # Standalone pruning verifies the entire latest tree before deleting history.
            if verified is None:
                verified, _ = self.fetch(folder, self.machine, tmp, tree=True)
            self.store.get(prefix + "/latest.json", tmp / "current.json")
            if read_json(tmp / "current.json") != verified:
                raise ArchiveError("Latest changed before retention")
            refs = []
            for snapshot in self.store.completed(prefix):
                if not re.fullmatch(r"[0-9TZ.]+-[0-9a-f]+", snapshot):
                    raise ArchiveError("Invalid snapshot identifier")
                self.store.get(
                    f"{prefix}/{snapshot}/complete.json", tmp / "candidate.json"
                )
                ref = read_json(tmp / "candidate.json")
                if (ref["snapshot"], ref["machine"], ref["folder"]) != (
                    snapshot,
                    self.machine,
                    folder,
                ):
                    raise ArchiveError("Retention snapshot identity mismatch")
                refs.append(ref)
            if verified["snapshot"] not in {r["snapshot"] for r in refs}:
                raise ArchiveError("Latest snapshot is not complete")
            keep = retention_keep(refs, verified["snapshot"], policy)
            remove = sorted(r["snapshot"] for r in refs if r["snapshot"] not in keep)
            report = {
                "folder": folder,
                "keep": sorted(keep),
                "remove": remove,
                "applied": apply,
            }
            if apply:
                atomic_json(
                    self.state / "retention" / (digest(folder) + ".json"), report
                )
                for snapshot in remove:
                    self.store.remove_snapshot(prefix, snapshot)
            return report

    def fetch(self, folder, source, dest, snapshot=None, tree=False):
        prefix = self.prefix(folder, source)
        if snapshot:
            if not re.fullmatch(r"[0-9TZ.]+-[0-9a-f]+", snapshot):
                raise ArchiveError("Invalid snapshot identifier")
            key = f"{prefix}/{snapshot}/complete.json"
        else:
            key = f"{prefix}/latest.json"
        self.store.get(key, dest / "ref.json")
        ref = read_json(dest / "ref.json")
        if ref["machine"] != source or ref["folder"] != folder or ref["version"] != 2:
            raise ArchiveError("Snapshot identity mismatch")
        snap = ref["snapshot"]
        if not re.fullmatch(r"[0-9TZ.]+-[0-9a-f]+", snap):
            raise ArchiveError("Invalid remote snapshot identifier")
        for name, checksum in [("manifest.json", ref["manifest_sha256"])] + (
            [("tree.tar.zst", ref["archive_sha256"])] if tree else []
        ):
            suffix = ".age" if ref.get("encrypted", True) else ""
            self.store.get(f"{prefix}/{snap}/{name}{suffix}", dest / (name + suffix))
            if file_hash(dest / (name + suffix)) != checksum:
                raise ArchiveError("Snapshot checksum mismatch")
            if suffix:
                self.decrypt(dest / (name + suffix), dest / name)
        manifest = read_json(dest / "manifest.json")
        if any(
            manifest[k] != ref[k] for k in ("machine", "folder", "snapshot", "version")
        ):
            raise ArchiveError("Manifest identity mismatch")
        check_tree(manifest["entries"])
        if tree:
            unpack(dest / "tree.tar.zst", manifest["entries"], dest / "tree")
        return ref, manifest

    def plan(self, folder, source, snapshot=None, restore=False):
        p = self.policy(folder)
        if not restore and not self.eligible(folder):
            raise ArchiveError(f"{folder} is not assigned to {self.machine}")
        if source == self.machine and not restore:
            raise ArchiveError("Choose a different source machine")
        if not restore and p["mode"] not in ("sync", "master"):
            raise ArchiveError(
                "This folder is backup-only; use restore into an empty directory"
            )
        if (
            p["mode"] in ("master", "master-backup", "desktop")
            and source != self.master
        ):
            raise ArchiveError(f"{folder} only accepts source {self.master}")
        if p["mode"] == "master" and self.machine == self.master:
            raise ArchiveError("Cannot overwrite the master through sync")
        with tempfile.TemporaryDirectory(dir=self.state) as tmp:
            ref, manifest = self.fetch(folder, source, Path(tmp), snapshot)
        baseline = self.baseline_path(folder, source)
        prior = read_json(baseline) if baseline.exists() else None
        base = prior["entries"] if prior else {}
        local = scan(self.home / folder, p)
        incoming = manifest["entries"]
        if managed_policy(manifest["policy"]) != managed_policy(p):
            raise ArchiveError(
                "Snapshot policy differs; create a backup under the current policy first"
            )
        master = p["mode"] == "master"
        desired, changes, conflicts = merge_entries(base, local, incoming, master)
        plan = {
            "version": 2,
            "machine": self.machine,
            "folder": folder,
            "source": source,
            "created_at": now(),
            "ref": ref,
            "policy": p,
            "baseline_hash": digest(prior),
            "local": local,
            "base": base,
            "incoming": incoming,
            "root_mode": manifest["root_mode"],
            "changes": changes,
            "conflicts": conflicts,
            "resolutions": {},
            "desired": desired,
        }
        path = self.state / "plans" / (uuid.uuid4().hex + ".json")
        atomic_json(path, plan)
        return path, plan

    def apply(self, path, resolutions=None):
        plan = read_json(path)
        folder, source = plan["folder"], plan["source"]
        host_name(source)
        p = self.policy(folder)
        if plan["machine"] != self.machine or plan["policy"] != p:
            raise ArchiveError("Plan belongs to another machine or policy")
        if (
            not self.eligible(folder)
            or p["mode"] not in ("sync", "master")
            or (
                p["mode"] == "master"
                and (source != self.master or self.machine == self.master)
            )
        ):
            raise ArchiveError("This folder cannot be synchronized from this source")
        baseline = self.baseline_path(folder, source)
        prior = read_json(baseline) if baseline.exists() else None
        if (
            digest(prior) != plan["baseline_hash"]
            or scan(self.home / folder, p) != plan["local"]
        ):
            raise ArchiveError(
                "Local files or sync baseline changed; create a new plan"
            )
        resolutions = resolutions or {}
        reviewable = {name for name in set(plan["local"]) | set(plan["incoming"])
                      if plan["local"].get(name) != plan["incoming"].get(name)} | set(plan["conflicts"])
        if set(resolutions) - reviewable:
            raise ArchiveError("Resolution names do not match this plan's differing files")
        if p["mode"] == "master" and any(value != "source" for value in resolutions.values()):
            raise ArchiveError("Exact mirrors cannot retain local file overrides")
        desired, changes, conflicts = merge_entries(
            plan["base"],
            plan["local"],
            plan["incoming"],
            p["mode"] == "master",
            resolutions,
        )
        if conflicts:
            raise ArchiveError(
                "Review required; resolve each file as local or source: "
                + ", ".join(conflicts)
            )
        target = self.home / folder
        if target.is_symlink():
            raise ArchiveError("Refusing symlinked destination")
        assert_no_writers(target)
        # Keep a complete sibling recovery tree (same filesystem); never merge into a live tree.
        target.parent.mkdir(parents=True, exist_ok=True)
        recovery = target.with_name(
            target.name + ".archive-recovery-" + uuid.uuid4().hex
        )
        with tempfile.TemporaryDirectory(
            prefix=".archive-apply-", dir=target.parent
        ) as work:
            work = Path(work)
            fetchdir = work / "source"
            fetchdir.mkdir(mode=0o700)
            ref, manifest = self.fetch(
                folder, source, fetchdir, plan["ref"]["snapshot"], tree=True
            )
            if ref != plan["ref"] or manifest["entries"] != plan["incoming"]:
                raise ArchiveError("Snapshot changed since planning")
            new = work / "new"

            def ignore_special(directory, names):
                return [
                    n
                    for n in names
                    if stat.S_ISSOCK((Path(directory) / n).lstat().st_mode)
                ]

            if target.exists():
                shutil.copytree(target, new, symlinks=True, ignore=ignore_special)
            else:
                new.mkdir(mode=0o700)
            # Remove changed managed entries, deepest first. Excluded files remain untouched.
            for name in sorted(plan["local"], key=lambda n: (-n.count("/"), n)):
                if plan["local"][name] == desired.get(name):
                    continue
                path_in_new = new / name
                if path_in_new.is_dir() and not path_in_new.is_symlink():
                    if desired.get(name, {}).get("type") == "dir":
                        continue
                    try:
                        path_in_new.rmdir()
                    except OSError as exc:
                        raise ArchiveError(
                            f"Excluded/local files prevent replacing directory {name}"
                        ) from exc
                else:
                    path_in_new.unlink(missing_ok=True)
            for name in sorted(desired, key=lambda n: (n.count("/"), n)):
                e, q = desired[name], new / name
                if e["type"] == "dir":
                    q.mkdir(exist_ok=True)
                elif plan["local"].get(name) != e:
                    if e != manifest["entries"].get(name):
                        raise ArchiveError("Plan refers to unknown content")
                    if e["type"] == "link":
                        q.symlink_to(e["target"])
                    else:
                        shutil.copyfile(fetchdir / "tree" / name, q)
                if e["type"] != "link":
                    q.chmod(e["mode"])
            new.chmod(plan["root_mode"])
            if scan(new, p) != desired or scan(target, p) != plan["local"]:
                raise ArchiveError("Files changed during apply; no live files replaced")
            journal = self.state / "transactions" / (uuid.uuid4().hex + ".json")
            receipt = {
                "folder": folder,
                "source": source,
                "snapshot": ref["snapshot"],
                "recovery": str(recovery),
                "status": "prepared",
                "created_at": now(),
            }
            atomic_json(journal, receipt)
            existed = target.exists()
            assert_no_writers(target)
            if existed:
                os.replace(target, recovery)
            try:
                os.replace(new, target)
                restorecon = shutil.which("restorecon")
                if restorecon and Path("/sys/fs/selinux/enforce").exists():
                    subprocess.run([restorecon, "-R", str(target)], check=True)
                if scan(target, p) != desired:
                    raise ArchiveError("Post-apply verification failed")
            except Exception:
                if target.exists():
                    os.replace(target, work / "failed")
                if existed:
                    os.replace(recovery, target)
                raise
            atomic_json(
                baseline,
                {
                    "version": 2,
                    "folder": folder,
                    "source": source,
                    "snapshot": ref["snapshot"],
                    "last_synced_at": now(),
                    "entries": manifest["entries"],
                },
            )
            receipt["status"] = "complete"
            atomic_json(journal, receipt)
            return receipt

    def restore(self, folder, source, dest, snapshot=None):
        p = self.policy(folder)
        if (
            p["mode"] in ("desktop", "master-backup", "master")
            and source != self.master
        ):
            raise ArchiveError("This folder must be restored from fi")
        dest = dest.expanduser().absolute()
        if dest.exists() or dest.is_symlink():
            raise ArchiveError(
                "Restore destination must not exist; restore to a new staging directory"
            )
        # Restoration always stages, especially databases: never overwrite a running backend.
        with tempfile.TemporaryDirectory(dir=self.state) as tmp:
            ref, manifest = self.fetch(folder, source, Path(tmp), snapshot, tree=True)
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(Path(tmp) / "tree", dest, symlinks=True)
            dest.chmod(manifest["root_mode"])
            if scan(dest) != manifest["entries"]:
                raise ArchiveError("Restore verification failed")
        return ref

    def diff(self, folder, left, right, file=None):
        with tempfile.TemporaryDirectory(dir=self.state) as tmp:
            tmp = Path(tmp)
            a, b = tmp / "left", tmp / "right"
            a.mkdir()
            b.mkdir()
            lr, lm = self.fetch(folder, left, a, tree=bool(file))
            rr, rm = self.fetch(folder, right, b, tree=bool(file))
            result = {"folder": folder, "left": lr, "right": rr, "changes": []}
            for name in sorted(set(lm["entries"]) | set(rm["entries"])):
                local_entry, r = lm["entries"].get(name), rm["entries"].get(name)
                if local_entry != r:
                    result["changes"].append(
                        {
                            "path": name,
                            "left": local_entry,
                            "right": r,
                            "change": "right-only"
                            if local_entry is None
                            else "left-only"
                            if r is None
                            else "different",
                        }
                    )
            if file:
                safe_name(file)
                contents = []
                for tree, manifest in ((a / "tree", lm), (b / "tree", rm)):
                    entry = manifest["entries"].get(file)
                    if entry is None:
                        contents.append("")
                        continue
                    if entry["type"] != "file" or entry["size"] > 1024 * 1024:
                        raise ArchiveError(
                            "Text diff requires regular files of at most 1 MiB"
                        )
                    raw = (tree / file).read_bytes()
                    if b"\0" in raw:
                        raise ArchiveError(
                            "Binary file; compare its manifest hash instead"
                        )
                    contents.append(raw.decode("utf-8"))
                result["text_diff"] = "".join(
                    difflib.unified_diff(
                        contents[0].splitlines(True),
                        contents[1].splitlines(True),
                        fromfile=left + "/" + file,
                        tofile=right + "/" + file,
                    )
                )
            return result
