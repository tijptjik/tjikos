# Synctjik

Verified Dotfiles LFS backups and reviewed device-state synchronization, implemented in Rust.
Tjikup orchestrates prerequisites and readiness; Synctjik owns snapshots, transfer,
merge review, restoration, retention and recovery. Dotjik owns personal policy.
Rclone is a transport dependency for remote stores, not the synchronization engine.

```sh
synctjik enroll --role laptop                 # source and collection selection
synctjik setup                               # native two-pane review, then confirm
synctjik ready                               # nonzero until every required collection is restored
synctjik schedule                            # enable synctjik-dotfiles-lfs-backup.timer
synctjik backup                              # snapshot only; never implicitly synchronize
synctjik plan .ssh --from fi                  # create a pinned plan without applying
synctjik review /private/path/to/plan.json     # review and explicitly confirm application
synctjik restore .gnupg --from fi --to ~/recovery/gnupg
synctjik prune                               # preview; --apply performs retention
synctjik status --json
synctjik --demo                              # synthetic terminal table, no credentials
# tjikup sync forwards the same commands unchanged.
```

`--config`, `--home`, `--machine`, and `--state` support isolated testing and explicit
recovery. No command discovers or scans unrelated credentials. The review terminal
can show sensitive file contents; they are never included in progress events.

## Policy and compatibility

The existing v2 `~/.config/archive-policy.json` remains supported. A local enrollment
at `~/.config/synctjik/device.json` adds the hostname, role, source and selected folder
names without editing the shared policy. Selected folders are requirements, not
best-effort suggestions. Skipping any required restore returns failure; rerunning
setup resumes using saved baselines. Roles control scheduling, not implicit copying
of identities. Selecting `.ssh` or `.gnupg` explicitly chooses to copy that identity.

| Existing mode | Behavior |
| --- | --- |
| `device` | Separate per-device backups; recovery only into a new directory |
| `sync` | Reviewed three-way synchronization, including additions and deletions |
| `master` | Exact managed-file mirror of the declared leader; exclusions remain local |
| `desktop` / `master-backup` | Leader-only backups; no automatic distribution |

Encryption is independent of mode and defaults to true. `encrypted: false` supports
non-secret assets. Folder include/exclude rules, allowlisted file patterns, SQLite
recovery, Telegram theme compaction, and latest/daily-weekly-monthly retention remain
supported. SQLite's online backup API preserves committed WAL; recovery omissions
modify only the disposable copy. Telegram format parsing fails closed on unknown
records and never reads account keys.

V2 tar+zstd archives, age encryption, manifests, completion/latest references, plan
files, sync baselines and receipts are compatible with device-archives. The state
path intentionally stays `~/.local/state/device-archives`, including its operation
lock, so old and new workers cannot overlap. Compatibility command wrappers forward
to the Rust executable. The frozen Python implementation under `tests/legacy` is a
test oracle only and is never shipped as runtime code.

Snapshot publication requires upload, download, digest verification, decryption and
full tree verification before completion/latest pointers and success receipts. The
unchanged path caches regular-file fingerprints while snapshotting SQLite each time
and checking remote completion/object presence. Standalone retention verifies the
latest archive before deletion.

Application rejects stale files, baselines, policy and pinned snapshots, checks for
active writers, preserves excluded files, and retains a sibling `.archive-recovery-*`
tree. A failed post-swap verification rolls back automatically. Transaction journals
record prepared/completed/rolled-back state. Retain recovery trees until the restored
application has been checked; Synctjik never deletes them automatically. After power
loss during a swap, inspect the journal and its target/recovery paths before retrying.

## Bootstrap and scheduling

The Fedora bootstrap downloads checksum-pinned static binaries, prompts to unlock
the encrypted BWS token, obtains the age identity, renders a dedicated R2 transport
config, and runs collection enrollment and restoration before declaring readiness.
`~/.config/synctjik/rclone.conf` is used when present; explicit `RCLONE_CONFIG` wins.
Media mounts and unrelated rclone configuration are untouched.

Scheduling uses `synctjik-dotfiles-lfs-backup.timer`. Migration disables
the old `backup-packages.timer` and refuses to proceed while its archive job
is running; it never stops the job. The
service invokes `synctjik ready` before `synctjik backup`. Desktop/laptop/server slots
are 00:00/00:15/00:30 local time. Existing running workers are not restarted during
installation. Backups still require the user systemd manager to be running.

## Development and release

```sh
cargo test --locked       # synthetic compatibility and safety tests
cargo fmt --check
bash scripts/build.sh
bash scripts/release.sh   # static x86_64 musl executable; build host needs a musl C compiler
```

Tests use Python, zstd, age and Python cryptography only as independent format oracles.
Runtime needs age for encrypted collections and rclone for remote transport; archive
compression, SQLite, terminal review and Telegram parsing are native.
Release installation uses `scripts/fetch-release.sh`, verifies the SHA-256 in the
bootstrap release manifest, and atomically replaces executables. No Rust or C compiler
is installed on the target device. With `TJIKUP_MANAGED_CONFIG=1`, installation
updates only the binary; Dotjik owns the compatibility command wrappers. Standalone
installation creates wrappers relative to the selected prefix.
For an explicit source-checkout repair, run `bun run build --filter=@tjikos/synctjik`
from the repository root, then `bash apps/synctjik/scripts/install.sh --from-build`.
This installs the local release build after checking its protection capabilities;
normal installation continues to use the pinned download.
Never validate a release by applying live private
folders or replacing a pinned running archive worker.

