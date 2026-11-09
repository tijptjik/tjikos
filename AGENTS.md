# TjikOS

## Response summaries

- Preface every final summary response with "# SUMMARY `r{sequence}`" with `{sequence}`
  is an auto-incrementing number.

## Repo

Turborepo with Bun workspaces. Applications live in apps/; shared packages in packages/.
mailtjik is a Go/Charm TUI with a Python archive backend.

## Dev flow

- Use bun install and bun run build. bun run check is a static syntax check.
- For every app developed in this repository, build and validate the changes, then
  replace the locally installed app using its supported installer before considering the
  task complete. This is the default workflow and needs no separate user confirmation.
  Verify that the installed files match the validated build, and report any restart or
  reopen needed to use it.
- Preserve user data and running work, including the pinned archive workers below.
- Keep all mail, credentials, account configuration, indexes, tokens and captures
  outside the repository.
- Demo recordings must use only the synthetic --demo fixtures. Do not interrupt or
  replace pinned running archive workers during application updates. Generated dist/ and
  private runtime files are ignored.
- Use gofmt for Go and four-space indentation for Python.
- Use Biome for Markdown: `bun run format:markdown:check` is read-only;
  `bun run format:markdown` intentionally rewrites Markdown and `llms.txt`.

## Testing

- On managed desktops, run heavy validation through `dev-check COMMAND ...`, including
  direct Vitest, Svelte checks and Cargo builds. It shares one user lock across agents
  and repositories. Do not set DOTJIK_DEV_CHECK manually or bypass a failed resource
  scope. Watch commands retain the slot until stopped; avoid them for one-shot checks.
- Launch all visible testing, browser automation, preview, and demo windows on Hyprland
  workspace **7**, silently, without switching the active workspace or stealing focus.
  This includes child windows and dialogs.
- Set placement before launching (for example, with a narrowly scoped Hyprland launch
  rule using `workspace 7 silent`); do not launch on the current workspace and move the
  window afterwards. Verify placement when a visible window is needed.
