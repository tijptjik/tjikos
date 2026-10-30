# Native Rust runtime

Notjik's launcher, CLI, JSON-lines worker, configuration, SQLite history, avatar
cache, lock observer, Telegram bridge, media renderer, voice recorder, and demo /
comparison / showcase orchestration are implemented in Rust. QML continues to
own rendering and notification behavior. TDLib and rlottie are loaded through
their native C APIs; FFmpeg remains the media encoder/decoder.

The distribution and installed runtime contain no Python modules or interpreter
fallback. `tests/reference/` retains the previous implementation solely as a
comparison oracle. Python test drivers are development tools, not dependencies
of any installed command. Existing user configuration and SQLite/account state
are reused without rewriting configuration or resetting Telegram authentication.

## Measurements

Measured on the migration host with 1,000 synthetic history events, a 100-card
history drawer, 10 warm process launches, and 10 steady-worker history requests.
First launch means the first process in the benchmark, **not** a flushed OS disk
cache. Each implementation receives its own identical synthetic database.

| Measurement | Python baseline | Rust |
| --- | ---: | ---: |
| First launch through initial history response | 27.49 ms | 3.45 ms |
| Warm launch median through initial history response | 27.32 ms | 2.91 ms |
| Steady-worker history request median | 2.11 ms | 1.76 ms |
| Worker resident memory | 23,952 KiB | 9,588 KiB |

These figures cover the worker, not Quickshell rendering or TDLib's account cache.
The history request includes SQLite scanning, deduplication, sorting, JSON output,
and test-driver decoding. It is not an isolated SQL lookup microbenchmark.

Reproduce after building:

```sh
python3 -B tests/benchmark_native.py --events 1000
cargo test --release --locked
python3 -B tests/native_contracts.py -v
```

## Acceptance boundaries

Native tests cover configuration validation, private IPC files, history retention,
non-executable snapshots, persistent pause state, fail-closed lock observation,
Telegram read boundaries, presence acknowledgements, reply validation, timeouts,
and frozen formatting/privacy fixtures. Executable tests convert actual synthetic
WebP/TGS/WebM media and exercise recording with a generated audio source.

Private-compositor acceptance covers D-Bus delivery, actions, queueing, reloads,
history, lock privacy, pause persistence, all 36 demos, and the Telegram media UI.
Synthetic UI tests exercise replies, failed-send draft retention, recording,
playback, transcription continuity, dragging, and lock cleanup. They do not send
messages to real Telegram recipients or mark real messages as read.

Native libraries can be installed with `notjikctl telegram setup`. The installer
extracts only native libraries from an existing local installation or pinned,
SHA-256-verified package archives. No virtual environment is needed at runtime.

## Installed verification

On 2026-10-30 the Rust runtime replaced the installed Notjik runtime after a
reflink rollback copy was saved outside the repository. Both user services are
active with zero restarts; Telegram reports authorized and connected using the
existing account. The worker loaded 2,506 history entries without an error,
resolved the unlocked desktop state, and retained the saved pause preference.
The user configuration checksum is unchanged. The existing isolated comparison
helper was also switched to Rust. Installed runtime files contain no `.py` or
`.pyc` files; worker, bridge, and comparison processes use `notjik-core`.

Final validation passed all 10 workspace builds and checks, 16 Rust unit tests,
9 native executable contract tests, and 141 retained reference tests. Both
notification servers passed all 36 paired demo cases. Native synthetic showcase
recording produced a verified H.264 video and chapter receipt and refused an
overwrite. Live verification checked service and connection health only; real
Telegram send/read actions were deliberately not exercised.