## Private assets and Cloudflare backups

Synctjik owns the existing local-first private-assets and Cloudflare backup
engines, implemented directly in Rust. HTTPS exports use Rustls; transfers use
rclone and Restic. No local or remote Python interpreter executes backup policy.
A small shell helper holds the remote flock; Rust validates its receipts and
stops transfer process groups if the lock connection disappears. Existing unit names,
locks, receipts, credential locations, retention and restore gates are retained.

```sh
synctjik capabilities
synctjik private-assets status
synctjik private-assets sync documents
synctjik private-assets backup --scheduled
synctjik cloudflare check
synctjik cloudflare backup
```

Both `rclone` and `restic` are runtime dependencies; Python is not.
`private-assets crypt-password` uses the `bws` CLI directly, authenticating with
`BWS_ACCESS_TOKEN` or the owner-only `~/.config/bws/environment` file. It invokes
`age` interactively and replaces `crypt.conf.age` only after encryption succeeds.
Private-assets sync uses **rclone bisync** to keep the configured local folders
and encrypted Drive remote in step. Private-assets backup separately creates
**encrypted Restic snapshots on Si**, retaining recoverable history even when
sync propagates a deletion. A successful bisync receipt is not proof that the
Restic backup succeeded; check their separate receipts.
The remote backup host needs POSIX shell, flock and coreutils. Managed
installation provisions them. `cloudflare check` validates policy and dependency
availability without reading credentials, accessing remote data or writing state.
It is not a backup or restore test. `cloudflare backup --dry-run` still exports
remote data into private temporary files; it is not an offline validation command.

Device policy belongs in the dotfiles repository. Cloudflare's non-secret policy
is deployed to `~/.config/synctjik/cloudflare.json`: mount UUID/path, repository,
account ID, expected bucket count, remote and credential-file paths. Private
assets retain `~/.config/private-storage/config.json`, including explicit SSH
transport/host selection and mount roots. No account settings or credentials are
embedded in Synctjik. The compatibility commands `private-storage` and
`cloudflare-restic-backup` delegate to these subcommands.

Cloudflare Resources backs up DNS, Workers KV and R2 objects from Cloudflare. Private assets
are the configured documents, projects, resources and media folders. Old pinned
releases without `native_backends: 1` must not overwrite a capable installed
runner; fresh installations require a compatible published release.

The dotfiles LFS schedule archives enrolled application state to R2 via rclone,
using tar/zstd and age for encrypted collections. It does not use Restic.

## Drive, application recovery and storage protection

`capabilities` reports `protection_backends: 1` for these native engines:

```sh
synctjik drive check --policy /path/to/drive.json
synctjik app-backup check --policy /path/to/application.json
synctjik app-pull check --policy /path/to/pull.json
synctjik snapraid check --policy /path/to/maintenance.json
synctjik snapraid-notify check --policy /path/to/notifications.json
```

These checks parse policy without invoking dependencies or accessing credentials.
Use each command's `--help` for operations. Drive supports backup, snapshots and
verified restore; application backup uses SQLite online snapshots and locked
MariaDB logical dumps. Independent copies retain all selected snapshots and run
Restic's full data check. `app-restore stage` requires an exact snapshot ID and a
new destination, checks its databases, and leaves live services alone. Explicit
`install` requires empty allowlisted destinations and stopped writers;
`import-mariadb` records a durable started marker before any database import.

Drive backups retain owner-only `<source>-rclone-*.log` and
`<source>-restic-*.log` files in the configured state directory. These contain
private provider diagnostics and filenames; terminal errors never print their
contents. Failed or incomplete snapshots do not produce a success receipt or
trigger retention.

SnapRAID checks disk identities and layout, enforces deletion limits, and retains
pause ownership for interrupted-writer recovery. `status` returns 75 when busy.
Sync/scrub are real maintenance operations: never use them against live storage
as migration tests. Notification health checks can send configured alerts.

Dotjik owns source selection, exclusions, host identities, mount expectations,
retention and notification policy. `${HOME}` and `${XDG_RUNTIME_DIR}` expand in
policy values. Existing credential/state paths and repository formats remain
unchanged. External dependencies are explicit: rclone/FUSE, Restic, rsync,
Docker/MariaDB, findmnt, mountpoint, SSH, systemctl, timeout and SnapRAID; Plex's
SQLite tool is required only for configured Plex databases. Python is test-only.

See [the compatibility contract](docs/protection-contract.md) for schemas and
failure behavior. `protection-migration --policy POLICY -- COMMAND...` holds the
existing locks and refuses active workers before a reviewed routing change.
It never enables timers or stops/restarts jobs. Install a capable root-owned
binary for system units before applying their routing. Published releases and
release pins are a separate step; the installer rejects older protection engines.

```sh
cargo test --locked
python3 -B -m unittest discover -s tests -p 'test_protection.py' -v
```

The protection suite uses generated SQLite data, fake mounts/processes and
localhost notification servers, plus real encrypted Restic repositories in
temporary directories for Drive, application and independent-copy restore/hash
rehearsals. These are synthetic recovery proofs, not live backup acceptance.
