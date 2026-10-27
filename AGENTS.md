# TjikOS

Turborepo with Bun workspaces. Applications live in apps/; shared packages in
packages/. mailtjik is a Go/Charm TUI with a Python archive backend.

Use bun install and bun run build. bun run check is a static syntax check.
Keep all mail, credentials, account configuration, indexes, tokens and captures
outside the repository. Demo recordings must use only the synthetic --demo
fixtures. Do not interrupt or replace pinned running archive workers during
application updates. Generated dist/ and private runtime files are ignored.

Use gofmt for Go and four-space indentation for Python. Public commands are
mailtjik read and mailtjik archive; no command opens the mode selector.
