# Shared library release

The release targets are the desktop contract and GTK primitives under
`packages/tjik-desktop` and `packages/tjik-gtk`. Tasktjik views under
`packages/tasktjik-gtk` remain an internal shared library with `publish = false` until
the applications have stabilised and the maintainer explicitly reconsiders publication.
Its use by Tasktjik and Runtjik does not require a registry release.

The maintainer selected the `tjik` prefix. Public release targets are `tjik-desktop` and
`tjik-gtk`, initially at version `0.1.0`. The internal Tasktjik crate retains the name
`tjik-task-gtk`. Cargo package names are global; an account or GitHub organization does
not create an exclusive crate-name namespace. Local source directories and dependency
aliases remain unchanged.

The initial releases are published:

- [`tjik-desktop` 0.1.0](https://crates.io/crates/tjik-desktop/0.1.0)
- [`tjik-gtk` 0.1.0](https://crates.io/crates/tjik-gtk/0.1.0)

Both releases use the clean source at `a0c45a7029abd41a14d6bd61844f109adaf7603d`.
Extracted-archive tests and registry-resolved dry runs passed before upload. Source tags
[`tjik-desktop-v0.1.0`](https://github.com/tijptjik/tjikos/tree/tjik-desktop-v0.1.0) and
[`tjik-gtk-v0.1.0`](https://github.com/tijptjik/tjikos/tree/tjik-gtk-v0.1.0) resolve to
that exact commit. Registry metadata and downloaded archives were read back after
publication; both SHA-256 checksums match their validated candidates.

The initial account-email verification gate is resolved. Cargo credentials are managed
through Dotjik and verified on Fi and Li. See the
[release receipt](library-release-receipt.json) for hashes, publication timestamps and
historical attempts. The earlier `1791bfa` candidate is historical.

For subsequent authorised releases, use the following procedure:

1. Query crates.io for each exact name and version. An unused name is only an
   observation; it is not reserved until publication succeeds. If a name already exists,
   check ownership before treating it as a release target.
2. Update the package names, self-referencing tests/examples and versioned dependency
   declarations. Application dependency keys can keep their existing names by setting
   Cargo's `package` field. Keep third-party dependency versions pinned.
3. Create a clean detached worktree for the selected release commit. Publish from that
   worktree so unrelated local edits cannot enter a registry archive. Run
   `dev-check bash packages/tjik-gtk/scripts/test-packages.sh`. The checker derives
   archive names from the manifests, packages staged copies, validates license/notice
   bytes and runs locked offline tests. Its local GTK patch establishes archive
   isolation; it does not prove that registry dependencies are available.
4. Publish only the independent desktop and GTK crates. Use the crates.io registry
   explicitly. Do not publish the internal Tasktjik view crate.
5. Read back the published version metadata and source archive; compare the released
   source and legal files with the validated candidate. Record package names, versions,
   archive hashes and registry URLs in the release receipt.

The shared libraries use MPL-2.0 with standard secondary-license compatibility. The
bundled desktop protocol retains BSD-3-Clause. The reviewed applications use
GPL-3.0-or-later. Native GTK and gtk4-layer-shell remain separately installed platform
dependencies with their own licenses.

Application builds bundle their own `LICENSE`/`NOTICE` and the reviewed linked Tjik
components under `dist/licenses/`. Supported installers copy those files into the
application directory. The pinned native layer-shell license remains in `dist/lib/`.

## Repository development checks

On managed development machines, run heavy checks through `dev-check`. These commands
are repository-maintenance tools, not requirements for third-party library consumers:

```sh
dev-check python3 -B packages/tjik-gtk/scripts/test-images.py
dev-check python3 -B packages/tjik-gtk/scripts/test-motion.py
dev-check bash packages/tjik-gtk/scripts/test-packages.sh
```

The image and motion runners provide their own isolated displays, with visible parent
windows placed silently on workspace 7. The archive checker also exercises the internal
Tasktjik view crate with a local GTK dependency patch. Packaging it for an isolation
check does not make it a publication target.

The public `tjik-gtk` README contains a compilable consumer example, native dependency
requirements and API/resource/lifecycle contracts. Release state and maintainer commands
belong here instead of that README. The corrected source author is `tijptjik`; the
tagged candidates include that attribution.
