# GTK architecture

GTK is the default Notjik notification frontend. `notjik --quickshell` retains the
compatibility frontend; `notjik --gtk-cards --demo` runs the isolated synthetic gallery.
The Control Center/history UI remains deferred to a BarTjik panel.

## Ownership

| Module               | Responsibility                                                                                                                         |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `controller.rs`      | Toolkit-independent identity, replacement, queue bounds, grouping, expiry, holds, selection, privacy and notification commands         |
| `gtk/desktop_bus.rs` | Freedesktop Notifications name, methods, image decoding and action/close signals                                                       |
| `ipc.rs`             | Private bounded Unix-socket requests, exclusive ownership and stale socket recovery                                                    |
| `gtk/host.rs`        | GTK main loop, monitor routing, layer surfaces, detached cards, keyboard focus and intent dispatch                                     |
| `gtk/executor.rs`    | Command execution, Telegram requests, single voice player and recording helper                                                         |
| `backend.rs`         | Configuration reload, pause persistence, Wayland lock/idle observation, SQLite history, sync dismissal and Telegram dismissal dispatch |
| `backend_channel.rs` | Bounded in-process service requests and updates, explicit backpressure and disconnection                                               |
| `worker.rs`          | Existing stdin/stdout adapter for the compatibility frontend                                                                           |
| `gtk/` card modules  | Shared presentations and typed interaction intents                                                                                     |

The controller accepts elapsed time and explicit state rather than reading widgets or
spawning processes. Its effects go through the host to one backend or executor. Backend
history writes retain ordering across channel backpressure. Configuration, account data,
credentials, captures and history stay outside the repository.

## Startup and lifecycle

The native daemon claims `org.freedesktop.Notifications` without replacing an existing
owner. It serves `$XDG_RUNTIME_DIR/notjik.sock` with private permissions and an
ownership lock. `NOTJIK_SOCKET` overrides that path for isolated tests. `notjikctl`
prefers this socket and falls back to Quickshell only if no native endpoint is
listening; failed submitted mutations are never retried against another frontend.

Readiness and lock state begin unknown. Backend disconnection conceals cards and blocks
actions. Bus disconnection closes native surfaces and exits. Unknown idle state reports
120 seconds to avoid advertising an unobserved device as active to Notjik sync.

Native replacements always reset expiry. Hover, selection, interactions, pause and lock
hold expiry. Ordinary popups do not take keyboard focus; explicit selection enables
keyboard navigation, activation, dismissal and Escape to release focus. Drag release
creates a detached card with an expiry hold; live movement during drag is not yet
implemented. Monitor removal closes its surfaces.

The executor keeps playback and recording ownership separate from widgets. Concealment
stops audio and stops an active recording. Synthetic notifications never send Telegram
actions or open the microphone. A pending or uncertain voice send retains its capture.

The popup scene keeps a fixed layer-surface height and measures its cards on GTK's frame
clock. Footer reveal, neighbouring card positions and the input region therefore update
together. Live cards are ordered oldest to newest toward the resting edge. Dismissed
cards keep their slot while their cached face shrinks to 90%, drops up to 28 pixels and
fades behind the stack (65% of `stack.animationMs`); the remaining 35% collapses their
slot. Concealment removes faces immediately. Background clicks dismiss cards while
buttons, text editors and seek controls keep their own interactions.

Voice playback updates the existing play control, waveform, seek bar and timestamp; it
does not replace the card, reply editor or footer during the progress ticker.
`NOTJIK_GTK_LAYOUT_TRACE` optionally writes allocation traces for isolated synthetic
acceptance checks; it is unset in normal use.

## Compatibility boundaries

All built-in card types use the GTK card library. Registered custom QML components fall
back to standard cards in GTK. The `--quickshell` frontend remains available for those
components and the old drawer. GTK returns an explicit deferred-panel error for drawer
commands, while local history data/action APIs remain available. Global history merging
and the six-card demo-only stack exception are not implemented by the GTK controller.

## Validation

Run heavy validation through `dev-check` on managed desktops:

```sh
dev-check bash -c 'bun run --filter @tjikos/notjik build && bun run --filter @tjikos/notjik check && bash apps/notjik/scripts/test-gtk.sh && python3 -B apps/notjik/tests/native_contracts.py && python3 -B apps/notjik/tests/gtk_daemon.py && python3 -B apps/notjik/tests/gtk_cards.py'
```

`gtk_daemon.py` uses a private D-Bus session, synthetic notifications and a private
Wayland compositor whose host window is placed silently on workspace 7. It checks name
ownership, private IPC, replacements, expiry holds, native actions/signals and image
data, command execution, configuration reload, history and layer surfaces. The gallery
checks 19 variants in two themes against the QML reference. Unit tests cover controller
privacy, suppression, overflow and idle observation. These checks do not establish real
Telegram delivery, microphone capture, or end-to-end drag/keyboard interaction parity.

Install the validated build with `bash scripts/install.sh notjik`. On a managed desktop,
set `TJIKUP_MANAGED_CONFIG=1` to preserve managed service configuration. Replacing files
does not restart a running daemon or migrate its in-memory queue; restart the service
when the current notifications can be cleared.
