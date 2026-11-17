# Tasktjik

A separate TjikOS Firefox/Zen extension for saving pages directly to Tasktjik Reading.
Runtjik is not required. The native helper uses the installed Tasktjik client and its
existing authority configuration; credentials never enter the extension.

- **Ctrl+Alt+S** toggles the Tasktjik sidebar.
- **Ctrl+S** saves the current HTTP(S) page's title and URL to **Inbox**.
- **Ctrl+Shift+S**, or the toolbar button, opens the **Tasktjik sidebar**.
- Saving and cancelling leave the tab open, including pinned tabs.
- Private windows and browser/internal pages are excluded.

The sidebar supports title, URL, domain, tags, project, templates, due date, natural
language repeat rules, reminders, nested subtasks and flows, workflow dependencies,
plan, assignee and acceptance criteria. Recurrence parsing, template instantiation and
workflow editing reuse Tasktjik's existing source. Image attachment is a post-save
Tasktjik operation, as in its native creation editor.

These shortcuts replace browser actions when Firefox/Zen assigns them to Tasktjik. Check
**Add-ons Manager → Manage Extension Shortcuts** for conflicts. Actual shortcut
precedence in Zen still requires live verification; manifest validation alone does not
prove it overrides Save Page or Screenshot.

The source workspace is `apps/tasktjik-browser` (`@tjikos/tasktjik-browser`). Install it
with `bash scripts/install.sh --from-build tasktjik-browser` after building.

The separate [Runtjik Browser Bridge](../runtjik-browser/docs/install.md) provides
Runtjik tab and history search; its source is in `apps/runtjik-browser/extension`.

The extension ID is `tasktjik@tjikos.local` and the native host is
`org.tjikos.tasktjik`.

## Install

Version 0.1.2 received Mozilla’s unlisted signature on 2026-11-18. It uses the Tasktjik
extension identity and native-host registration. Install
`~/.local/lib/tasktjik-browser/31c748c914694d1ba257-0.1.2.xpi` through Zen/Firefox’s
`about:addons` → gear menu → **Install Add-on From File**. The local build ZIP remains
unsigned; use the signed XPI for permanent installation.

Firefox treats the changed extension identity as a new add-on. Existing browser-local
settings and drafts are not transferred automatically. Finish pending saves in the
previous extension before replacing it; saved Reading items remain in Tasktjik.

Requires native Linux Firefox 142+ or compatible Zen, Python 3, and an installed,
configured Tasktjik. Flatpak/Snap native-host paths are not covered.

From the Git root, build through the managed validation runner:

```sh
dev-check python3 -B apps/tasktjik-browser/scripts/build.py
python3 -B apps/tasktjik-browser/scripts/install.py
```

The installer atomically installs and hash-checks the helper and extension archive in
`~/.local/lib/tasktjik-browser`, then registers `org.tjikos.tasktjik` under
`~/.mozilla/native-messaging-hosts`. It does not modify any browser profile.

For development, load `dist/extension/manifest.json` through `about:debugging` → **This
Firefox → Load Temporary Add-on**. This disappears at browser restart. A permanent
installation requires a Mozilla-signed XPI installed through `about:addons`. The build
ZIP is unsigned; do not disable signature enforcement. No signing or publication is
performed by the build or installer.

## Drafts and retries

Drafts are stored in the extension's local storage, including title, URL and editor
fields. They are never synced by this extension. Closing the panel preserves the draft.
After a confirmed save, its stored page data is removed. A pending draft takes
precedence over a new shortcut capture, so switching tabs cannot silently overwrite your
edits. After a browser restart, capturing the same URL recovers an orphaned draft.

A save freezes its request UUID and fields before contacting Tasktjik. An uncertain
reply leaves a **Retry original save** action. Retrying uses exactly the same request,
allowing Tasktjik to deduplicate it. Local model-validation failures reopen the fields.
Discarding an uncertain submission warns that it may already exist in Reading. Drafts
disappear if the extension is uninstalled or its storage is cleared.

## Validation

```sh
dev-check bash -c 'node apps/tasktjik-browser/tests/background.cjs && python3 -B apps/tasktjik-browser/tests/host.py'
dev-check apps/runtjik-browser/node_modules/.bin/web-ext lint --source-dir apps/tasktjik-browser/dist/extension --warnings-as-errors
```

The repository's existing `web-ext` installation is a development validator only; the
installed Tasktjik extension and native helper have no Runtjik dependency. Synthetic
bridge tests do not prove live browser shortcut or sidebar behavior.
