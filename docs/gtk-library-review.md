# GTK library review

The technical review and application validation are complete for the scope below. The
final commit audit identified release follow-ups listed below. Shared-library license
texts and notices have passed extracted-archive validation.

## Current assessment

Current publication scope is `tjik-desktop` and `tjik-gtk` only. The Tasktjik view
library remains internal with `publish = false` until the applications have stabilised
and the maintainer reconsiders. Historical three-crate release entries below are
superseded by this decision; publication remains pending.

The shared core now has public adapter boundaries and bounded background work:
`tjik-desktop` owns toolkit-independent state/actions; `tjik-gtk` owns presentation
primitives; `tasktjik-gtk` is explicitly a Tasktjik-domain view, with commands and
preview transport supplied by its host. Additional desktop or preview adapters can
implement the public interfaces without changing the shared crates. GTK widgets remain
main-thread objects, while desktop transport, preview transport and image processing run
off-thread.

| Requirement                | Current evidence                                                                                        | Remaining limit                                                                                                                |
| -------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| External adapters          | Public desktop integration test, arbitrary-model GTK factory doctest, public preview-provider doctest   | Only Hyprland and synthetic desktop adapters are exercised                                                                     |
| Bounded work and lifecycle | Latest-state subscriptions, image queue/cache limits, preview cancellation, motion/disposal tests       | Native image-decoder intermediate memory is not hard bounded                                                                   |
| Collection performance     | Rendered 2,000-record navigation/reflow tests; bounded realized cards; release profiles                 | Single synthetic timings, not statistical performance guarantees                                                               |
| App integration            | Runtjik full GTK fixture/build/install/readback; Notjik lifecycle/daemon fixture/build/install/readback | Bartjik non-display and selected synthetic card/transition fixtures pass; concurrent migration changes remain separately owned |
| Distribution               | Three extracted archives tested offline, desktop default/all features, public API examples              | Native GTK dependencies must be supplied; no registry upload or availability check                                             |
| Publication metadata       | Descriptions, READMEs, verified repository URL, versioned shared dependency                             | MPL-2.0 selected; license texts and notices verified in extracted archives                                                     |

Linux with GTK 4.12+ and gtk4-layer-shell is the supported native target. No claim is
made for other compositors, platforms or toolkits. Explicit legacy renderer paths and
asset directories still exist; the default native routes do not require the Quickshell
renderer. This thread is committed as a signed atomic series. No push or publication has
been performed.

The validation record below is chronological. Later entries supersede earlier open
findings and intermediate performance measurements.

## Follow-up status

- Image burst loading is fixed in `d71d842`. Waiting requests hold one weak reference
  per live widget, without copied source strings or textures. Rebinding coalesces
  requests and prunes dead targets; one decoder runs at a time. This replaces the
  64-entry eviction policy that could lose early grid icons. The isolated committed
  regression passes 100 simultaneous binds, repeated rebinds and 200 discarded widgets.
- Application notice distribution is implemented in `17866be`. All four builds bundle
  their own license/notice files and the reviewed linked Tjik components. Full supported
  installers and Bartjik's GTK/artwork/notice-only modes pass isolated-home readback.
  The actual installed notices match the distributions (8 Bartjik, 6 Notjik, 10 Runtjik
  and 2 Tasktjik files). Installed Runtjik, Notjik and Tasktjik binaries match their
  builds. Bartjik received notices through `--licenses-only`, preserving its binary and
  running service.
- The maintainer selected `tjik-desktop`, `tjik-gtk` and `tjik-task-gtk` for initial
  `0.1.0` releases. Namespace metadata and consumer aliases are committed at `1791bfa`.
  Clean archive tests and the independent crates' registry dry runs pass. Cargo's first
  publication command stopped before upload because no token was configured, and the
  maintainer requested that publication remain pending. See
  [the release receipt](library-release-receipt.json). No crate was uploaded.
- The separate Bartjik performance/tray/theme work and icon-fade changes remain
  uncommitted and outside this follow-up's scope.

The four-application Bun build and full Runtjik GTK fixture pass at `/tmp/bc-dlx3etdj`.
Supported installer tests and all three extracted archive suites pass in
`/tmp/tjik-followup-installer-test.log`; the exact committed image-loader tests pass in
`/tmp/tjik-followup-committed-images.log`. Reopen Runtjik to load the image-loader
update; Notjik can load its replacement binary at its next normal restart.

