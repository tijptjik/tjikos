# Native file and template management

`tjikup files` manages an explicitly declared set of files without Chezmoi or
Chezetc. It is a separate command so existing recipes can call it through their
normal `check` and `apply` argv arrays. Existing `files` manager adapters and
Go-template rendering retain their original behavior.

The dotfiles repository owns fleet manifests and recipes, including `system/`.
Fi system files and all declared home files use this engine, including captured
Zed/Herdr settings and Bitwarden-backed Docker configuration. Maintenance-task
templates and encrypted server system configuration continue through the legacy
Chezmoi adapter during migration.

## Manifest

```json
{
  "version": 1,
  "id": "desktop-files",
  "host": "fi",
  "context": {"user": {"name": "io", "home": "/home/io"}},
  "partials": {"header": "templates/header.j2"},
  "entries": [
    {
      "kind": "file",
      "path": "{{ user.home }}/.config/example.conf",
      "source": "templates/example.conf.j2",
      "template": true,
      "mode": "0600",
      "uid": 1000,
      "gid": 1000,
      "sensitive": true
    },
    {
      "kind": "symlink",
      "path": "{{ user.home }}/.local/bin/example",
      "target": "../lib/example/bin/example",
      "uid": 1000,
      "gid": 1000
    }
  ]
}
```

Sources and declared partials are relative to the manifest directory. Traversal,
absolute source paths and symlinks escaping that directory are rejected. Templates
use MiniJinja with strict undefined variables, preserved trailing newlines and no
HTML escaping. Only declared context and partials are available; rendering does
not execute commands or implicitly read environment variables. Host identity comes
from the running kernel. User identity for templates is explicit context, so sudo
does not silently turn a user's home into `/root`.

`when` is an optional MiniJinja expression that must produce a boolean. A false
condition leaves the destination unmanaged for that run; it does not remove it.
`create_only: true` preserves any existing regular file or symlink, including
application edits. `kind: "absent"` explicitly declares removal; it only removes
files/symlinks previously tracked by this deployment state. An already absent
path needs no change. Removing an entry from the manifest never deletes its target.

Regular files default to mode `0644`; uid/gid default to the invoking effective
identity. Declare numeric uid/gid explicitly for deployments that cross privilege
boundaries. Special permission bits are unsupported. Sensitive files require
owner-only modes. Destination directories are created as needed; managed directory
metadata, recursive tree pruning and encrypted sources are not implemented.

## Commands and recipe integration

```sh
tjikup files plan --manifest ./manifest.json --state-dir "$HOME/.local/state/tjikup/files"
tjikup files check --manifest ./manifest.json --state-dir "$HOME/.local/state/tjikup/files"
tjikup files apply --manifest ./manifest.json --state-dir "$HOME/.local/state/tjikup/files"
```

`plan` and `check` render all inputs and inspect current files without creating
state, changing destinations, running validators, or starting services. Both
return **0** when current, **1** when changes are needed, and **2** on conflicts or
errors. `--json` outputs a path/action inventory; neither mode prints file content
or template data. `apply` returns 0 on success and 2 on error. All rendering and
conflict detection complete before destination changes begin. Application-specific
syntax validation can be a separate recipe against an isolated staged root; it is
not implicitly executed by a file manifest.

An explicit `--root /absolute/staging-directory` remaps destination paths into an
existing directory for isolated application. Symlink targets retain their declared
text; they are never dereferenced by the writer. State is bound to that root and
manifest ID, so a staged baseline cannot authorize writes to `/`. Keep state and
receipts outside the configuration repository. Use one state directory per
privilege scope, shared by native file recipes to serialize their deployments.
Each destination should have one owning manifest.

A future system recipe can use the existing runner's ordinary command interface:

```json
{
  "id": "system:native-files",
  "check": ["sudo", "-n", "/home/io/.local/bin/tjikup", "files", "check", "--manifest", "{config_dir}/native/fi/manifest.json", "--state-dir", "/var/lib/tjikup/files"],
  "apply": ["sudo", "-n", "/home/io/.local/bin/tjikup", "files", "apply", "--manifest", "{config_dir}/native/fi/manifest.json", "--state-dir", "/var/lib/tjikup/files"],
  "resources": ["system-files"]
}
```

