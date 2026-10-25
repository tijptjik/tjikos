# tooltjik

![Turborepo](https://img.shields.io/badge/Turborepo-EF4444?style=flat-square&logo=turborepo&logoColor=white)
![Bun](https://img.shields.io/badge/Bun-14151A?style=flat-square&logo=bun&logoColor=white)
![Go](https://img.shields.io/badge/Go-00ADD8?style=flat-square&logo=go&logoColor=white)
![Charm](https://img.shields.io/badge/Charm-terminal_tools-FF75B5?style=flat-square)

A Turborepo for personal terminal tools.

## mailtjik

**Preserve email in encrypted annual archives, then find and read it in the terminal.**

![mailtjik demo](apps/mailtjik/docs/demo.gif)

*Recorded with Charm VHS using synthetic mail only.*

- Participant, From/To/Cc/Bcc, subject, account and year-range search.
- Account, year and search-result counts.
- Glamour Markdown rendering, HTML conversion and bounded inline Kitty images.
- Encrypted annual archives, read-only capture, label exclusions and durable workers.

## Install

```sh
bun install
bun run install:local
```

Requires Bun, Go, Python, age, zstd and rclone; see the [application installation guide](apps/mailtjik/README.md#install).

## Setup

Open `mailtjik archive` → **Setup** to choose private working paths. Prepare retained sources, then publish encrypted annual collections. See [archive preparation and live account setup](apps/mailtjik/README.md#setup).

## Use

```sh
mailtjik          # choose a mode
mailtjik read     # find and read email
mailtjik archive  # prepare, capture and preserve
```

See the [full mailtjik README](apps/mailtjik/README.md) for features, key bindings, restoration and the VHS demo tape. `READMD.md` is also available with the requested spelling.

## Workspace

Applications live under `apps/`; shared libraries can live under `packages/`.
`bun run build` builds through Turbo; `bun run check` performs static checks;
`bun run demo` regenerates the synthetic-mail recording.

## tjikup

[Configurable script runner](apps/tjikup/README.md) with ordered stages, explicit
parallel groups, status rows, dry-run commands and completion notifications.
Requires only Python 3.10+. Workflows and fleet policy live in their own repos.

```sh
bash apps/tjikup/scripts/install.sh
tjikup --config /path/to/workflow.json --check
```
