# Fedora cold bootstrap

`sh.type.hk` is a dedicated static Cloudflare Worker (`tjikos-sh`). It serves a
reviewable shell script and this guide. There is no HTTP upload, enrollment API,
credential storage, or public write endpoint. Source remains in `tijptjik/tjikos`
and `tijptjik/dotfiles`; only authorized GitHub maintainers and Cloudflare account
members can publish. Public readability is necessary for an unauthenticated
new machine.

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

1. Checks Fedora's existing Python 3.11+ for the remaining standard-library
   helpers. It installs only `ca-certificates curl git fish chezmoi gum age jq
   openssh-clients`. It installs no Python TOML packages, compiler, build tools,
   archive tools or GitHub CLI.
2. Fetches the two public repositories over HTTPS so no Git credential is needed
   to obtain the credential-setup code. It checks out the exact reviewed commits
   on a local `bootstrap` branch tracking `bootstrap/fedora-2026-10-28`.
3. Downloads the uncompressed static Linux x86_64 tjikup executable from a pinned
   GitHub Release, verifies the SHA-256 embedded in this script, and atomically
   installs it. No compilation happens on the new device. Normal tjikup updates
   also use the pinned binary; source builds remain an explicit developer action.
4. Runs `tjikup new device`, asking for desktop/laptop/server and optional recipes.
5. Runs the existing Bitwarden bootstrap. You enter the passphrase for the
   encrypted machine token on the terminal. The token remains in
   `~/.config/bws/environment` and the age identity in `~/.keys/chezmoi.txt`, both
   private. This uses Secrets Manager, not an unlocked personal `bw` vault.
6. Initializes Chezmoi's template configuration with the selected role. It does
   not apply home or system configuration. Existing TOML is read with Python's
   built-in `tomllib` and serialized by Chezmoi itself; custom settings are retained.
7. Configures GitHub SSH from BWS, verifies the authenticated account, then sets
   both remotes to `git@github.com:tijptjik/...`. Initial public HTTPS fetching is
   only the credential-free bootstrap step. Normal Git synchronization uses SSH.
8. Prints `tjikup --plan`. Review it before running
   `tjikup --skip-repo-sync --show-skips` for the first apply.

### GitHub SSH credentials

To retrieve an existing authorized SSH private key from BWS, supply its secret ID
(the value must be an unlocked OpenSSH private key):

```bash
bash init.sh --ssh-key-secret YOUR_BWS_SECRET_ID
```

Without that option, bootstrap reads `GHI_TOKEN` from BWS and registers a newly
created Ed25519 key for this device. The token must be valid and have SSH-key write
permission (`admin:public_key` for a classic GitHub token). A rejected token stops
setup with an actionable error; bootstrap does not fall back to another account
or GitHub CLI authentication.

The private key is stored as `~/.ssh/tjikup_github_ed25519` with mode 0600. Existing
keys are preserved. The two checkouts use a dedicated SSH configuration with
strict host checking and GitHub host keys obtained from its HTTPS metadata API.
The GitHub token is never stored in Git configuration or passed in process argv.
Git author identity is initialized per checkout only if it is not already set.
An already working managed SSH key can be reused without registering it again.


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
recipe schema. Also pin `tjikup_binary.url`, `sha256`, and `source_revision` to
the versioned GitHub Release. Its `build-info.json` records the compiler and exact
source used for the executable; the source checkout can contain later packaging
changes without changing the pinned runtime binary. `scripts/deploy.py` verifies key files exist at both commits,
builds with `--release`, verifies the published binary checksum, performs a
Wrangler dry run, deploys, then byte-compares
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

The static binary is built using `apps/tjikup/scripts/release.sh` on a build
machine. The GitHub Actions workflow automates this from signed `tjikup-v*` tags
once [the CI-only PR](https://github.com/tijptjik/tjikos/pull/1) is merged. The
initial static release was built and checked in an isolated local toolchain.

Acceptance requires a fresh Fedora VM run through package installation,
interactive credential unlock and selected first apply. Local mock tests and a
successful Worker deployment alone do not establish that acceptance.
