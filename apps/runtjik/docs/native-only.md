# Native-only Runtjik and Tasktjik

Runtjik no longer has a Python backend selector or a Python provider host. Only
bundled native providers are accepted. The old runtime files are removed during
installation. The obsolete source implementations have also been deleted.
Only test drivers remain in Python; protocol parity uses frozen synthetic JSON
fixtures. The Rust contract JSON is a checked-in build input.

Tasktjik owns the shared Rust client library used by both applications. Its CLI,
QML state requests, HTTPS/socket transport, enrollment, device management, local
store, SSH endpoint and notification scheduler now run natively. SSH authorities
must provide `.local/lib/tasktjik/tasktjik-api serve`; there is no Python fallback.
Existing task records and configuration are preserved. Enrollment explicitly
selects HTTPS and clears an old socket setting while preserving other TOML fields
and saving a private backup.

Reminder delivery uses native IPC to Notjik's existing notification service;
it does not spawn the Python `notjikctl` client. A claim is acknowledged only
when Notjik accepts the card, and failed delivery releases the claim. Task titles
and contents travel in a private temporary request file, not process arguments.
Notjik is a separate application; this change does not rewrite its own server.

These two applications’ build/check scripts use Cargo and shell only. Browser native-host registration is
implemented in Rust, exposed through `browser-extension/install-host.sh`.
Installers atomically replace executables so an active capture worker or authority
can finish on its original inode. No archive worker is stopped. Existing browser
registrations are updated to the native host.

## Validation

Run the workspace build and static checks, then the Rust and executable client
suites. Python is required only to run development tests:

```sh
bun run build
bun run check
(cd apps/runtjik && bun run test)
(cd apps/tasktjik && bun run test)
python3 -B apps/runtjik/tests/native_install.py
python3 -B apps/runtjik/tests/rust_ui.py
```

The installation fixture uses a temporary home, checks that no Python payload is
shipped, validates native registration and both installed CLI wrappers, and
verifies that replacing the binary does not interrupt a running backend. Tasktjik
fixtures exercise enrollment, stable device IDs, native SSH, standalone actions,
private notification transport, failed-delivery release and successful ack.
Historical Python/Rust measurements remain in `native-runtime.md`; new UI
benchmarks measure Rust only because the Python UI selector has been removed.

## Verified installation

Installed into the user's local application directories after workspace build and
check passed. The live Runtjik launcher reported ready and visible on Rust, then
was dismissed normally. The previous Python browser host was retired by verified
process identity and the extension reconnected to a Rust host. The installed
Tasktjik client successfully read its configured HTTPS authority without exposing
task contents. A private compositor also verified the installed standalone
Tasktjik window using synthetic demo state. Both installed payloads contain zero
`.py` or `.pyc` files. The existing reminder timer was retained; no reminder or
archive worker was manually started or restarted.

Validation passed: 129 Runtjik reference tests, 48 protocol parity comparisons,
12 native migration tests, 11 Runtjik Rust tests, 9 Tasktjik reference tests,
4 native client integration tests, 3 shared-library tests, 10 authority tests,
and the atomic installation fixture. Real QML validation passed in the private
compositor. These checks do not claim a rewrite of Notjik's separate server.

## Source cleanup

The obsolete Python runtime and tests of its removed in-process APIs have been
removed. Protocol behavior is checked against 54 frozen synthetic outputs,
independently captured before removal. Native unit, executable, installer, and
private-compositor tests remain. Benchmarks now run only the native backend;
historical measurements remain in the migration reports.

Cleanup verification: 54 frozen protocol cases, nine native migration cases,
the installed-payload removal check, the countdown UI check and four Tasktjik
native CLI tests passed. The migration cases were run against a binary built from
committed main (`5a22023`), using `RUNTJIK_TEST_BINARY`, because concurrent,
uncommitted Tasktjik cache work changes local-authority and asynchronous request
semantics. Those cache changes are outside this cleanup; these results do not
claim validation of that work.
