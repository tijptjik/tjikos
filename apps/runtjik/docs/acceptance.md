# Native acceptance

On 2026-11-10, 34 Rust backend tests and 7 GTK tests passed. Shared desktop contracts
passed 13 unit tests and one external-adapter test. A same-session ordinary-window
fixture on workspace 7 verified cold initial order, metadata-only updates, appended
results and stable columns without moving the pointer or taking focus.

The application now ships only the native frontend. `tests/gtk_session.py` exercises
stable cold/warm results with private configuration and synthetic data.
`tests/window_recording.py` substitutes the native countdown executable and checks
portal selection, cancellation and save ordering. Existing backend tests retain capture,
clipboard, browser, history, commands and task contracts.

Older renderer-only UI runners are retired. `gtk_ui.py` and `gtk_desktop.py` retain
native lab coverage for IME and scaled outputs, but their nested-compositor harness is
not approved for this desktop session. This pass does not claim physical CJK IME, device
reconnection or physical mixed-DPI sharpness acceptance. Run such tests only with an
explicitly isolated or approved interactive setup.

See [desktop acceptance](../../../docs/desktop-native-acceptance.md) for the matched
startup/idle measurements and their limitations.
