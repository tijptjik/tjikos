# Bartjik migration testing

GTK is the main service default after the authorized 2026-11-10 cutover. Use
`bartjik --quickshell` for an explicitly selected legacy comparison or rollback. Do not
start a second native provider authority alongside the main bar.

Visible tests and comparison previews remain confined to workspace **7** in the current
Hyprland session. Do not launch nested compositors, switch workspace or take focus. Set
placement before launch, including child windows. The live-display bar benchmark was an
explicit one-time user exception, not a change to the testing default.

`python3 apps/bartjik/scripts/gtk_compare.py start|status|stop` provides the historical
preview workflow. Its read-only comparison requires a running Quickshell source.
`start --native --workspace 7` starts independent providers and real configured actions;
do not use it alongside the GTK main service. Use synthetic/private fixtures for
automated mutations.

Build and run finite validation through `dev-check`; do not wrap the persistent
interactive comparison viewer in `dev-check`, since that holds the shared lock until the
viewer closes. `test:gtk` checks the running same-session comparison. Historical
isolated-compositor fixtures are not the default test path.
