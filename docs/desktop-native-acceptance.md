# Native desktop acceptance — 2026-11-10

Bartjik, Runtjik and Notjik use GTK4. The retired renderer is removed from those app
sources, payloads, launchers and current documentation. Historical evidence lives in
`docs/gtk-migration-history/`; it is not a runnable installation or acceptance claim. No
notification ownership, history, credentials or capture data is migrated or reset.

## Acceptance evidence

Before cleanup, dev-check ran all three native unit suites, the desktop adapter
contracts and same-session fixtures. Bartjik passed 30 backend and 42 GTK tests; Runtjik
passed 34 backend and 7 GTK tests; Notjik passed 63 tests. The adapter passed 13 unit
tests and 1 external contract. Explicit ignored tests verified detached Bartjik and
Notjik windows on workspace 7. Native fixtures verified fixed-layout/clipped panel
transitions, hidden-bar input release and Runtjik stable cold result ordering. No test
switched the host workspace, moved the pointer or launched a nested compositor.

Logs for this run are under `/tmp/gtk-acceptance/`. These are ephemeral artifacts.
`apps/bartjik/docs/gtk-remaining.md` and `apps/runtjik/docs/acceptance.md` describe
retained coverage. Old frontend-only screenshots and event runners are retired, not
relabelled as native coverage. Domain reference fixtures remain where they still check
Rust contracts.

Physical IME, mixed-DPI/rotated-display sharpness, real device reconnects and live
permission agents were not exercised. Real pointer-driven handoff flicker still needs
visual review. Current cross-output behavior was accepted by the user; drag-back docking
is explicitly out of scope. No full physical-device acceptance is claimed.

## Matched startup and idle measurements

Earlier on the same date, three trials per renderer used matched configurations and
process-cold starts with caches retained. Startup is readiness, not first painted frame.
Memory is PSS; CPU percentages refer to one core. Panels were closed.

| App     | Earlier renderer startup | GTK startup | Earlier PSS |   GTK PSS | Earlier CPU | GTK CPU |
| ------- | -----------------------: | ----------: | ----------: | --------: | ----------: | ------: |
| Bartjik |                586.26 ms |   110.18 ms |  249.23 MiB | 84.74 MiB |      15.60% |   4.29% |
| Runtjik |                321.20 ms |   113.93 ms |  146.81 MiB | 81.37 MiB |       0.40% |   0.47% |
| Notjik  |                253.59 ms |    52.32 ms |   86.14 MiB | 18.26 MiB |       0.13% |   0.20% |

Bartjik used three real bars and whole-service cgroup accounting, including exited
helpers. Runtjik used 3,001 synthetic entries with 100 displayed, an ordinary 720×600
window on inactive workspace 7, and no outside-pointer hooks in the earlier renderer.
Notjik used zero notices with private state and a private bus. Its earlier frontend came
from revision `2c4352282b5b7cc5014aec1f6b9b7a002b35218d` with the current Rust worker.

Runtjik and Notjik memory/CPU account for roots and descendants; they may miss exited
helper CPU. Shared compositor/portal costs are excluded. These are not equivalent active
GPU measurements. Active thumbnail/media workload measurement remains outstanding. Raw
method, hashes and trials were retained at `/tmp/three-app-benchmark/comparison.json`.

## Release and rollback

Use each supported installer after building and checking through dev-check. Preserve
managed service policy with `TJIKUP_MANAGED_CONFIG=1`; install does not authorize
restarting unrelated services or recorder workers. Verify installed binaries/resources
against dist. A rollback now requires a saved complete native payload or a reviewed
historical checkout/build; there is no alternate renderer flag in these applications.

## Post-cleanup result

The complete package tests passed for all three apps. Bartjik passed 30 backend tests, 9
executable contract checks, 210 domain tests and its Bun algorithm suites. Runtjik
passed 34 backend tests, 53 frozen protocol contracts and its command, capture, history,
clipboard and browser regressions. Native media/tray, network and audio fixtures passed.
The recording fixture passed successful save, picker cancellation, countdown
cancellation and encoder failure. Native Runtjik and Notjik session fixtures passed with
service auto-activation disabled on their private buses. Local-development routing
passed 7 tests; showcase retirement passed 4 tests.

All three applications were built and statically checked through `dev-check`, then
installed with managed configuration preserved. The initial installed-byte comparison
verified 529 resource files plus native binaries; license/notice verification also
passed. `bun run check:native-desktop` confirms no retired renderer references or QML
payloads in these three source applications. The packaged text audit was also empty.

Bartjik restarted as PID 1914496 with three bars and a ready provider backend; its
running executable matched the installed bytes. Notjik kept PID 3518030 and its existing
paused state. Its newly installed binary differs from the running executable and will
apply on its next restart; active notification cards were not interrupted. Runtjik is
installed and opens the new payload on its next invocation.

Concurrent work rebuilt Runtjik after installation and changed source again during a
follow-up check. That later installation was withheld. The installed three-app binaries
were reverified against the successful deployment receipt, not the subsequently mutable
`dist` directory. Newer concurrent edits are outside this acceptance result.

Deployment receipt: `/tmp/gtk-acceptance/installed-hashes.json`. Bartjik rollback
payload: `/tmp/bartjik-native-rollback.r96J1o7l/`. These local artifacts are ephemeral.
Restoring a payload requires its matching wrapper/resources, not just one binary. No
commits were created by this cleanup.

## Residual dependency cleanup

Removed the unused shared notification-history package and its Bartjik build input.
Bartjik now documents its existing Notjik Rust card ownership and tracks those Rust
sources as build inputs. Removed the retired desktop showcase implementations; the
remaining suite records terminal tools only. Its isolated lab transport, pointer
protocol and bus configuration are owned by the showcase tooling. Historical showcase
documentation is archived under `docs/gtk-migration-history/`.

Extracted the unchanged page-preview helper into `packages/page-preview`, preserving its
GPL license and cache/protocol behavior. Runtjik no longer imports through Tasktjik's UI
directory. Tasktjik retains its existing helper path, and both builds dereference the
shared source into standalone installed files. The native-source guard now rejects
resource links through retired UI directories and the return of removed payloads.

Follow-up validation ran through `dev-check`: builds and static checks passed for
Bartjik, Runtjik and Tasktjik; both preview-helper suites passed 6 tests each; terminal
showcase tests passed 5 tests. Native launcher routing and Runtjik cold-order,
metadata-update, append and warm-reopen checks passed on workspace 7. The source guard
and `git diff --check` passed.

Installed Bartjik and Runtjik with managed configuration preserved. Used Tasktjik's
`--preview-helper-only` installer to avoid replacing its concurrently developed
frontend. Verified 399 installed payload files against captured build hashes, including
the helper and its license/notice; build artifacts remained unchanged during install.
Build-only browser-addon staging and externally installed service/application entries
are excluded from that count. Logs and receipts are in `/tmp/gtk-retirement-final/`,
including `installed-hashes.json`.

No bar or notification service was restarted. Runtjik uses the installed payload on its
next invocation; Tasktjik loads the helper on its next helper invocation. The physical
acceptance and active-workload measurement limits above remain unchanged. No commits
were created by this follow-up.
