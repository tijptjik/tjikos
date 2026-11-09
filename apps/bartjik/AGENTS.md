# Bartjik migration testing

The user requested a live GTK preview limited to workspace **7** on 2026-11-09. Use the
current Hyprland session; do not launch nested compositors or switch workspace or focus.
The preview uses adapter workspace state to create surfaces only on outputs currently
displaying workspace 7. Keep ordinary test windows on workspace 7 too. Leave
Quickshell's normal auto-hide unchanged; GTK sits above it with an 8-pixel gutter.

Use `python3 apps/bartjik/scripts/gtk_compare.py start|status|stop`. The default
comparison uses the existing bar's cached items read-only and starts no duplicate
providers. `start --native --workspace 7` runs independent GTK providers and real
configured actions on the session bus. Both modes reserve no additional edge. Use
synthetic/private fixtures for automated mutations. Keep the Quickshell service as
default.

Build and run finite validation through `dev-check`; do not wrap the persistent
interactive comparison viewer in `dev-check`, since that holds the shared lock until the
viewer closes. `test:gtk` checks the running same-session comparison. Historical
isolated-compositor fixtures are not the default test path.
