# Native cutover status — 2026-10-30

The release and deployment preparation is complete through the safe waiting
point. **The fleet is not yet fully cut over:** Si is waiting for its existing
backup and restore validation; Li went offline during deployment. Neither is
reported as successfully completed.

## Completed

- Published signed `synctjik-v0.1.0-bootstrap.4` from
  `2c0963bfc806f100dc292b2036e9cb83823eeba8`.
  Static Linux x86_64 executable SHA-256:
  `eaf2e98fcd4bc0ad41d324170ceca763ad5161d224982146914c89e520e0a426`.
  Downloaded assets, checksum manifest, source provenance, static linking and
  native/protection capabilities were verified.
- Fixed `private-assets crypt-password` to use BWS directly with the existing
  private token file or environment. It no longer invokes the removed Fish
  helper. Synthetic tests cover successful encryption, provider failures,
  mismatched secret IDs, token-file permissions and preservation of old output.
  No live encryption password was changed.
- Fixed a directory-order-dependent application-backup bug: SQLite snapshot
  replacement removes staged WAL/SHM/journal files; the scanner now skips those
  deliberately removed paths. The live source database remains untouched.
- Fixed release CI's sparse checkout to include shared Fish completion files
  and installed its real restore-test dependencies. The earlier `.3` tag failed
  CI and produced no release; its immutable tag was not rewritten.
- Updated binary and source pins, removed obsolete Python enrollment guidance,
  and deployed `https://sh.type.hk`. The public `/init` and `/guide` were compared
  byte-for-byte with the reviewed artifacts. Worker version:
  `42e63974-3974-4c81-b1db-8f21d91d8bdf`.
- Fi now runs the published Synctjik binary; readiness passes and schedule
  migration reports current. Its next scheduled jobs use this binary.
- Fi, Li and Si user, weekly and privileged Tjikup copies converge on published
  `tjikup-v0.2.0-bootstrap.6`, SHA-256
  `2419a610a0cc47f6d96b7d590b022cf870638ebd0afc0b4ba9af39599e0f55af`.
  Native system applies found 19/20/90 unchanged files respectively.
- Merged Li and Si's equivalent Telegram-policy commits into shared history.
  Both remote checkouts were clean when inspected. Published policy revision
  `a28c5f8046b16bcdd6594818671bb63192fc9f72` includes removal rules for an older
  `chezup` launcher and obsolete Python Tjikup/Chezup modules discovered during
  this pass. Fi's copies were moved to private recovery storage.
- Preserved all unrelated local icon edits and all running archive workers.

## Further Fi-only cleanup

Completed without contacting Si or Li:

- Dotjik and Systjik now have canonical icon files, catalog keys and fleet
  mappings. Old icon paths are removal tombstones. Artwork bytes are unchanged.
  Fi received only the eight affected icon-path changes through native receipts,
  and its icon cache was refreshed. All eight entries then checked unchanged.
- Integrated the rename through an isolated worktree; verified all 68 existing
  dirty files retained their prior edits, including overlapping catalog metadata.
- Discovered and retired the older `chezupdate` Python launcher/package as well.
  Removal rules are published in policy revision
  `58b328a7c9ce13fce663d576d2a64a76664a8f4e`.
- Moved Fi's duplicate legacy age identity, unused legacy configuration folders,
  two inactive Fish snapshots and an old migration-recovery directory into
  `~/.local/state/tjikup/native-cutover-20261030/`. Key bytes were compared before
  moving the duplicate; the canonical private identity passed the system check.
- Active Fish configuration, user units, text launchers and installed source
  modules scan clean for the retired deployment names. Native Tjikup validation
  and Synctjik readiness pass. Recovery copies and migration fallbacks remain.
- Replaced the Python context-command example and obsolete adapter/system-runtime
  guidance with native workflow commands. Updated icon documentation.
- Published bootstrap source pins containing this cleanup and verified the public
  installer and release manifest. No new runtime release was needed.
- Revalidated the dirty policy checkout: 321 tests, nine skipped, passed; all
  nine workspace build tasks and all ten bootstrap checks passed. Fi's final
  native configuration, icon-file checks and archive readiness passed.

