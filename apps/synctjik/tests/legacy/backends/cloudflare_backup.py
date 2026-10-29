#!/usr/bin/env python3
"""Export Cloudflare DNS and KV plus R2 object contents to an encrypted Restic repo."""
from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen


MOUNT_UUID = ""
MOUNT_POINT = Path("/nonexistent")
REPOSITORY = Path("/nonexistent")
ACCOUNT_ID = ""
CONFIG = Path.home() / ".config/private-storage"
STATE = Path.home() / ".local/state/cloudflare-restic"
RESTIC_PASSWORD = CONFIG / "restic-password"
CLOUDFLARE_TOKEN = CONFIG / "cloudflare-backup-token"
CLOUDFLARE_CONFIG_TOKEN = CONFIG / "cloudflare-config-token"
RCLONE_CONFIG = Path.home() / ".config/rclone/rclone.conf"
R2_REMOTE = ""
API = "https://api.cloudflare.com/client/v4"
EXPECTED_R2_BUCKETS = 0
MAX_BACKUP_ATTEMPTS = 2


class BackupError(RuntimeError):
    pass


def run(args: list[str], *, capture: bool = False, **kwargs):
    return subprocess.run(
        [str(item) for item in args], check=True,
        capture_output=capture, text=capture, **kwargs,
    )


def verify_mount(*, require_writable: bool = False) -> None:
    """Require the exact UUID mounted at the approved path; never infer by name."""
    if MOUNT_POINT.is_symlink() or not MOUNT_POINT.is_dir() or not os.path.ismount(MOUNT_POINT):
        raise BackupError("Approved backup drive is not mounted at its exact mount point")
    try:
        result = run(["findmnt", "--noheadings", "--raw", "--mountpoint",
                      MOUNT_POINT, "--output", "UUID,OPTIONS"], capture=True)
    except (subprocess.CalledProcessError, FileNotFoundError) as error:
        raise BackupError("Could not verify the backup drive UUID") from error
    fields = result.stdout.strip().split()
    if not fields or fields[0] != MOUNT_UUID:
        raise BackupError("Mounted device UUID does not match the approved backup drive")
    if require_writable and (len(fields) < 2 or "rw" not in fields[1].split(",")):
        raise BackupError("Approved backup drive is mounted read-only")
    if MOUNT_POINT.resolve() != MOUNT_POINT:
        raise BackupError("Backup mount path resolved through a symlink")


def verify_repository_path() -> None:
    verify_mount(require_writable=True)
    parent = REPOSITORY.parent
    if parent.is_symlink() or (parent.exists() and parent.resolve() != parent):
        raise BackupError("Backup parent path must be a real directory on the approved mount")
    if parent.exists() and not parent.is_dir():
        raise BackupError("Backup parent path is not a directory")
    if REPOSITORY.is_symlink() or (REPOSITORY.exists() and REPOSITORY.resolve() != REPOSITORY):
        raise BackupError("Restic repository path must not resolve through a symlink")


def safe_secret_file(path: Path) -> str:
    if path.is_symlink():
        raise BackupError(f"Credential path must not be a symlink: {path.name}")
    try:
        info = path.stat()
    except OSError as error:
        raise BackupError(f"Required private credential file is unavailable: {path.name}") from error
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise BackupError(f"Credential file must be a regular owner-only file: {path.name}")
    value = path.read_text().strip()
    if not value:
        raise BackupError(f"Credential file is empty: {path.name}")
    return value