The isolated committed implementation at `71e6665` passes all three extracted-library
archive suites (including desktop capture and public adapter examples), release
`cargo check --all-targets --features gtk-ui` for Runtjik/Notjik/Bartjik, and all 40
Tasktjik tests. These checks exclude the concurrent working-tree changes above. The
subsequent documentation commit changes no implementation. Every commit path list and
the final contents of all 85 thread-owned paths were verified; concurrent changes remain
unstaged. Validation log: `/tmp/tjik-thread-commits/validation.log`.

## Scope and completion evidence

Review the GTK applications and their shared crates for consistent contracts, bounded
background work, main-thread responsiveness, lifecycle cleanup, and extensible adapter
boundaries. Completion requires validated fixes, downstream API checks, package and
licensing review, application builds, supported local installation, installed artifact
comparison, and rendered interaction checks where behavior changes.

The existing Bartjik and Notjik worktree changes are concurrent migration work and must
be preserved. Passing a shared-crate test alone does not validate those applications.

## Findings from source inspection

- External desktop adapters could implement the public traits but could not construct a
  subscription. The private event hub is now exposed as `EventSource`, preserving the
  built-in adapters' bounded latest-state semantics. A downstream integration test
  checks public construction, revision assignment, coalescing, late subscription, and
  pending-event delivery before closure. All 14 desktop contract tests pass.
- Notification sync presence/status, global removal, and Tasktjik local reminders still
  invoked Quickshell. These paths now use `notjik_sync::native`, extracted from the
  native Notjik client. The CLI and sync consumers share endpoint selection, framing,
  limits, timeouts, and response validation. Submitted mutations are never retried.
  Global removal remains distinct from dismissal. Four new transport tests cover native
  operations, malformed/rejected responses, missing endpoints, and limits.
- Default GTK routing was traced in both launchers. Runtjik selects the GTK worker and
  native countdown; Bartjik handles `.panel` intents in its native deck and returns
  before backend dispatch. Explicit legacy renderer/controller paths remain for the
  documented comparison/rollback workflow. Notification sync's active legacy calls were
  fixed above; a text search alone is not evidence of a GTK dependency.
- Notjik still polled `hyprctl activewindow` and `monitors` twice per second. It now
  receives the desktop adapter's bounded subscription once and drains it without
  blocking on the GTK thread. The repeating subprocess worker is removed. Snapshot focus
  is explicit, and detached-card workspace tracking uses opaque IDs rather than
  integer/name conversion. Desktop tests pass, including absent/racing focus. Notjik's
  63 unit tests and display lifecycle check pass. The daemon fixture now uses compositor
  geometry for native windows and verifies stable hover geometry, widget identity,
  mixed-scale moves, workspace visibility, and output removal.
- Settled-scroll hover selection previously scanned and parsed every JSON model entry.
  Shared result rows now hold a weak reference to their live GTK list item and read its
  current position directly. Unbound items are rejected; the row does not retain its
  list item. Selection no longer depends on JSON payloads or unique model IDs. The
  rendered interaction fixture passes scroll, hover, fractional wheel, keyboard,
  stale-result, and reopen checks. Legacy Quickshell comparison is now opt-in.
- Image file I/O and decoding now use one worker operation at a time with 64 pending
  requests, stale-row rejection, encoded input limits, and bounded output textures.
  Small sources retain their size; larger ones downscale to 256×256. Embedded cache
  pixel storage is bounded to 8 MiB. Platform decoder intermediate allocations remain
  outside this bound. Three decoder tests and the final rendered GTK fixture pass.
- The shared result factory now accepts a typed `Presentation` callback over arbitrary
  GObject model items. Runtjik owns JSON decoding, provider names, icon precedence,
  emoji interpretation, and clipboard artwork resolution in `result_adapter.rs`.
  `tjik-gtk` no longer depends on serde_json. A plain-string public API example and
  provider-mapping tests cover the boundary. All pass, along with the rendered fixture.
- Word/capture styles now live in Runtjik, and task/reading styles live in tasktjik-gtk.
  The primitive theme supports composed application CSS through validated named colors.
  The composed build and rendered interaction fixture pass.
- Closing pulses now have explicit callback ownership. Unmap, reset, and drop cancel
  immediately; remap resumes only pending work. The isolated-display regression passes
  100 rapid resets, unmap/remap, and destruction without orphan animation. Result rows
  dispose their controllers explicitly. The full GTK interaction fixture also passes.
