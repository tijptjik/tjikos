# Li native cutover and boot investigation

Recorded 2026-10-30 (Hong Kong time). **Package updates and configuration
convergence now both report ALL SYSTEMS GO.** Docker and Tailscale are active;
Synctjik enrollment, readiness and schedules are verified. Following explicit
confirmation of the folders and destination, a fresh Restic backup and sample
restore verification also succeeded.

## Verified backup after explicit confirmation

The 01:00 scheduled backup exposed a native compatibility bug: its repository
probe used POSIX syntax through Si's Fish login shell. Fixed the probe to invoke
`sh` explicitly and pass the repository as a quoted positional argument
(`d517d72`). Five focused tests passed, including real Fish/POSIX probes with
spaces, apostrophes and shell metacharacters in the path. Release build and the
workspace build passed (9 tasks). Release CI now installs Fish for this test.

Installed the verified binary on Li with workers inactive and ran the approved
native backup of documents, projects, resources and media to Si. Evidence:

- `synctjik-li-verified-backup.service`: exit 0.
- Log: `verified-backup.log` in the private recovery directory; native repository
  check and retention completed successfully.
- New snapshot: **`f372bebb`**, hostname `li`, created
  **2026-10-30 01:05:58 HKT**, containing all four configured source folders.
- Local success receipt: `20261029T170637Z`; Si's coordination receipt agrees.
- Restored one small documents file into the private audit directory and
  confirmed it matches the local file byte-for-byte. This is a representative
  restore check, not a full restore of all four folders.

Bisync remains the two-way synchronization path to encrypted Drive. Restic is
the separate recoverable-history layer on Si; it does not replace bisync.
The old error receipt remains historical and predates the successful backup.
Earlier approval-boundary notes below describe prior checkpoints and are now
superseded for this operation.

## Final catch-up after approval of all remaining work

- Applied the backed-up Docker nftables configuration and explicit IPv4
  forwarding after user approval. Docker starts and its bridge network exists.
  Preserved all other daemon settings. Declarative Li policy contains both
  `/etc/docker/daemon.json` and `/etc/sysctl.d/90-tjikup-docker.conf`.
- Completed RPM/application upgrades. The first run exposed a Zed sudo/PATH
  bug; the updater now invokes the target user's native Tjikup by absolute path.
  The retry finished successfully, including stable/preview Zed, Tippecanoe,
  Gitfetch and upstream Tailscale. Log: `package-retry.log`; user unit
  `tjikup-li-package-retry.service` reports `Result=success`, exit 0.
- Final configuration run also reports ALL SYSTEMS GO in
  `config-workflow-final.log`. Tailscale has automatic updates enabled,
  operator `io`, and tailnet DNS/default-route acceptance disabled as intended.
- Captured and committed Li's newer Fcitx group ordering and Zed preference
  (`f9e6f27`), retaining concurrent weekly-schedule edits. Reconciled the captured
  history locally. Li's TjikOS checkout advanced to committed `15dcb87`.
- Reconciled schedule descriptions with native enrollment (`4465742`) and fixed
  Synctjik's unit installation permissions (`4b813ed`). The tested binary was
  hash-verified and installed with workers inactive. Rerunning `synctjik schedule`
  now preserves mode 0644 and no longer causes managed-file drift.
- Made `~/.local/share/dotjik` the real repository directory. The former physical
  name remains a compatibility symlink, preserving existing application paths.
  The config link now resolves consistently to Dotjik. Final home file check
  exits 0; credentials and system checks also passed. Last canonical-path receipt:
  `~/.local/state/tjikup/files/home/receipt-596Q9y/receipt.json`.
- All three Synctjik timers and the concurrently configured weekly Tjikup timer
  are enabled. Synctjik reports ready and eight valid enrolled collections.
  A later private sync receipt confirms all four configured folders completed
  at `20261029T163645Z`. No fresh backup receipt is claimed.
- DNF's reboot check reports no updated core libraries/services requiring reboot.
  The running kernel remains `7.2.11-300.fc45.x86_64`. No reboot was performed;
  the earlier intermittent Plymouth stall has not been reproduced or resolved.

Backup routing was repaired in `4c8d42d`: Si's public address timed out and its
old LAN address had no route, while discovered tailnet IPv6
`fd7a:115c:a1e0::32f:dc65` returned hostname `si` with the existing pinned
`HostKeyAlias=ssh.type.hk`. The shared policy now prefers that verified route.

