# TjikOS

Turborepo with Bun workspaces. Applications live in apps/; shared packages in
packages/. mailtjik is a Go/Charm TUI with a Python archive backend.

Use bun install and bun run build. bun run check is a static syntax check.
For every app developed in this repository, build and validate the changes, then
replace the locally installed app using its supported installer before considering
the task complete. This is the default workflow and needs no separate user
confirmation. Install only the affected apps, verify that the installed files
match the validated build, and report any restart or reopen needed to use it.
Preserve user data and running work, including the pinned archive workers below.
Keep all mail, credentials, account configuration, indexes, tokens and captures
outside the repository. Demo recordings must use only the synthetic --demo
fixtures. Do not interrupt or replace pinned running archive workers during
application updates. Generated dist/ and private runtime files are ignored.

Use gofmt for Go and four-space indentation for Python. Public commands are
mailtjik read and mailtjik archive; no command opens the mode selector.

On managed desktops, run heavy validation through `dev-check COMMAND ...`,
including direct Vitest, Svelte checks and Cargo builds. It shares one user lock
across agents and repositories. Do not set DOTJIK_DEV_CHECK manually or bypass
a failed resource scope. Watch commands retain the slot until stopped; avoid
them for one-shot checks.

## Test window placement

- Launch all visible testing, browser automation, preview, and demo windows on
  Hyprland workspace **7**, silently, without switching the active workspace or
  stealing focus. This includes child windows and dialogs.
- Set placement before launching (for example, with a narrowly scoped Hyprland
  launch rule using `workspace 7 silent`); do not launch on the current workspace
  and move the window afterwards. Verify placement when a visible window is needed.
- Preserve the user's existing windows, tabs, and activity. Do not move or repurpose
  an existing user window for testing; use a dedicated test window on workspace 7.
- Prefer headless checks when they provide the required evidence. If the tool cannot
  guarantee silent placement on workspace 7, use a headless or isolated alternative
  rather than opening a disruptive window. Only focus or switch to workspace 7 when
  the user explicitly asks.
