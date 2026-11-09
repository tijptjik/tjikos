# Bartjik migration testing

The user explicitly requested same-session comparisons on 2026-11-09. For Bartjik bar
comparisons, use the current Hyprland session: leave the existing Quickshell bar in
place and render GTK immediately above it with an 8-pixel gutter. Do not launch a nested
Hyprland session or switch the user's workspace/focus. This bar-specific request
supersedes the root workspace-7 preview rule for these layer surfaces. Keep ordinary
unrelated test windows on workspace 7.

Use `python3 apps/bartjik/scripts/gtk_compare.py start|status|stop`. The default
comparison uses the existing bar's cached items read-only and starts no duplicate
providers. `start --native` runs independent GTK providers and real configured actions
on the session bus. Both modes reserve no additional edge. Use synthetic/private
fixtures for automated mutations. Keep the Quickshell service as default.

Build and run finite validation through `dev-check`; do not wrap the persistent
interactive comparison viewer in `dev-check`, since that holds the shared lock until the
viewer closes. `test:gtk` checks the running same-session comparison. Historical
isolated-compositor fixtures are not the default test path.
