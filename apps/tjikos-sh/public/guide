# Fedora cold bootstrap

`sh.type.hk` is a dedicated static Cloudflare Worker (`tjikos-sh`). It serves a
reviewable shell script and this guide. There is no HTTP upload, enrollment API,
credential storage, or public write endpoint. Source remains in `tijptjik/tjikos`
and `tijptjik/dotfiles`; only authorized GitHub maintainers and Cloudflare account
members can publish. Public readability is necessary for an unauthenticated
new machine. Do not describe a public repository as "write-only": it is public
read, owner-controlled write.

## On a new machine

Install Fedora on x86_64, create your normal account with sudo/wheel privileges,
and connect to the network. Set the intended unique hostname before enrollment;
do not call a new machine `fi`, `li`, or `si` to inherit their configuration.
Those names carry hardware, storage, and backup policy.

Prefer downloading and inspecting exactly the file you will execute:

```bash
curl --proto '=https' --tlsv1.2 -fsSLo init.sh https://sh.type.hk/init
less init.sh
bash init.sh --plan
bash init.sh
```

The convenience form is `curl --proto '=https' --tlsv1.2 -fsSL
https://sh.type.hk/init | bash`. Use HTTPS explicitly. Piping skips the review
step; downloading and reviewing a different copy before piping would not verify
the executed copy. A checksum from the same host detects accidental differences,
not compromise of that host. Compare the script and pinned source commits with
your independently trusted GitHub checkout when checking for tampering.

The script:

1. Installs Fedora packages `ca-certificates curl git fish chezmoi gum age jq
   python3 python3-tomli python3-tomli-w cargo gcc make pkgconf-pkg-config
   openssh-clients unzip tar gzip gh`.
2. Clones public dotfiles into `~/.local/share/dotjik` and tjikOS into
   `~/code/tjikos` over HTTPS. SSH keys are not needed for this step.
3. Checks out the exact commits embedded in the reviewed script on a local
   `bootstrap` branch tracking the published bootstrap release branch, then
   builds the Rust runner. This release uses `bootstrap/fedora-2026-10-28` in
   both repositories; switching to `main` later requires reviewing its schema
   compatibility first.
4. Runs `tjikup new device`, asking for desktop/laptop/server and optional recipes.
5. Runs the existing Bitwarden bootstrap. You enter the passphrase for the
   encrypted machine token on the terminal. The token remains in
   `~/.config/bws/environment` and the age identity in `~/.keys/chezmoi.txt`, both
   private. This uses Secrets Manager, not an unlocked personal `bw` vault.
6. Initializes Chezmoi's template configuration with the selected role. It does
   not apply home or system configuration. Chezetc's legacy role is resolved
   through the same local profile.
7. Offers GitHub device/web authentication via `gh`; it never embeds your GitHub
   credential in the URL, profile, or repository. Git writes need your account's
   existing repository permission; bootstrap does not grant anybody write access.
8. Prints `tjikup --plan`. Review it before running
   `tjikup --skip-repo-sync --show-skips` for the first apply.

Keep Bitwarden access and recovery material outside Git. Tailscale membership,
disk initialization, backups, desktop login and any hardware setup remain
explicit device setup steps. No cold bootstrap should guess disk UUIDs or copy
Si's storage layout.

## Enrolling and changing subscriptions

```bash
tjikup new device
# Revise an existing enrollment:
tjikup new device --replace
python3 ~/.local/share/dotjik/scripts/device-policy.py configure
tjikup --plan
```

Profiles live at `~/.config/tjikup/device.json` (or under `XDG_CONFIG_HOME`). They
contain a hostname, role and explicit recipe IDs, not secrets. Do not copy a
profile to a different hostname. Automation can use:

```bash
tjikup new device --role laptop --subscribe system:zed,home:python
```

The wizard lists recipes compatible with the role and host, then shows the full
resolved plan before saving. Enter `none` for the portable baseline, `all` for
all compatible optional recipes, or comma-separated numbers/IDs. Dependencies
are automatically included and shown. Unsubscribing stops future task execution;
it does not uninstall packages, remove configuration or revoke credentials.

Recipe metadata is declarative:

```json
{
  "id": "editor",
  "optional": true,
  "roles": ["desktop", "laptop"],
  "needs": ["compiler"],
  "after": ["fonts"],
  "apply": ["install-editor"]
}
```

`needs` includes a true prerequisite. `after` orders recipes only when both are
selected. `hosts` restricts genuinely machine-specific tasks. Root `device_file`
selects the local profile; `legacy_hosts` preserves existing behavior only for
explicitly grandfathered devices. Unknown unenrolled hosts fail before execution.
A profile with an unknown, incompatible, or removed recipe fails validation.

## Current portability boundary

This is not a claim that the whole legacy dotfiles tree is machine-independent.
Existing `fi`/`li`/`si` native manifests remain authoritative for those hosts.
New devices use `native/home/files-device.json` with a small explicit
`portable-files.json` allowlist. Its app files are conditioned on resolved
subscriptions and role. It excludes captured state, old machine identifiers,
external symlinks, broad service activation and storage/backup credentials.
Additional portable files must be reviewed and added deliberately. A new device
with no app subscriptions does not receive all 831 existing home entries.

New host system-file application is a no-op until a host-specific manifest is
provided; selected package recipes can still run. Storage-heavy server recipes
remain restricted to `si`. Existing hostname conditions for hardware, mounts,
backups and account-specific integrations are intentionally preserved. Roles
replace the broad client/desktop/laptop/server switches, not physical identity.

## Publishing

The bootstrap must not point at an unpublished migration. Set `release.json`
with exact, publicly available dotfiles and tjikOS commit hashes containing the
new runner, enrollment command, role policy, portable manifests, and matching
recipe schema. `scripts/deploy.py` verifies key files exist at both commits,
builds with `--release`, performs a Wrangler dry run, deploys, then byte-compares
the public endpoint. Asset upload uses Wrangler; hostname attachment uses the
account-level Workers custom-domain API, so deployment does not need permission
to list or modify unrelated zone Worker routes. A normal workspace build may produce an explicitly
non-executable draft while publication is pending.

```bash
bun install
bun run --filter @tjikos/sh check
# Export Cloudflare credentials in your shell or load them with a private local
# launcher. Never put tokens into wrangler.jsonc, release.json, or the script.
bun run --filter @tjikos/sh deploy
```

Cloudflare configuration is in `wrangler.jsonc`: custom domain `sh.type.hk`,
static assets only, no workers.dev alias, no secret bindings. Keep GitHub write
access owner-only and deployment tokens restricted to your account. Account
administrators can still publish; a repository setting cannot override that.
Use reviewed commits and retain prior release hashes for rollback.

Acceptance requires a fresh Fedora VM run through package installation,
interactive credential unlock and selected first apply. Local mock tests and a
successful Worker deployment alone do not establish that acceptance.