Automatic approval review initially required confirmation naming both the
payload and destination. The user subsequently supplied that confirmation for
Li's `/home/io/documents`, `/home/io/projects`, `/home/io/resources` and
`/home/io/media` into the existing encrypted Restic repository on Si at
`/mnt/storage/private/backups/clients/li`. Native policy excludes `.git`,
dependency caches and trash. The successful run is recorded above.

Validation: the two native schedule-migration tests passed; the Synctjik release
build passed; `bun install` and the workspace build passed (9 tasks). All 52
task templates rendered and syntax-checked separately for Fi/Li/Si. The broad
policy suite encountered unrelated concurrent Mullvad icon-contrast failures;
its older rendering assertion also expected 51 tasks after the concurrent weekly
work added a 52nd. Those unrelated files/tests were not modified here.

## Approved convergence and Synctjik enrollment

The configuration workflow refreshed Hyprvoice and configured private-assets
schedules. Docker failed with its existing firewall backend error; the final
workflow status was `NEEDS ATTENTION`, with Tailscale convergence blocked by
dependencies. Log on Li: `config-workflow.log` in the recovery directory below.

Enrolled Synctjik with role `laptop`, source `fi`, and the eight shared
sync/master collections already present in its policy: SSH, GnuPG, icons,
backgrounds, fonts, Bazecor, KOReader and Sublime Text. Existing verified restore
receipts were retained; no new identity copying or forced restoration occurred.
`synctjik ready` returned true and `synctjik check` reported eight valid
collections. `tjikup --check` validated 94 recipes.

All three native timers are now **enabled and active**:

- `synctjik-dotfiles-lfs-backup.timer`
- `synctjik-private-assets-backup.timer`
- `synctjik-private-assets-sync.timer`

This supersedes the inactive-timer checkpoint below. Scheduling is verified;
fresh backup success is not. The latest private-assets receipt still records
the earlier successful sync and the backup error reaching Si. An immediate
backup test was blocked by automatic approval review because it transfers private
data; no manual backup was started.

Docker's supported nftables backend passed configuration validation and avoided
the iptables parser failure, but startup then required explicit IPv4 forwarding.
Automatic approval review blocked persistent forwarding as a network-boundary
change. The original `/etc/docker/daemon.json` was restored byte-for-byte;
no forwarding sysctl was installed or changed. The prepared candidate and original
remain private in the recovery directory. Docker remains failed.

Automatic approval review also blocked the broad package-update workflow,
interpreting the latest approval as limited to service convergence and Synctjik
setup. No package upgrades ran. Further approval is needed for package upgrades,
Docker's forwarding change and, if desired, an immediate backup verification.

Concurrent weekly Tjikup scheduling edits appeared in Li's Dotjik checkout during
the run. They were preserved and excluded from this thread's changes.

## Preservation and reconciliation

- Li was paused in a rebase of `611d4ef` onto `4c71f68`, with one conflict in
  the Herdr session template. Its entire checkout, including `.git` and rebase
  state, was archived privately and checksum-verified before changes.
- Recovery directory on Li:
  `/home/io/.local/state/tjikup/li-cutover-20261029T160807Z`.
  `~/.local/state/tjikup/li-cutover-current` records that location.
- Preserved Li's layout in `session-li.json.j2`, converted template syntax and
  source paths, and separated Fi/Li capture routing. Completed the rebase and
  merged committed Dotjik policy through bundles, without publication.
- Captured Li's current Herdr session and Zed preference. Rejected a stale
  Codex capture that merely restored the old checkout trust path while removing
  its migrated replacements. The current native trust paths were retained.
- Dotjik commits: `fb5bdc4` (host-specific session), `f60cb81` (Li rebase
  preservation), `09e2295` (merge), `164f52d` (live Li captures), and `6683102`
  (skip desktop monitor file on Li). Reconciled history is also in local Dotjik.
- Li's clean TjikOS checkout advanced from `731a3db` to `41c103a`.
  The old separate Systjik checkout was preserved; authoritative system policy
  now lives inside Dotjik. Unrelated local icon/Runtjik work was not included.

## Native deployment

Installed native user and root Tjikup/Synctjik binaries and Fish completions;
preserved the previous Python runner privately. Enrolled Li with the laptop
role and its available recipe subscriptions. Corrected legacy age identity and
Bitwarden access-token permissions to private modes without changing key bytes.

