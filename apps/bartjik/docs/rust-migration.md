# Rust runtime migration

Bartjik now ships one Rust executable for the backend, providers and process
helpers. Quickshell/QML remains the renderer. The old implementation is kept only
under `tests/reference` as a development oracle and is excluded from installation.
There is no backend selector or runtime Python fallback.

The port preserves independent/deduplicated provider workers, last-good state on
failed reload, bounded subprocess and HTTP work, stream process-group cleanup,
action validation and trusted argv, media/artwork/lyrics caches, private Spotify
credentials, tray attention/focus and audio/network controls. The native check
validates presets, themes and pinned font size/hash/license. The showcase invokes
the native backend with explicitly synthetic fixtures.

## Evidence (2026-10-31)

- Workspace `bun install --frozen-lockfile`, `bun run build` and `bun run check` passed.
- Rust tests cover 144 frozen reference cases plus malformed input/history guards.
  Seven executable contracts cover config parity, invalid config, reload retention,
  deduplication, independent workers, stream cleanup, demo isolation and init safety.
- All 189 test-only reference tests and 172 JavaScript layout/interaction assertions
  passed, along with the Codex timeline and battery assertions.
- Private compositor acceptance passed render (both themes), details, native panels,
  native tray/MPRIS, Spotify page navigation, lyrics position/seek/disconnect,
  media click/ticker/hover/disappearance, and artwork cache/stale/error transitions.
  Representative panel screenshots were inspected. All fixtures were synthetic.
- Local installation retained the managed config unchanged and a private rollback
  copy. The installed binary matches the built artifact. All 15 live providers
  reached ready with no error; the service process tree contains Rust helpers and
  dbus-monitor, no Python. Live checks did not change audio, network or VPN settings.
- Live validation caught and fixed the CoinMarketCap `quote` nesting; a frozen
  synthetic regression case now covers it.

Limits: the standalone interaction fixture still fails its tooltip visibility
assertion (it does not launch the backend), and full hover testing hits an edge
centering expectation. These are not claimed as passing. The showcase recording
could not start because the shared recorder references missing
`apps/bartjik/demo_pointer.xml`; its preparation unit test passes, but no new film
is claimed. Physical hotplug and live account-changing actions were not exercised.

## Worker measurements

Synthetic minimal preset, same screen/config, 11 launches per implementation and
30 synchronous config reload requests per worker. First launch does not mean
OS-cache-flushed. RSS excludes Quickshell and external services.

| Metric | Python reference | Rust |
| --- | ---: | ---: |
| First launch | 107.0 ms | 4.5 ms |
| Warm launch median | 80.5 ms | 3.1 ms |
| Config reload median | 31.5 ms | 30.2 ms |
| Steady RSS median | 27.9 MiB | 9.8 MiB |

The reload path includes the existing emission/debounce timing. This is not a
search lookup benchmark: Bartjik is a panel provider worker, not a launcher.
[Raw samples](benchmark-rust.json); reproduce with `python3 scripts/benchmark_native.py`.