- `packages/tjik-desktop/Cargo.toml` lacks a license declaration, while GTK and terminal
  crates declare GPL-3.0-or-later. No repository-root license was found. The intended
  shared-library license has been requested from the maintainer; package archives have
  been checked, but licensing still prevents a publication-readiness claim.

## Validation record

The EventSource changes and external integration test pass targeted rustfmt checks.
`git diff --check` passes. The desktop test command completed through `dev-check`: 12
unit tests and one external-adapter integration test passed. Sandbox access to the user
scope bus failed initially; the same scoped command succeeded with host access.

The native transport changes pass all 22 notification-library tests, all 40 Tasktjik
tests, and all nine Notjik executable acceptance checks using synthetic state.
`dev-check bun run build --filter=@tjikos/notjik` and the subsequent Tasktjik build
passed. The supported installer installed both applications. SHA-256 comparisons confirm
installed Notjik core/GTK/sync binaries, launchers, bundled layer-shell library, and
Tasktjik executable/launcher match the build artifacts. No running service was
restarted; persistent processes need a restart to load these builds. No rendered
interaction claim is made for this transport-only change.

Runtjik built successfully after an offline lockfile reconciliation removed dependencies
for disabled desktop capture features; no dependency versions were upgraded. Its GTK
fixture passed on a private display whose parent was verified on workspace 7 without
focus theft. The synthetic screenshot was inspected. Fixture timing was 255 ms cold,
37–40 ms warm control round trips, and 47,139 KiB combined PSS (Cairo, 15 synthetic
apps); these are not before/after performance measurements. The supported installer
completed, and installed core/GTK binaries, launcher, and layer-shell library match
SHA-256 hashes of the tested artifacts. Runtjik will use the new build on its next open.

The asynchronous image-loader build passed the GTK favicon, thumbnail, stale-result,
scroll, keyboard, and reopen fixture. Its final screenshot was inspected. Final fixture
PSS was 48,841 KiB, cold startup 266 ms, and warm control round trips 41–42 ms; no
statistical performance comparison is claimed. The supported Runtjik installer ran and
installed artifacts again match build SHA-256 hashes. The decoder checks are available
through `dev-check bun run --cwd packages/tjik-gtk test`.

The typed presentation boundary passes three shared-library tests, its downstream
plain-string API doctest, and both Runtjik adapter tests. The final Runtjik build and
GTK interaction fixture pass; the rendered screenshot was inspected. Runtjik was
installed using the supported installer and its core, GTK executable, launcher, and
layer-shell library match the build hashes. Reopen Runtjik to use the update.

Cargo package inventories were inspected for `tjik-gtk` and `tjik-desktop`: source,
README, bundled protocol/Lua support, and tests are included; target/build artifacts are
excluded. Descriptions and README metadata were added. Package compilation outside the
checkout is covered below; final licensing/release policy remains unresolved.

Local `.crate` archives were extracted under `/tmp/tjik-package-review.5iK67S` and
tested offline without checkout-relative Rust sources. Desktop default tests pass (13),
as do all-feature tests (16, including capture buffer/deadline checks). GTK tests pass
(three unit tests plus the public API doctest). Native GTK/layer-shell dependencies came
from the provisioned development environment; this is not a clean-distribution install
test. No package was published.

The composed-style Runtjik build passed the synthetic GTK interaction fixture, with
workspace-7 placement/focus checks. The screenshot was inspected and no CSS parser
errors were found in fixture logs. Supported installation completed; installed core, GTK
executable, launcher, and layer-shell library match build hashes. Reopen Runtjik to use
the update.

The lifecycle fix built successfully and passed its dedicated display regression and
full GTK interaction fixture. Both fixtures verified silent workspace-7 placement. The
rendered screenshot was inspected, installation completed, and installed artifacts match
build hashes. Reopen Runtjik to load the change.

Runtjik route inspection confirms the default launcher goes to `runtjik-gtk`; its worker
sets `RUNTJIK_FRONTEND=gtk`, selecting the native capture countdown. The Quickshell
controller remains reachable through the explicitly selected legacy launcher path.
Bartjik routes native `.panel` actions before legacy dispatch. The broader application
audit remains open, including the Tasktjik-specific boundaries below.

