# Li native cutover and boot investigation

Recorded 2026-10-30 (Hong Kong time). File deployment completed; the remaining
Tjikup service-convergence workflow and package updates have not run.

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
