# GTK architecture

GTK is the default Notjik notification frontend. `notjik --quickshell` retains the
compatibility frontend; `notjik --gtk-cards --demo` runs the isolated synthetic gallery.
The Control Center/history UI is implemented as a BarTjik panel.

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
hold expiry. Ordinary popups do not take keyboard focus on arrival; click focus delivers
held modifiers, and explicit selection enables keyboard navigation, activation,
dismissal and Escape to release focus. Dragging plain card content or artwork
immediately transfers the original widget from stack flow to an input-transparent drag
surface. No duplicate card or reserved gap remains, including when the stack becomes
empty. Native Wayland drag-and-drop and monitor-sized input regions maintain the gesture
across outputs; the pointer selects the destination and release clamps the card to that
output. Detached cards can be dragged again. Buttons, editors and seeking retain their
own pointer gestures. Reply drafts and action-page selection stay in the same card
widget. Monitor removal cancels a grab and rehomes cards on a surviving output; if no
output remains, cards are retained for reconnection. Pause, lock and backend
disconnection hide surfaces without discarding composers. Composing and dragging hold
notification expiry. Ctrl-click requests Telegram safe-read for that card; on an
ordinary card it dismisses the entire queue.

The executor keeps playback and recording ownership separate from widgets. Concealment
stops audio and stops an active recording. Synthetic notifications never send Telegram
actions or open the microphone. A pending or uncertain voice send retains its capture.

The popup scene keeps a fixed layer-surface height and measures its cards on GTK's frame
clock. Footer reveal, neighbouring card positions and the input region therefore update
together. Live cards are ordered oldest to newest toward the resting edge. Dismissed
cards keep their slot while their cached face shrinks to 82%, drops up to 64 pixels and
fades behind the stack (65% of `stack.animationMs`); the remaining 35% collapses their
slot. Concealment removes faces immediately. Background clicks dismiss cards while
buttons, text editors and seek controls keep their own interactions.

Voice playback updates the existing play control, waveform, seek bar and timestamp; it
does not replace the card, reply editor or footer during the progress ticker.
`NOTJIK_GTK_LAYOUT_TRACE` optionally writes allocation traces for isolated synthetic
acceptance checks; it is unset in normal use.

## Compatibility boundaries

All built-in card types use the GTK card library. The legacy `components` configuration
accepts user-authored QML files only for `--quickshell`; those optional extensions use
standard cards in GTK. This is not a missing built-in card type.

GTK, named demos, directed showcases and recording use `assets/icons` and the standalone
`assets/artwork.json` manifest. Launchers discover installations through that manifest.
The optional Quickshell frontend and visual reference tests remain in `qml/`; its
`icons` link points to the shared assets. GTK and its demo tooling run without that
directory. The six-card synthetic mix temporarily takes precedence over ordinary queued
notices and restores them when cleared, without changing saved limits. Its stack can
scroll on short outputs so every demo card remains reachable.

Comparisons run GTK and Dunst on separate private buses. Directed showcases use a GTK
stage and real card widgets; control requests are accepted only by the explicitly
synthetic host. Replies, recording, playback progress and actions are simulated there;
commands and microphone access are disabled. The production daemon rejects showcase
control requests. History UI is provided by BarTjik, with local history data/action APIs
remaining available from Notjik.

## Validation

Run heavy validation through `dev-check` on managed desktops:

```sh
dev-check bash -c 'bun run --filter @tjikos/notjik build && bun run --filter @tjikos/notjik check && bash apps/notjik/scripts/test-gtk.sh && python3 -B apps/notjik/tests/native_contracts.py && python3 -B apps/notjik/tests/gtk_daemon.py && python3 -B apps/notjik/tests/gtk_cards.py && python3 -B apps/notjik/tests/gtk_demo.py'
```

`gtk_daemon.py` uses a private D-Bus session, synthetic notifications and a private
Wayland compositor whose host window is placed silently on workspace 7. It checks name
ownership, private IPC, replacements, expiry holds, native actions/signals and image
data, command execution, configuration reload, history and layer surfaces. The gallery
checks 19 variants in two themes against the QML reference. Unit tests cover controller
privacy, suppression, overflow and idle observation. Mechanical lifecycle checks use
real GTK widgets and a private fake Telegram socket to test replacement during dragging,
pause/resume, lock/unlock, backend loss/recovery, uncertain send completion, output
removal/reconnection, draft identity and expiry holds. Pointer acceptance checks the
last-card buffer with a screenshot, background dismissal, repeated dragging, mixed-scale
cross-output dragging, monitor removal during a grab, and Ctrl-click semantics. These
checks do not establish real Telegram delivery or microphone capture.

Install the validated build with `bash scripts/install.sh notjik`. On a managed desktop,
set `TJIKUP_MANAGED_CONFIG=1` to preserve managed service configuration. Replacing files
does not restart a running daemon or migrate its in-memory queue; restart the service
when the current notifications can be cleared.
