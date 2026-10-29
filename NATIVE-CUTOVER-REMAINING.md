# Tjikup / Synctjik migration audit

Read-only code and live fleet audit, 2026-10-30. The migration is **not fully
complete**. A successful selected workflow on Fi or Li does not establish that
every fleet entry point, optional command, installation path and document has
been migrated.

## Release and installed runtime state

| Component | Verified state | Remaining work |
| --- | --- | --- |
| User Tjikup and weekly runtime | Fi, Li and Si use the published static `tjikup-v0.2.0-bootstrap.6`, checksum `2419a610…0f55af` | Keep both entry points on verified releases |
| Privileged Tjikup helper | All three retain an older dynamic build at `/usr/local/lib/tjikup/tjikup`, checksum `46c39790…409` | Converge privileged copies through the native system-runtime installation path |
| Synctjik | Native engines exist on all three; Fi/Si use local build `098a01f6…dc3`, Li uses the newer Fish-probe fix `d8b95a6a…dd5` | Publish a compatible release and deploy the same verified artifact without replacing active pinned workers |
| Synctjik installer pin | `apps/tjikos-sh/release.json` still pins `synctjik-v0.1.0-bootstrap.2` | Advance it after release verification; current installer deliberately retains a capable existing binary or rejects a fresh incompatible install |

The new release must include native backup/protection engines, schedule metadata
and permission fixes, and the Fish-compatible backup probe. Fix the dangling
optional command below before calling the entire native interface complete.

## Si still has active Python entry points

Observed on Si:

- `~/.local/bin/device-archives` and `~/.local/bin/private-storage` are Python
  executables, unlike Fi/Li's shell wrappers delegating to native Synctjik.
- `backup-packages.timer` is enabled. Its service calls `device-archives ready`
  and `device-archives backup`, so this scheduled route still executes Python.
- `~/.local/lib/fleet_ssh.py` and `~/.local/lib/tjik_git.py` remain installed.
- `tjikup-si-native-cutover.service` has a waiting start job; the Drive
  restore-validation service is actively running. Both `queued-run.exit` and
  `queued-run.finished` are absent. An inactive unit with default exit status 0
  is not evidence that the queued cutover completed.
- The inspected system application-backup route already calls
  `/usr/local/bin/synctjik app-backup backup`. System and home cutovers have
  different completion states.

Let the pinned restore-validation job finish, inspect its result, then complete
the guarded home apply. Verify removal/replacement of Python entry points and
legacy schedules, native readiness, receipts and a representative restore.
Do not interrupt the worker merely to complete cleanup.

## Broken or stale migration references

1. `apps/synctjik/src/backends/private_assets.rs`, `crypt_password()`, still
   invokes `~/.local/share/dotjik/home/.bws-wrapper.fish`. That helper is absent
   from current policy and was confirmed absent on Fi and Si. This optional
   command has not been fully migrated; use the supported secret provider
   rather than recreating the old deployment helper.
2. `apps/tjikos-sh/README.md` and generated `apps/tjikos-sh/public/guide` still
   instruct users to execute `scripts/device-policy.py configure`. The script
   is absent. Replace the instruction with the native configuration workflow,
   rebuild the guide and include it in the next site deployment. The public
   website itself was not fetched in this audit.

## Legacy names and files remain

- Li keeps a compatibility `chezmoi` symlink, the old `chezetc` checkout and a
  `tooltjik` symlink. Si retains old checkout directories. These were observed,
  not removed by this audit.
- Old `~/.keys/chezmoi.txt` identities remain on all three machines; Tjikup has
  an explicit migration fallback. Retire only after checking neutral identity
  copies, references and recovery coverage; never delete keys for cosmetic
  terminology cleanup.
- `device-archives` / `private-storage` compatibility commands and their state
  directory names remain. Native archives deliberately preserve existing
  baselines and lock paths. Rename them only as a coordinated state migration.
- Icon policy still contains `chezmoi` / `chezetc` names and maps Dotjik/Systjik
  aliases to them. This is active naming debt, not evidence of a deployment
  engine dependency.
- Numerous legacy paths in manifests are **`kind: absent` removal entries**.
  Those are cleanup rules, not installed Python modules. Removing those strings
  would weaken the migration for machines that still have the old files.

## Python scope

Tjikup/Synctjik's native runner and backup engines are Rust, but neither the
whole fleet nor the entire TjikOS repository is Python-free:

- Synctjik retains frozen Python implementations under `tests/legacy` as
  compatibility oracles. Python tests/fixtures are not shipped by its binary
  installer. Deleting those is a separate test-retirement decision.
- `tjikos-sh` still uses Python build/deployment scripts, and Dotjik uses Python
  validation and development utilities.
- Runtime/application integrations remain Python-based: Codex-LB environment
  setup, Gitfetch's Python installation, Kitty's notification extension, Herdr's
  local-file helper, and parts of Bartjik, Greetjik, Mailtjik, Notjik and Runtjik.
  These are separate applications or host extension APIs, not automatically
  responsibilities of the Tjikup orchestration or Synctjik backup engines.

## Recommended finish order

1. Fix the dangling secret-helper call and obsolete onboarding instructions.
2. Publish and pin a compatible Synctjik binary; retain Tjikup's current release.
3. Complete Si's guarded home cutover after its active validation finishes.
4. Converge native binary versions across user, privileged and weekly paths.
5. Retire verified-unused aliases/modules and reconcile icon/documentation names.
6. Treat any wider removal of Python application/build/test code as a separately
   scoped migration, rather than claiming it happened with the runner cutover.

No services, credentials, legacy files or running workers were modified by this
audit. Remote audit results are a point-in-time observation.
