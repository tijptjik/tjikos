# Desktop adapter contract

`tjik-desktop` provides toolkit-independent desktop state and actions. GTK objects,
application models, and async runtimes do not cross this boundary. The built-in Hyprland
adapter and synthetic `fake::Fake` implement the same traits.

Consumers can accept `Arc<dyn Desktop>` to allow caller-provided adapters. `selected()`
is an application convenience that resolves the environment once per process; it is not
an adapter registry. Unknown or ambiguous compositor choices fail closed.

Implement `DesktopSession`, `WindowActions`, `WorkspaceActions`, and `Desktop` for an
external adapter. Advertise supported operations in `Diagnostics.capabilities` and
return `DesktopError::Unsupported` for unavailable operations. Blocking operations
belong on worker threads, including snapshot acquisition and action dispatch.

Own an `EventSource` in the adapter and return `events.subscribe()` from `subscribe`.
Publish complete `DesktopState` values when the backend changes. The source assigns
monotonic sequence numbers and snapshot revisions; use the returned snapshot revision in
any adapter cache. Serialize cache updates and publication so synchronous snapshots and
subscriber events cannot disagree about ordering. Set `resync` on reconnection or
generation changes. IDs must remain opaque to consumers, and adapters must reject stale
action targets after a generation change.

Each subscription retains at most one pending event. A slow or newly attached consumer
receives the latest full state with `resync` set. Consumers replace their state rather
than replaying deltas. Dropping the source closes subscriptions after any pending event
has been delivered. The source owns no worker thread and performs no backend I/O.

The external-adapter integration test compiles against only the public API. Run its
contract checks with:

```sh
dev-check cargo test --manifest-path packages/tjik-desktop/Cargo.toml --locked
```

The optional `capture` feature enables the Hyprland capture implementation. Additional
adapters need not support capture or depend on GTK.

`Snapshot.focused_window` names the current keyboard-focused window when the adapter can
report it; `None` does not imply that the most recent focus-history entry is active.
Hyprland resolves the active address to a window in the same snapshot and returns None
when focus disappears during acquisition. Workspace IDs are opaque session identities;
consumers must not parse them as numbers or substitute workspace names for identity.

## License

Source by tijptjik in this package is licensed under MPL-2.0; see `LICENSE` and
`NOTICE`. Standard MPL secondary-license compatibility is retained, allowing use in
GPLv3 applications. Third-party components retain their own licenses and notices.

For continuously refreshed thumbnails, call `Arc<dyn Desktop>::capture_stream(&id)` on a
worker and retain the returned `CaptureStream`. Each `capture()` validates the window
identity and returns RGBA8 pixels; the Hyprland implementation reuses its connection and
shared memory buffer, reallocating on size or format changes. Discard the stream after
an error and drop it when hidden. Unsupported adapters return
`DesktopError::Unsupported`. The one-shot `capture(&id)` API remains available.