Rendered desired home files and credentials into private staging. Reviewed
41 home conflicts and two credential conflicts; recorded their existing bytes
and metadata through the native deployment engine before applying desired
policy. No force-overwrite or manual state editing was used.

Applied:

- Home: **72 changed, 688 unchanged**;
  `~/.local/state/tjikup/files/home/receipt-PrSgti/receipt.json`.
- Credentials: **2 changed, 3 unchanged**;
  `~/.local/state/tjikup/files/home/receipt-KJCFz5/receipt.json`.
  Hyprvoice contents were identical; Fish changes were naming and quoting only.
  Both legacy mode-0644 files are now mode 0600.
- System: **2 changed, 16 unchanged**;
  `/var/lib/tjikup/files/receipt-CCO3k2/receipt.json`.
  This only renames the Docker repository override with identical contents.
  A newly discovered empty desktop monitor file on laptops was prevented by
  fixing the manifest condition before applying.

Dotjik validation passed **321 tests, 9 skipped**, including separate Fi/Li/Si
rendering. Private plan, render, baseline and apply logs remain in the recovery
directory. No reboot or desktop-session restart was performed.

Final live home-files, credentials and system checks all exited **0**; native
configuration validation passed with **94 recipes**. All three inspected Li
checkouts were clean. Both installed runners were verified as native ELF files.

## Remaining workflow and approval boundary

Automatic approval review initially blocked system deployment. After fixing the
monitor condition and proving the complete plan was only an identical-content
repository override rename, the reviewed system apply was allowed and succeeded.

Automatic approval review separately rejected this remaining command:

```sh
tjikup --config ~/.local/share/dotjik/tjikup.json \
  --apply-dotfiles --apply-sysfiles --skip-repo-sync
```

Its stated concern is that the workflow can disable active legacy rclone
services, enable storage timers and start Docker, potentially interrupting
archive/sync workers. The command did not run. The reviewed plan is private at
`config-workflow-plan.log`; user approval is required to proceed past this
remaining block. Do not bypass it by invoking the same side effects separately.
Native schedule migration reported ready, and no active backup/archive worker
was observed during the checks. Recheck immediately before any approved run.
Both legacy whole-tree rclone services were confirmed inactive. Legacy backup
timers have been retired by the native migration; the replacement Synctjik
timers remain inactive pending the blocked convergence step. **Scheduled
private backup/sync must not be considered enabled at this checkpoint.**
The read-only service preview identified Hyprvoice as needing refresh; the
other active inspected desktop services were unchanged.

Package updates also remain pending. Do not describe Li as fully up to date or
the complete workflow as successful until these steps and their checks finish.
The existing private backup failure is separate: its last run could not reach
Si with the verified SSH identity. No fresh backup/restore proof was established.

## Boot pause

Current boot: firmware 4.894s, loader 4.563s, kernel 1.318s, initrd 5.749s,
userspace 96.625s; total **113.150s**.

The blocking unit was `plymouth-read-write.service`, whose
`plymouth update-root-fs --read-write` call took **82.107 seconds**, from
monotonic 9.115s to 91.223s. It runs before `sysinit.target`, delaying ordinary
startup. The preceding boot used the same kernel and command line and completed
this step in approximately **19 milliseconds**. Later Plymouth quit-wait took
11.135 seconds in the current boot.

No emergency shell, filesystem timeout, encryption prompt or logged request to
press Enter was found. Entropy initialized at 2.060s; i915/framebuffer were ready
by 5.65s. Camera firmware warnings occurred but do not establish causation.
The logs localize the stall to Plymouth; they do not establish why it stalled
or whether Enter released it. No speculative masking, boot-argument change or
reboot was performed. Next-boot observation, and Plymouth debug logging if it
recurs, are needed to identify its internal cause.

Docker failed after the Plymouth stall, so it did not cause that delay.
Its error is `INVALID_IPV: 'ipv4' is not a valid backend or is unavailable`.
Read-only reproduction with `iptables -t filter -S` reports
`meta sreg is not an immediate` and `Parsing nftables rule failed`.
Firewalld probes table availability through iptables and disables a backend
when that probe fails. This explains the unavailable backend, but the offending
rule and correct compatibility fix remain unverified. The previous boot's
Docker ran successfully. Firewall rules have not been flushed or replaced.

## Diagnostic handling

A diagnostic intended to redact credential values missed commented assignments
and printed their values in tool output. The user was informed that the exposed
values should be rotated. No credentials or private captures were committed.