Current Notjik adapter migration validation: desktop contracts pass (14 total), Notjik
unit tests pass (63; two display-only tests excluded), and the dedicated host lifecycle
test passes on the private display. The application build passes. The daemon fixture's
native geometry and drag targeting were reconciled without relaxing identity, movement,
workspace or cross-output assertions. It exposed a real hover resize shift: detached
windows now reserve hidden-control height before mapping and retain that allocation when
controls collapse. The stack limits input to the visible card, leaving unused space
click-through. No corrective compositor move runs on the GTK thread.

The full daemon fixture passes at `/tmp/notjik-native-gtk-bpkk27uw`; the rendered
expanded card was inspected. Its third drag explicitly re-enters with pointer motion
after a compositor-owned grab; a zero-distance pointer command did not establish the
next grab in the private compositor. A speculative gesture reset did not resolve that
case and was removed. The final source build and daemon fixture pass, including an
additional origin check after footer collapse. Final artifacts are at
`/tmp/notjik-native-gtk-hgtp3rj4`; the expanded-card screenshot was inspected. The
supported Notjik installer completed. SHA-256 readback matches core, GTK, sync, both
launchers, and bundled layer-shell library. Running services were not restarted; Notjik
needs a restart to load the update.

## Additional library boundaries requiring follow-up

`tasktjik-gtk` remains a domain-specific view package, with its preview transport now
injected through `PreviewProvider` and typed `PagePreview` values. Runtjik owns helper
path discovery, subprocess I/O, deadline and cancellation. The view deduplicates current
references, runs one four-URL batch at a time, rejects stale/unsolicited results, and
cancels and releases its future on disposal. Pending references are queued once per
snapshot rather than rescanning the collection after each batch. Completion updates
existing card widgets instead of rebuilding the collection. Reading pictures now share
the bounded asynchronous decoder with launcher images.

The synthetic display test covers request bounds, stale results, disposal, unchanged
card identity, picture decoding and stale-picture rejection. A separate host test proves
that releasing the future terminates its helper; six Python tests cover cache behavior
and cancellation through browser cleanup. The crate has README/description metadata and
a versioned local dependency; release order remains `tjik-gtk` before `tasktjik-gtk`.
The final build and full GTK interaction fixture pass; the reading screenshot at
`/tmp/bc-10eycb5p/gtk-reading.png` was inspected. Supported installation completed for
Runtjik and its Tasktjik helper. Installed core/GTK binaries, launchers, layer-shell
library and both copies of the preview helper match build SHA-256 hashes. Reopen Runtjik
to use the changes. The fixture measured 262 ms cold startup, 34–40 ms warm control
round trips and 48,752 KiB combined PSS; this small synthetic fixture does not establish
large-collection performance or a before/after speedup.

Task and reading views rebuild all matching cards on a changed snapshot rather than
using the virtualized result factory. They skip unchanged task renders and preserve an
active editor, but no large-collection latency or memory bound has been established.
These views should not inherit the launcher's virtualized performance claims.

Final shared archive checks pass from `/tmp/tjik-package-review.3sn5qc`: desktop has 14
default-feature checks and 17 all-feature checks; GTK has three decoder checks and its
public factory doctest. The display-only motion regression is excluded from this archive
run and was validated separately earlier. Focused Markdown formatting and
`git diff --check` pass. The repository-wide Markdown check still reports unrelated
formatting in Bartjik's CPU profile document and the Notjik/Runtjik READMEs.

Tasktjik attachment reads/base64 conversion, full-image decoding and clipboard PNG
encoding now run on a worker. One job is admitted per view. Navigation/unmap invalidates
completion; file attachments additionally require the original editor object and task
revision. Regular-file reads are capped while reading, and oversized encoded image
responses are rejected before decoding. Native decoder intermediates remain outside the
encoded-input bound. Unit/display tests pass for byte bounds, non-files, worker texture
encoding/decoding, responsiveness during blocked work, duplicate suppression and stale
completion. The final application build and full GTK interaction fixture pass; the task
screenshot at `/tmp/bc-m1kt23dv/gtk-tasks.png` was inspected. The supported installer
completed and installed core/GTK binaries, launcher and layer-shell library match build
hashes. Reopen Runtjik to load the changes.

The explicit rendered collection profile now provides a baseline (release, Cairo,
private workspace-7 display, one run): tasks at 100/1,000 records used 2,979/27,279
widgets and took 54/468 ms to first paint (10/96 ms in synchronous update); reading used
1,472/12,272 widgets and took 25/185 ms (3/35 ms update). This confirms that full widget
construction is a material scaling limit. Virtualized or incremental collection
presentation remains required before claiming large collections are responsive.

