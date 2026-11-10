# Shared library release

The release consists of the desktop contract, GTK primitives and Tasktjik views under
`packages/tjik-desktop`, `packages/tjik-gtk` and `packages/tasktjik-gtk`. These
directory names and the applications' dependency aliases can remain stable when public
Cargo package names change.

The maintainer chooses the public crate-name prefix before registry names are changed or
reserved. Cargo package names are global; an account or GitHub organization does not
create an exclusive crate-name namespace. The intended pattern is `<prefix>-desktop`,
`<prefix>-gtk`, and `<prefix>-task-gtk`.

For the selected names:

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
4. Publish the independent desktop and GTK crates, and wait until the selected GTK
   version is visible in the registry. Then run a registry-resolved dry run and publish
   the dependent Tasktjik view crate. Use the crates.io registry explicitly.
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
