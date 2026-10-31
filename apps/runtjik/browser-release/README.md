# Browser extension releases

The extension keeps the stable ID `runtjik-browser@tjikos.local`. Signed
installation survives browser restarts; `persistent: true` alone does not make
a temporary add-on permanent. No startup daemon or browser profile edits are
needed for a signed extension.

From the repository root:

```sh
bun install
bun run --cwd apps/runtjik browser:build
bun run --cwd apps/runtjik browser:dev
```

`browser:dev` launches a separate temporary Zen profile and reloads edited
extension code. Set `RUNTJIK_DEV_BROWSER=/usr/bin/firefox` to use Firefox.
This development mode does not modify the normal browser profile and does not
test signed-install permissions or restart persistence.

Builds lint with warnings treated as errors and package only manifest,
background script and icon. Artifacts are in `apps/runtjik/browser-artifacts/`
(ignored by Git and retained across native app builds). Normal native builds
also create an unsigned extension ZIP. They never submit or sign it.

## Publication and review

Version 0.3.0 has already been submitted:
[Mozilla submission](https://addons.mozilla.org/en-US/developers/addon/runtjik-browser-bridge/versions/6645275).
It was awaiting review at the last check on 2026-10-31. Do not resubmit that
version while waiting. After approval, follow the
[installation and restart checklist](../docs/browser-install.md) using the
[public listing](https://addons.mozilla.org/en-US/firefox/addon/runtjik-browser-bridge/).
Keep the submitted package unchanged; prepare extension changes under a new
version. The steps below describe the publication workflow for future releases.

1. Sign into the [AMO Developer Hub](https://addons.mozilla.org/developers/).
   Complete its developer agreement yourself if prompted.
2. Review `amo-metadata.json`, the privacy statement in
   `../browser-extension/PRIVACY.md`, and `reviewer-notes.md`. The metadata
   conservatively uses All Rights Reserved because this repository does not
   declare an extension license; choose another license before submitting if
   desired. Do not advertise Android or non-Linux compatibility. Disclose
   native Firefox/Zen, the separate Runtjik installation, and Hyprland focus.
3. Bump the manifest version and build the next release. Upload its ZIP from
   `browser-artifacts/`, choose listing
   on AMO, supply the metadata, privacy text and reviewer notes, and submit.
   This ZIP is not signed; installing it directly will not work permanently.
4. Once approved, install from the listing or download the signed XPI and use
   `about:addons` → gear → Install Add-on From File. Remove a temporary copy
   first if it masks the permanent copy. Keep signature enforcement enabled.
5. Verify the extension is enabled after quitting and restarting the browser,
   then query `/tabs` and `/history` in Runtjik. Do not call a release installed
   until this actual restart check passes.

For automated submission, obtain AMO API credentials and keep them in a secret
manager or a mode-0600 file outside this repository. Supply them through
`WEB_EXT_API_KEY` and `WEB_EXT_API_SECRET`, then run:

```sh
bun run --cwd apps/runtjik browser:sign:listed
# Or a signed development build, without updating the public release:
bun run --cwd apps/runtjik browser:sign:unlisted
```

On the configured maintainer machine, Dotjik manages `~/.config/runtjik/amo.env`
from BWS. Apply it with
`tjikup workflow files credentials apply --group runtjik-amo`, then export its
values only in the signing shell (`set +x; set -a; . ~/.config/runtjik/amo.env;
set +a`). Never paste the values into the repository or release notes.

The listed command supplies `amo-metadata.json`; reviewer/privacy information
must also be maintained in the Developer Hub. Signing can take time and may
require manual review. A successful lint or upload is not approval or signing.
Unlisted signing downloads an installable XPI. Listed releases are distributed
and automatically updated through AMO. Bump the manifest version for each new
submission; keep development and published version numbers distinct.

Runtjik/backend-only changes need no extension release while the native bridge
protocol stays compatible. Preserve the extension ID and native-host name.
Changing browser-side code or permissions requires a new signed release for
normal permanent installations.

## Validation

```sh
bun run --cwd apps/runtjik browser:lint
cargo test --release --locked --manifest-path apps/runtjik/Cargo.toml
bun run --cwd apps/runtjik test:browser
```

Build the native executable first. Tests use `apps/runtjik/runtjik-core`, or
`RUNTJIK_TEST_BINARY` when testing a separately built executable. The browser
suite also runs as part of `bun run --cwd apps/runtjik test`. It covers title/URL
matching, recency and the global cap, WAL visibility, profile-specific opening,
stale selections, host exit/crash/restart, and live/offline transitions. The
native-host tests require permission to bind private local Unix sockets. The JS
tests use the real background script with synthetic browser APIs and timers.
No tests here claim a real signed browser installation has passed acceptance.

Use synthetic profiles for tests. Never put profiles, history databases,
credentials, private screenshots or signing secrets in this repository.