Use the actual deployed binary and manifest path for the recipe's declaring
directory. Do not enable this alongside Chezetc writes to the same destinations.
Tjikup does not elevate itself. Home and root file deployments can belong to the
same feature, while keeping explicit identity, ownership and state boundaries.

## Drift, durability and recovery

Existing destinations are automatically adopted only when they already equal the
desired bytes/type/mode/ownership. Different unmanaged files, local edits and local
metadata changes are conflicts. There is deliberately no blanket force/overwrite
switch. Resolve the discrepancy or explicitly back up and remove the target before
adopting a new baseline.

Identical repeat applies do not rewrite targets or the state file. Parent symlinks,
directories at file destinations and special files are rejected. Writes use pinned
parent descriptors, staged files and atomic rename. Existing extended attributes
(including SELinux labels and ACLs) are preserved; an explicit mode change updates
the ACL mask. New live regular files resolve their SELinux label with `matchpathcon`
when SELinux is active. Atomicity is per file, not across the entire manifest.

Before replacing anything, a private receipt directory holds original content,
metadata, extended attributes and before/after checksums. State has a write-ahead
checkpoint for the current file. After interruption, Tjikup recognizes a target
matching either the old or new state; any other state requires attention. It does
not claim rollback of arbitrary tasks or a system-wide transaction.

```sh
tjikup files restore --receipt /path/to/receipt-XXXX/receipt.json \
  --state-dir /path/to/deployment-state --root /the/original/root
```

Restore validates backup hashes and requires the same root and privilege scope.
It refuses targets edited since the apply, restores saved file metadata/content
and symlink targets, checkpoints the restored baseline, and writes its own recovery
receipt. Newly created empty parent directories are retained. Backup content is
private plaintext; encryption and retention policy remain separate work.

## Verification

`cargo test --locked --test managed_files` covers real temporary filesystem
operations: repeat-apply inode/timestamp stability, conflict preflight, templates,
symlinks, ownership defaults, modes, create-only files, explicit deletion,
interruption recovery, extended attributes and checked restoration. The dotfiles `system/`
comparison script additionally verifies all Fi candidates against Chezetc rendering
and root ownership using an isolated destination, then previews live drift.

## Bitwarden Secrets Manager

Add a provider to a manifest; tokens and returned values never belong in the repo:

```json
"bitwarden": {
  "command": ["/usr/local/bin/bws"],
  "access_token_file": "{{ user.home }}/.config/bws/environment"
}
```

`BWS_ACCESS_TOKEN` from the environment takes precedence. Otherwise the provider
reads one `BWS_ACCESS_TOKEN=...` line from the configured regular file, owned by
the invoking user with mode `0600`. The token goes only in the child environment.
This uses the existing machine access token, not a personal-vault login.

Content templates can call `{{ secret("secret-uuid") }}`. Use `fish_quote` for
Fish literals and `json_quote` for JSON strings. Values are cached only for one
manifest render. Lookup failures, invalid responses and timeouts fail the whole
render before any target is written. Provider output and rendered values are
omitted from diagnostics and plans. Destination paths and conditions cannot call
`secret()`. A secret-backed entry must declare `sensitive: true` and owner-only
permissions. Review formatting for each destination language; JSON quoting is
not a general-purpose shell or TOML serializer.

Sensitive replacement backups are still private plaintext under the state
directory. Keep that directory outside the repo and apply local backup retention
policy to it. Create-only files preserve application-refreshed credentials.

## Shared policy and runtime context

A manifest may load `context_files` (JSON objects) and `entries_files` (JSON arrays)
relative to its directory. Source containment rules apply to both. Context files
are merged in order, followed by inline `context`; `--context-file PATH` overlays
explicit runtime values without moving the manifest or its sources. Merging
replaces top-level keys. No file values are persisted in deployment state.
The dotfiles helper uses this for GTK's selected icon theme and checkout path;
captured bookmark/ZapZap data stays in repository JSON inputs.

`kind: "directory"` declares directory mode and ownership. Directories are
processed before descendants. Existing directories are updated in place without
replacing their contents. Receipts restore directory metadata; removing a newly
created directory only succeeds when it is empty. There is no recursive deletion.
Ordinary directory/file type replacement is rejected during preflight.

Use `compose_quote` for Docker Compose secret strings: it JSON-quotes the value
and doubles dollar signs so Compose preserves them instead of interpolating
host environment variables. File sources remain literal unless `template: true`
is explicit, including files that contain another application's template syntax.