Reading now uses a virtualized list of typed heading/card rows. Grid rows hold up to
three cards according to allocated width; a small viewport widget coalesces width-change
notifications without frame polling. Logical navigation covers all records independently
of bound widgets, and unbind releases card/preview references. A visible record remains
in view when rows regroup. The 2,000-record display regression passes off-screen
selection/action dispatch, grouping, filtering and an actual compositor resize from
three columns to one. The test uses a running GLib main loop to dispatch Wayland
configure events; merely polling pending sources missed those events.

The same release/Cairo profile measured reading at 100/1,000 records with 1,571/2,936
widgets and 27/44 ms first paint (4/13 ms synchronous update), compared with the earlier
1,472/12,272 widgets and 25/185 ms. These are individual synthetic runs, not statistical
benchmarks. Task results remain approximately 27,279 widgets and 461 ms at 1,000
records; task collection virtualization remains open. The final Runtjik build and full
GTK fixture pass. The reading screenshot at `/tmp/bc-nwtzyc66/gtk-reading.png` was
inspected; the supported installer completed, and core/GTK binaries, launcher and
layer-shell library match installed SHA-256 hashes. Reopen Runtjik to use the change.

Task collections now use virtual columns with one outer scrollbar. A prefix tree keeps
measured row heights and locates visible records without scanning all tasks. Only the
viewport and overscan rows own cards; focused and dragged cards remain pinned until
released. Task navigation tracks the complete logical order using weak column/card
references. Existing card controls, group/item drop targets and mutation revisions are
preserved. Date-filter horizon calculation is hoisted out of the task loop.

The original nested-native-list prototype was rejected: an actively rendering display
exposed repeated layout requests between the inner and outer adjustments. The earlier
coordinate-only result did not establish correctness. GTK regression/profile runners now
create a headless output inside the private compositor so hidden workspace-7 parent
windows do not stop frame delivery. The replacement uses only the board adjustment. The
2,000-task regression passes off-screen focus, return navigation, filtering, mutation
ID/revision preservation and real window resizing across group reflow. A variable-height
fixture checks visible row geometry, tall-card settling, focus/pin retention and
release. Preview, media and reading regressions also pass on this display.

The release/Cairo profile measured 468 task widgets for both 100 and 1,000 records, with
12/13 ms first paint and 0/4 ms synchronous update. Reading measured 1,571/2,936 widgets
with 25/47 ms first paint and 4/14 ms update. These single runs establish bounded widget
construction, not a statistical speedup: the earlier eager-task baseline also used a
different frame-delivery configuration. The private profile is at `/tmp/bc-s9miv79x`.

The final application build and full Runtjik GTK fixture pass after the resize/tall-card
fixes. The task screenshot at `/tmp/bc-zl1pl0ze/gtk-tasks.png` was inspected. Supported
installation completed; installed core/GTK binaries, launcher and layer-shell library
match build SHA-256 hashes. Reopen Runtjik to load this version. The installed GTK hash
is `79cf5f9314be787e717e416f3eee31cbd65259d976cf7b80e9380a4f32e154b0`.

Refreshed extracted package checks pass at `/tmp/tjik-package-review.Vg4keB`: desktop
has 13 unit tests plus the external-adapter contract test; GTK has three decoder tests
and its public-factory doctest. The display-only motion test remains outside that
archive run. Licensing and publication metadata remain unresolved; these checks do not
establish publication readiness or validate concurrent Bartjik changes.

All three library manifests now identify the repository verified from `origin`:
`https://github.com/tijptjik/tjikos`. The reproducible archive check is
`dev-check bash packages/tjik-gtk/scripts/test-packages.sh`. Its run at
`/tmp/tjik-package-review.B22ul2` passes desktop default/all-feature contracts, GTK
non-display tests and factory doctest, and Tasktjik non-display tests plus its new
public preview-provider example. The Tasktjik dependency is patched only to the
extracted GTK archive; no checkout-relative source is required by the extracted build.
Registry availability and legal licensing are not established by this test.

The shared motion regression now uses the same actively rendering private-display helper
as the collection tests. It asserts that opacity changes before exercising
reset/unmap/remap/drop, and passes. This strengthens the previous callback-ownership
proof by requiring actual animation frames. These changes affect test infrastructure,
package metadata and documentation, not installed application behavior.

