# Desktop fixtures and terminal showcases

Bartjik, Runtjik and Notjik use native GTK fixtures. Their old desktop recording
implementations have been removed, including the Runtjik branch of the terminal suite.
Historical recording notes remain in `gtk-migration-history/showcase.md`; commands there
are not current instructions. Existing recordings and capture receipts are untouched.

## Native desktop verification

Finite visible tests belong on workspace 7, silently placed before launch, without
switching the user's workspace or stealing focus:

```sh
dev-check bun run --filter @tjikos/bartjik test:gtk
dev-check bun run --filter @tjikos/runtjik test:gtk-session
dev-check bun run --filter @tjikos/notjik test:gtk-session
```

These fixtures use synthetic data and private services. They do not establish physical
IME, hardware reconnect or mixed-DPI visual acceptance. See
[desktop acceptance](desktop-native-acceptance.md).

Notjik provides native `notjikctl demo record` for directed synthetic recordings; see
[its recording documentation](../apps/notjik/README.md). Establish approved test
placement before recording. A replacement directed Bartjik/Runtjik showcase recorder is
not part of this cleanup.

## Terminal showcase tooling

`scripts/suite_showcase.py` retains Mailtjik, Tjikup and Greetjik only. Its transport
and pointer utilities now live in `scripts/showcase_runtime.py`, with their own bus
configuration and pointer protocol. It has no imports from retired desktop recorders.

```sh
python3 -B scripts/suite_showcase.py mailtjik --plan
```

The terminal recorder still uses a QML presentation stage and an isolated compositor. It
is not an application frontend or a runtime dependency of the three GTK apps. Do not run
this lab recording harness on the managed desktop: its nested compositor does not meet
the current-session testing policy. Only the non-GUI plan and unit checks are used for
this cleanup. Existing masters, JSON receipts and review directories are never replaced
by recording commands.
