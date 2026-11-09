# GTK migration architecture

Native services are callable from a GTK application, and the
[GTK card library](gtk-cards.md) implements the built-in card presentations. The
installed notification frontend remains Quickshell; the synthetic GTK gallery does not
introduce a second notification server. The Control Center/history panel is deferred to
a BarTjik panel.

## Implemented boundary

`notjik-core` now has a library as well as the existing CLI binary:

- `backend::run(root, transport)` owns configuration reloads, persisted pause
  preferences, lock observation, history retention and updates, sync dismissal
  enqueueing, and Telegram dismissal dispatch. It runs on a service thread.
- `backend::Transport` separates those services from their transport. Requests and
  updates retain the existing JSON worker protocol, including history snapshots and
  error messages. This is the worker protocol, not the `notjikctl` IPC API.
- `backend_channel::channel()` supplies a bounded in-process adapter. The UI submits
  requests without blocking and drains updates on its main loop. Queue saturation and
  disconnection are explicit errors. Service updates apply backpressure rather than
  silently dropping state. Drop the client before joining the service thread.
- `worker.rs` adapts stdin/stdout to the same runtime for the existing QML frontend. It
  retains bounded input and the 128 KiB request limit. Output failure now ends the
  worker instead of silently discarding updates.
- `cli.rs` owns process entry, argument parsing, launching and CLI dispatch. The binary
  retains its existing name, command behavior and process umask; the library runtime
  does not change the embedding process's umask.

A native frontend can establish its service connection like this, with no GTK objects
crossing the thread boundary:

```rust,ignore
let (client, mut transport) = notjik_core::backend_channel::channel();
let service = std::thread::spawn(move || {
    notjik_core::backend::run(&root, &mut transport)
});
// GTK main loop: client.submit(request), then drain client.try_recv().
// Handle service errors/disconnection by clearing readiness and lock knowledge.
// Shutdown: drop(client), then join service outside the GTK main loop.
```

The process supplies its configured resource root and environment before spawning
services. Keep state, account configuration, credentials and captures outside the
repository. A native frontend must begin with unknown lock state and preserve the
current fail-closed privacy behavior if the service stops. No renderer may infer that
absence of updates means the session is unlocked.

## Next ownership changes

| Responsibility                                                            | Current owner                                | GTK migration target                                                 |
| ------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------------------------------------------- |
| Configuration, SQLite history, lock observation, worker side effects      | Shared Rust backend                          | Same backend through the in-process adapter                          |
| Notification identity, replacement, queue, grouping, expiry and selection | `shell.qml` and `Logic.js`                   | Toolkit-independent Rust controller with explicit inputs and effects |
| Desktop notification D-Bus interface and action/close signals             | Quickshell notification service              | Rust service adapter feeding that controller                         |
| `notjikctl` request transport                                             | Quickshell IPC                               | Native IPC adapter using the same controller commands                |
| Monitor routing, layer-shell surfaces, focus and geometry                 | QML panels                                   | GTK frontend using `tjik-gtk` primitives where appropriate           |
| Card presentations                                                        | GTK card library and existing QML components | Connect GTK intents to the native controller and media executor      |
| Control Center/history panel                                              | Existing QML component                       | Deferred to a BarTjik panel                                          |

Move controller state before switching the launcher. The controller must accept explicit
elapsed time, hover/focus holds, lock state and display availability; it must not read
GTK widgets or own subprocesses. Snapshot data must not contain Quickshell
image-provider handles. D-Bus notification IDs, replacement semantics, close reasons and
action signals need contract tests before ownership moves. Effects such as history
writes, Telegram reads and sync dismissals must retain one owner so a staged migration
cannot execute an action twice.

GTK startup must eventually claim the existing D-Bus name without displacing an owner,
connect native IPC, and create surfaces on the GTK main loop. Only switch the installed
launcher when those paths and the necessary cards are ready. Do not start a partial GTK
server that accepts notifications it cannot display.

Custom QML component paths remain valid for the current renderer. They are not a GTK
extension API; choose an explicit compatibility or replacement policy when native custom
component support is added. Existing history data and the history control panel remain
unchanged in this stage.