The final archive runner stages Git-visible inputs before packaging, so Cargo cannot
rewrite working-tree locks during path-dependency normalization. The run at
`/tmp/tjik-package-review.20ISl3` passes with locked offline extracted tests; each
archive is checked against its source lock for dependency names, versions and checksums.

Bartjik's current source passes 29 core tests and 42 GTK non-display tests. Its private
executable audio transport fixture also passes. Supported synthetic
notification-history, workspace-card and panel-transition fixtures pass on workspace 7
without focus theft, provider startup or nested compositors. The rendered
history/workspace/transition artifacts were inspected. These checks cover the shared
card/desktop integration and lifecycle paths, not a new whole-service idle benchmark. No
Bartjik source edits, installation or service restart were performed as part of this
review turn; the concurrent migration changes remain separately owned.

Current Runtjik and Notjik core/GTK binaries and bundled layer-shell libraries still
match their installed counterparts. This turn changes package metadata, documentation,
public API examples and test infrastructure only. The outstanding publication gate is
the maintainer's shared-library license decision: GTK packages declare GPL-3.0-or-later,
while `tjik-desktop` and the repository root have no corresponding license declaration.

The maintainer selected MPL-2.0 for `tjik-desktop`, `tjik-gtk` and `tasktjik-gtk`. Each
package now contains the full MPL text and an explicit source notice, retaining standard
secondary-license compatibility. `tjik-desktop` also preserves the bundled Hyprland
protocol's BSD-3-Clause notice; its package SPDX expression includes that additional
license. Third-party dependencies retain their existing licenses.

GPL-3.0-or-later is the recommended compatible application license for distributed
copyleft desktop applications. This recommendation has not changed application license
metadata or assigned a repository-wide license. No package has been published.

Final licensed-archive validation passes at `/tmp/tjik-package-review.aOvjXN`. All three
normalized manifests have the expected SPDX license expressions, and extracted
`LICENSE`/`NOTICE` bytes match their package sources. Desktop default/all-feature
contracts, GTK unit/API tests and Tasktjik unit/preview-provider API tests pass again.
This resolves the shared-library licensing gate and completes the reviewed release
readiness work. Application licensing remains the separate GPL-3.0-or-later
recommendation above; registry publication was not requested or performed.

The maintainer subsequently accepted GPL-3.0-or-later for the reviewed applications:
Bartjik, Notjik, Runtjik and Tasktjik. Their source package manifests now declare that
license, with full GPLv3 license texts and explicit “version 3 or later” notices.
Third-party source and asset notices remain in place. The shared libraries retain
MPL-2.0 (plus the desktop package's bundled BSD protocol notice).

## Public documentation and hotfix maintenance

The public GTK README now addresses third-party developers: native dependencies,
consumer API examples, styling, image loading and lifecycle contracts. Maintainer
validation and release ordering live in `docs/library-release.md`, and the repository
instructions keep publication status out of public package READMEs. Cargo author
metadata and source notices identify `tijptjik`; third-party copyright notices remain
unchanged. Updated archive checks and the README's compiled example pass at
`/tmp/tjik-author-readme-check.log`.

Bartjik was rebuilt from the latest worktree after hotfix `208f857`, passed 29 core and
42 GTK non-display tests, and was installed through its supported GTK installer. The
existing service was restarted and read back as active with a ready backend and three
bars. Running and installed GTK bytes match SHA-256
`3f94013656f6771e854423fb1b1efcfa0d5329e2e7b654d9cea1243fe1294103`. The installed core,
launcher and layer-shell library also match the build. Author notices were refreshed and
verified for all four installed applications without restarting the other applications.
The concurrent Bartjik work remains separately owned.

Cargo credential provisioning is complete. Hidden local input uploaded
`CRATES_IO_API_TOKEN` to the bws `dotfiles` project, and exact value readback verified
the upload without exposing the token. Dotjik commit `d8b1a01` enrolls Fi, Li and
portable clients with the `home:secrets` subscription by UUID; server roles stay
excluded. All 410 Dotjik tests passed with nine skips using synthetic credentials. Only
the local Cargo credential group was applied: the file is mode 0600, its value matches
bws, and the managed check converges. Other eligible systems receive it on their next
Dotjik apply; no remote application or registry publication was performed.
