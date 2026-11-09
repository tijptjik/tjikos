# GTK cutover preparation

The user accepted existing monitor movement/removal behavior and authorized switching
the main bar after interaction checks and the performance comparison. Drag-back docking
is explicitly out of scope. Do not ask for switch authorization again.

## Applied, 2026-11-10

The full supported installer ran with `TJIKUP_MANAGED_CONFIG=1`; the authoritative
Dotjik unit was then deployed and only `bartjik.service` restarted. The main service now
runs `bartjik --gtk serve --reserve`. There is no comparison gutter or workspace scope.
All three configured bars are present across DP-1 and DP-2. Backend readiness is true,
with no frontend or pointer error. Notjik remains active and was not restarted.

Source/dist/installed executable comparisons passed for both cores, the GTK renderer,
launchers and bundled layer-shell library. `/proc/MainPID/exe` resolves to the installed
`~/.local/lib/bartjik/bin/bartjik-gtk`. Reveal/hide and workspace-reminder commands
passed against the installed CLI; a final Bartjik-only restart cleared their temporary
states.

The backup of the previous installed library, launchers and unit is
`/tmp/bartjik-pre-gtk-gHpKpIi6` (ephemeral). The validated performance results and
binary hashes are in `gtk-performance-comparison.md`.

## Rollback

For this session, run `bash /tmp/bartjik-rollback-quickshell.sh`. Equivalently:

```sh
mkdir -p ~/.config/systemd/user/bartjik.service.d
cat > ~/.config/systemd/user/bartjik.service.d/90-renderer-rollback.conf <<'UNIT'
[Service]
ExecStart=
ExecStart=/usr/bin/fish -lc 'exec %h/.local/bin/bartjik --quickshell'
UNIT
systemctl --user daemon-reload
systemctl --user restart bartjik.service
```

Remove only `90-renderer-rollback.conf` and reload/restart to return to GTK. Preserve
other service drop-ins. A permanent rollback must also update authoritative Dotjik
policy; the command above is a local override.

## Verification

All checks used `dev-check`. Visible fixtures used the current compositor, with silent
workspace-7 placement established before mapping and no focus changes.

- GTK unit suite: 40 passed, four explicitly gated renderer fixtures excluded.
- Native detached-window fixture: passed, including independent ownership and handoff.
- Rendered fixtures: card transitions, notification history, audio, network,
  music/lyrics and workspaces passed.
- Private media/tray bus: owner loss, stale identities, registration, actions and menus
  passed. Private audio and network executables covered controls, stale stream IDs,
  Wi-Fi/Ethernet actions, stdin-only secrets and denied authorization.
- Idle/display rollback fixtures passed; desktop adapter suite: 12 passed.
- Dotjik validation: 407 tests passed, nine skipped. Its installer fixture was updated
  to include the new shared GTK/desktop cache inputs.
- Read-only live Spotify check resolved a valid exact artist destination and track-radio
  identity. No live playback or focus mutation was performed.

This does not claim physical IME input, real device unplug/replug or every compositor
frame sequence was exercised. Those remain normal-desktop acceptance observations.

Logs are in `/tmp/bartjik-cutover-checks.log`, `/tmp/bartjik-cutover-gtk.log` and
`/tmp/bartjik-cutover-final-checks.log`; these are ephemeral artifacts.

## Benchmark and remaining observations

The user approved the live-display bar-only benchmark. Three readiness samples and
20-second visible/hidden idle samples completed for each renderer with identical
configuration/output coverage and verified concealment. The first run caught excessive
D-Bus refresh work; the fix passed a new unrelated-connection-churn regression test and
the matched benchmark was repeated. Final build/static/assets checks passed.

Active thumbnail/music workload measurements are still pending the separate requested
permission to open panels on the current displays. They are not included in the idle
results. Physical IME/device recovery, detailed live flicker and cross-workspace hiding
remain normal-desktop acceptance checks. The switch is reversible and is not a claim
that those observations are complete. Spotify destinations were checked read-only;
actual navigation and playback remain for interactive review.