The previously queued Si/Li work retains its reviewed pins. These newer cosmetic
and cleanup changes can reach them through normal policy synchronization after
that cutover; no remote job was changed during this follow-up.

## Si: queued behind real backup and restore evidence

The backup and validator remained running with unchanged PIDs 1910018 and
1919892. Validation status was `running`, phase `waiting-for-backup` at the
last check. Its installed user/root Synctjik binaries remain unchanged because
that running validator invokes the user binary again during restoration.

The new release is downloaded and checksum-verified in the TjikOS checkout.
`tjikup-si-native-cutover.service` remains queued after those existing jobs.
The reviewed script now:

1. Requires the validator's durable status to be `passed` / `complete`, exit 0.
2. Checks the reviewed policy/tool revisions, clean tracked worktrees, and
   Tjikup/staged Synctjik checksums. The old script's stale revision/checksum
   guards were corrected; live baseline snapshots still matched before queuing.
3. Preserves previous runtimes, installs the staged release for user and root,
   and retains obsolete Python deployment modules in private recovery storage.
4. Runs native baseline adoption under protection locks, then Tjikup with
   repository synchronization and package upgrades skipped.
5. Checks nginx, storage, Compose, retired Python entry points and native
   archive readiness. Conflicting changes fail closed rather than being forced.

Private evidence remains under
`~/.local/state/tjikup/si-native-cutover-20261029T151734Z/`:
`queued-run.log`, `queued-run.exit`, `queued-run.finished`.
The old script and binary recovery copies are retained there. No completion
marker is inferred from an inactive systemd unit.

On Si, inspect:

```sh
systemctl --user list-jobs
systemctl --user status tjikup-si-native-cutover.service
cat ~/.local/state/tjikup/si-native-cutover-20261029T151734Z/queued-run.exit
```

## Li: automatic catch-up when reachable

Li's privileged Tjikup update completed before the laptop became unreachable.
Its Synctjik update is **not yet verified**.

Fi has a one-off user service, `tjikup-li-native-release-catchup.service`, that
retries every ten minutes and is enabled across Fi restarts. It uses pinned
native SSH resolution and the reviewed script under
`~/.local/state/tjikup/li-native-release-catchup-20261030/`.
It accepts only clean checkouts that can fast-forward to the reviewed revisions,
defers while Synctjik is running, installs/verifies the released binary,
preserves obsolete Python modules, and checks readiness and schedule routing.
It stops on review-required exit status 2 and disables itself after success.
It does not initiate a new backup or replace Li's existing verified Restic data.

On Fi, inspect:

```sh
systemctl --user status tjikup-li-native-release-catchup.service
journalctl --user -u tjikup-li-native-release-catchup.service -n 30
```

## Remaining names and wider Python scope

This is not a claim that every historical string or Python file has vanished.

- Keep removal tombstones until obsolete clients have converged. Old lock,
  baseline and receipt paths remain compatibility contracts.
- Existing age identity recovery copies and the migration fallback are retained.
  Fi now keeps its duplicate only in private recovery storage.
- Si's old checkout paths remain until home acceptance. Li's old links/checkouts
  cannot be retired while it is offline. Fi's old Tooltjik checkout is clean
  and its commit is ancestral to TjikOS, but an open Fish shell still uses it
  as its working directory, so it was not moved.
- Canonical icon naming is complete in policy and on Fi. Si/Li still need the
  later policy revision after their already queued native cutover.
- Frozen Python test oracles, build/test utilities and Python application or
  host extensions remain. They are not all responsibilities of Tjikup/Synctjik.

## Validation

- Published release CI: 24 Rust tests passed, one intentionally ignored;
  all 75 Python compatibility/protection tests passed.
- Local protection rerun after the WAL fix: 28 passed, including synthetic
  Restic restore verification. Installer tests: 7 passed.
- Workspace `bun install` / `bun run build`: all nine build tasks passed.
- Bootstrap checks: 10 passed; deployed installer and guide byte checks passed.
- Policy validation: 321 tests, nine skipped, passed; native task templates
  rendered for Fi, Li and Si. An initial isolated rendering failure passed its
  focused rerun and subsequent full validation runs.

Fleet results are observations at deployment time. Successful release tests and
Fi readiness do not establish Si restore success or Li deployment completion.