def write_json(path: Path, data: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("w", encoding="utf-8") as stream:
        os.chmod(temporary, 0o600)
        json.dump(data, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


def cloudflare_request(token: str, path: str, query: dict | None = None) -> tuple[object, str | None]:
    url = API + path
    if query:
        url += "?" + urlencode(query)
    request = Request(url, headers={"Authorization": f"Bearer {token}",
                                    "Accept": "application/json"})
    try:
        with urlopen(request, timeout=60) as response:
            payload = json.loads(response.read())
    except HTTPError as error:
        # Do not print provider response bodies: they can echo submitted values.
        raise BackupError(f"Cloudflare API request failed for {path} with HTTP {error.code}") from None
    except (URLError, TimeoutError, json.JSONDecodeError) as error:
        raise BackupError(f"Cloudflare API request failed: {type(error).__name__}") from None
    if not payload.get("success"):
        raise BackupError("Cloudflare API returned an unsuccessful response")
    return payload.get("result"), payload.get("result_info", {}).get("cursor")


def paged_results(token: str, path: str, *, query: dict | None = None) -> list[dict]:
    params = dict(query or {})
    params.setdefault("per_page", 100)
    page = 1
    output: list[dict] = []
    while True:
        params["page"] = page
        result, _ = cloudflare_request(token, path, params)
        if not isinstance(result, list):
            raise BackupError("Cloudflare API returned an unexpected list response")
        output.extend(result)
        if len(result) < params["per_page"]:
            return output
        page += 1


def export_zones(token: str, root: Path, *, dns_token: str | None = None) -> tuple[int, int]:
    zones = paged_results(token, "/zones", query={"account.id": ACCOUNT_ID})
    if any(zone.get("account", {}).get("id") != ACCOUNT_ID for zone in zones):
        raise BackupError("Cloudflare returned a zone outside the approved account")
    dns_total = 0
    for zone in zones:
        zone_id = str(zone["id"])
        settings, _ = cloudflare_request(token, f"/zones/{quote(zone_id, safe='')}/settings")
        records = paged_results(dns_token or token, f"/zones/{quote(zone_id, safe='')}/dns_records")
        write_json(root / "dns" / zone_id / "zone.json", zone)
        write_json(root / "dns" / zone_id / "settings.json", settings)
        write_json(root / "dns" / zone_id / "records.json", records)
        dns_total += len(records)
    if not zones:
        raise BackupError("Cloudflare returned no zones; refusing an empty DNS export")
    write_json(root / "dns" / "inventory.json", {
        "exported_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "zone_count": len(zones),
        "dns_record_count": dns_total,
        "zones": [{"id": z.get("id"), "name": z.get("name"), "status": z.get("status")}
                  for z in zones],
    })
    return len(zones), dns_total


def export_kv(token: str, account_id: str, root: Path) -> tuple[int, int]:
    namespaces = paged_results(
        token, f"/accounts/{quote(account_id, safe='')}/storage/kv/namespaces")
    if not namespaces:
        raise BackupError("Cloudflare returned no KV namespaces; refusing an empty KV export")
    key_total = 0
    byte_total = 0
    for namespace in namespaces:
        namespace_id = str(namespace["id"])
        listing: list[dict] = []
        cursor: str | None = None
        while True:
            params = {"limit": 1000}
            if cursor:
                params["cursor"] = cursor
            keys, next_cursor = cloudflare_request(
                token, f"/accounts/{quote(account_id, safe='')}/storage/kv/namespaces/"
                f"{quote(namespace_id, safe='')}/keys", params)
            if not isinstance(keys, list):
                raise BackupError("Cloudflare API returned an unexpected KV key list")
            listing.extend(keys)
            if not next_cursor:
                break
            cursor = str(next_cursor)
        namespace_dir = root / "kv" / namespace_id
        values = []
        for item in listing:
            key = str(item["name"])
            path = (f"/accounts/{quote(account_id, safe='')}/storage/kv/namespaces/"
                    f"{quote(namespace_id, safe='')}/values/{quote(key, safe='')}")
            request = Request(API + path, headers={"Authorization": f"Bearer {token}"})
            try:
                with urlopen(request, timeout=120) as response:
                    value = response.read()
            except HTTPError as error:
                raise BackupError(f"Cloudflare KV value export failed with HTTP {error.code}") from None
            except (URLError, TimeoutError) as error:
                raise BackupError(f"Cloudflare KV value export failed: {type(error).__name__}") from None
            digest = hashlib.sha256(value).hexdigest()
            filename = digest + ".value"
            namespace_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
            target = namespace_dir / filename
            target.write_bytes(value)
            os.chmod(target, 0o600)
            values.append({"key": key, "metadata": item.get("metadata"),
                           "expiration": item.get("expiration"),
                           "expiration_ttl": item.get("expiration_ttl"),
                           "file": filename, "size": len(value), "sha256": digest})
            byte_total += len(value)
        write_json(namespace_dir / "manifest.json", {
            "namespace": namespace, "key_count": len(values), "values": values})
        key_total += len(values)
    write_json(root / "kv" / "inventory.json", {
        "exported_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "namespace_count": len(namespaces), "key_count": key_total,
        "value_bytes": byte_total,
        "namespaces": [{"id": n.get("id"), "title": n.get("title")} for n in namespaces],
    })
    return len(namespaces), key_total


def export_r2_inventory(root: Path) -> tuple[int, int, int]:
    result = run(["rclone", "size", R2_REMOTE, "--json", "--config", RCLONE_CONFIG], capture=True)
    summary = json.loads(result.stdout)
    buckets_result = run(["rclone", "lsd", R2_REMOTE, "--config", RCLONE_CONFIG], capture=True)
    buckets = [line.rsplit(None, 1)[-1] for line in buckets_result.stdout.splitlines()
               if line.strip()]
    if len(buckets) < EXPECTED_R2_BUCKETS:
        raise BackupError(f"R2 inventory is incomplete: expected at least {EXPECTED_R2_BUCKETS} buckets, found {len(buckets)}")
    write_json(root / "r2" / "inventory.json", {
        "exported_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "bucket_count": len(buckets), "object_count": summary.get("count"),
        "bytes": summary.get("bytes"), "buckets": buckets,
        "metadata_note": "Object bodies, names, and server modification timestamps are read through the read-only rclone mount. "
                         "Custom S3 metadata is not guaranteed to be restored.",
    })
    return len(buckets), int(summary.get("count", 0)), int(summary.get("bytes", 0))


@contextlib.contextmanager
def mounted_r2(path: Path, log_path: Path):
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    log = log_path.open("ab")
    process = subprocess.Popen([
        "rclone", "mount", R2_REMOTE, str(path), "--config", str(RCLONE_CONFIG),
        "--read-only", "--vfs-cache-mode", "off", "--dir-cache-time", "24h",
        "--vfs-refresh", "--fast-list", "--use-server-modtime",
        "--poll-interval", "0", "--log-level", "ERROR",
    ], stdout=subprocess.DEVNULL, stderr=log, start_new_session=True)
    try:
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise BackupError("Could not mount the R2 source; see private service logs")
            if os.path.ismount(path):
                break
            time.sleep(0.25)
        else:
            raise BackupError("Timed out waiting for the read-only R2 mount")
        yield
    finally:
        if os.path.ismount(path):
            unmount = shutil.which("fusermount3") or shutil.which("fusermount")
            if unmount:
                subprocess.run([unmount, "-u", str(path)], stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, check=False)
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        log.close()


def restic_env() -> dict[str, str]:
    env = os.environ.copy()
    env["RESTIC_REPOSITORY"] = str(REPOSITORY)
    env["RESTIC_PASSWORD_FILE"] = str(RESTIC_PASSWORD)
    env.setdefault("RESTIC_PROGRESS_FPS", "0.1")
    return env


def restic(args: list[str]) -> subprocess.CompletedProcess:
    verify_repository_path()
    return run(["restic", *args], env=restic_env())


def export_sources(work_root: Path, token: str, dns_token: str) -> tuple[int, ...]:
    # A retry must not retain exports of keys/zones deleted since the last pass.
    for name in ("dns", "kv", "r2"):
        directory = work_root / name
        if directory.exists():
            shutil.rmtree(directory)
    print("Exporting Cloudflare DNS and zone settings", flush=True)
    zones, records = export_zones(token, work_root, dns_token=dns_token)
    print(f"DNS export ready: zones={zones}, records={records}", flush=True)
    namespaces, keys = export_kv(token, ACCOUNT_ID, work_root)
    print(f"KV export ready: namespaces={namespaces}, keys={keys}", flush=True)
    buckets, objects, r2_bytes = export_r2_inventory(work_root)
    print(f"R2 inventory ready: buckets={buckets}, objects={objects}, bytes={r2_bytes}", flush=True)
    return zones, records, namespaces, keys, buckets, objects, r2_bytes


def backup(dry_run: bool = False) -> None:
    os.umask(0o077)
    if dry_run:
        verify_mount()
    else:
        verify_repository_path()
    if not RCLONE_CONFIG.is_file():
        raise BackupError("Private R2 rclone configuration is unavailable")
    if RCLONE_CONFIG.stat().st_mode & 0o077:
        raise BackupError("R2 rclone configuration must be owner-only")
    if not dry_run:
        password = safe_secret_file(RESTIC_PASSWORD)
        del password
    dns_token = safe_secret_file(CLOUDFLARE_TOKEN)
    token = safe_secret_file(CLOUDFLARE_CONFIG_TOKEN)
    STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(STATE, 0o700)
    with (STATE / "backup.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise BackupError("A Cloudflare Restic backup is already running") from error
        work_root = Path(tempfile.mkdtemp(prefix="cloudflare-restic-", dir="/tmp"))
        os.chmod(work_root, 0o700)
        try:
            zones, records, namespaces, keys, buckets, objects, r2_bytes = export_sources(
                work_root, token, dns_token)
            if dry_run:
                print(f"Dry run ready: zones={zones}, dns_records={records}, "
                      f"kv_namespaces={namespaces}, kv_keys={keys}, r2_buckets={buckets}, "
                      f"r2_objects={objects}, r2_bytes={r2_bytes}")
                return
            repo_config = REPOSITORY / "config"
            verify_repository_path()
            if shutil.disk_usage(MOUNT_POINT).free < r2_bytes + 1024 ** 3:
                raise BackupError("Backup drive lacks room for source data plus 1 GiB headroom")
            if not repo_config.exists():
                if REPOSITORY.exists() and any(REPOSITORY.iterdir()):
                    raise BackupError("Repository target exists but is not an initialized Restic repository")
                REPOSITORY.mkdir(parents=True, exist_ok=True, mode=0o700)
                restic(["init"])
            mount_path = work_root / "r2-mount"
            for attempt in range(1, MAX_BACKUP_ATTEMPTS + 1):
                if attempt > 1:
                    zones, records, namespaces, keys, buckets, objects, r2_bytes = export_sources(
                        work_root, token, dns_token)
                    verify_repository_path()
                    if shutil.disk_usage(MOUNT_POINT).free < r2_bytes + 1024 ** 3:
                        raise BackupError("Backup drive lacks room for source data plus 1 GiB headroom")
                try:
                    # Remount each attempt: an object replaced during a long run
                    # can leave a stale size in the previous mount's dir cache.
                    with mounted_r2(mount_path, STATE / "rclone-mount.log"):
                        if not any(mount_path.iterdir()):
                            raise BackupError("R2 mount returned an empty root; refusing an incomplete snapshot")
                        verify_mount()
                        print(f"Backing up DNS, KV, and R2 (attempt {attempt}/{MAX_BACKUP_ATTEMPTS})", flush=True)
                        # The path stays fixed within this invocation. Restic can
                        # reuse unchanged files in its partial parent snapshot;
                        # remounting assigns new synthetic inode/ctime values.
                        restic(["backup", "--json", "--ignore-inode", "--read-concurrency", "32",
                                "--tag", "cloudflare", "--host", "cloudflare", str(work_root)])
                    break
                except subprocess.CalledProcessError as error:
                    if error.returncode != 3 or attempt == MAX_BACKUP_ATTEMPTS:
                        raise
                    print("Incomplete source read; retaining partial snapshot and retrying once "
                          "with fresh exports and a new read-only R2 mount", flush=True)
            restic(["check"])
            verify_repository_path()
            snapshots = subprocess.run(["restic", "snapshots", "--json"],
                                       env=restic_env(), check=True, capture_output=True,
                                       text=True)
            matching = [s for s in json.loads(snapshots.stdout)
                        if s.get("hostname") == "cloudflare" and "cloudflare" in s.get("tags", [])]
            if not matching:
                raise BackupError("Restic did not report the expected Cloudflare snapshot")
            latest = max(matching, key=lambda snapshot: snapshot["time"])
            retention_marker = STATE / "retention-enabled.json"
            if retention_marker.exists():
                restic(["forget", "--host", "cloudflare", "--tag", "cloudflare",
                        "--group-by", "host,tags", "--keep-weekly", "1",
                        "--keep-monthly", "1", "--prune"])
            else:
                # Setup never prunes. Enable retention only after the first
                # successful snapshot and integrity check; existing contents
                # are retained until a later scheduled run.
                write_json(retention_marker, {
                    "enabled_after": dt.datetime.now(dt.timezone.utc).isoformat(),
                    "group_by": "host,tags", "keep_weekly": 1, "keep_monthly": 1,
                })
            success = {
                "completed": dt.datetime.now(dt.timezone.utc).isoformat(),
                "snapshot_id": latest["id"],
                "zones": zones, "dns_records": records,
                "kv_namespaces": namespaces, "kv_keys": keys,
                "r2_buckets": buckets, "r2_objects": objects, "r2_bytes": r2_bytes,
            }
            write_json(STATE / "last-success.json", success)
            try:
                (STATE / "last-failure.json").unlink()
            except FileNotFoundError:
                pass
            print("Cloudflare Restic backup complete: " + json.dumps(success, sort_keys=True))
        finally:
            shutil.rmtree(work_root, ignore_errors=True)


def load_policy(path):
    """Load non-secret device policy; credentials remain in separate files."""
    values = json.loads(Path(path).read_text())
    paths = ("mount_point", "repository", "restic_password", "cloudflare_token",
             "cloudflare_config_token", "rclone_config")
    for key in ("mount_uuid", "account_id", "r2_remote", *paths):
        if not isinstance(values.get(key), str) or not values[key].strip():
            raise BackupError("Cloudflare policy requires " + key)
    if not isinstance(values.get("expected_r2_buckets"), int) or values["expected_r2_buckets"] < 1:
        raise BackupError("Cloudflare policy requires a positive expected_r2_buckets")
    configured = {}
    for key in paths:
        value = Path(values[key]).expanduser()
        if not value.is_absolute() or ".." in value.parts:
            raise BackupError("Cloudflare policy paths must be absolute")
        configured[key.upper()] = value
    if configured["REPOSITORY"] == configured["MOUNT_POINT"] or not configured["REPOSITORY"].is_relative_to(configured["MOUNT_POINT"]):
        raise BackupError("Cloudflare repository must be inside the approved mount")
    configured.update({key.upper(): values[key] for key in
                       ("mount_uuid", "account_id", "r2_remote", "expected_r2_buckets")})
    globals().update(configured)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("backup", "check"), nargs="?", default="backup")
    parser.add_argument("--policy", type=Path, default=Path.home() / ".config/synctjik/cloudflare.json")
    parser.add_argument("--dry-run", action="store_true",
                        help="export sources to private temporary files, report counts, and do not access Restic")
    args = parser.parse_args()
    try:
        load_policy(args.policy)
        for command in ("rclone", "restic"):
            if not shutil.which(command):
                raise BackupError("Required dependency unavailable: " + command)
        if args.action == "check":
            print("Cloudflare policy and dependencies valid; no remote data accessed")
            return 0
        backup(dry_run=args.dry_run)
        return 0
    except (BackupError, OSError, ValueError, subprocess.CalledProcessError) as error:
        message = error if isinstance(error, BackupError) else type(error).__name__
        details = {}
        if isinstance(error, subprocess.CalledProcessError):
            # Do not include command arguments or captured output: these can
            # contain private object paths or credential-bearing API requests.
            command = error.cmd if isinstance(error.cmd, (list, tuple)) else []
            program = Path(str(command[0])).name if command else "subprocess"
            if program not in {"restic", "rclone", "findmnt"}:
                program = "subprocess"
            operation = str(command[1]) if len(command) > 1 else ""
            if operation not in {"backup", "check", "snapshots", "forget", "init", "size", "lsf", "lsjson"}:
                operation = "operation"
            details = {"program": program, "operation": operation, "exit_code": error.returncode}
            message = f"{program} {operation} failed (exit {error.returncode})"
            if program == "restic" and operation == "backup" and error.returncode == 3:
                message += ": source files unreadable after retry; partial snapshots retained"
        if args.action == "check":
            print(f"Cloudflare check failed: {message}", file=sys.stderr)
            return 1
        STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
        write_json(STATE / "last-failure.json", {
            "failed": dt.datetime.now(dt.timezone.utc).isoformat(),
            "error": str(message),
            **details,
        })
        print(f"Cloudflare Restic backup failed: {message}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
