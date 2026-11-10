# Bartjik native testing

GTK is the sole frontend and main service. Do not start a second native provider
authority alongside the main bar. Use private buses and synthetic providers for tests.

Place every visible test on workspace **7** silently before launch, including child
windows. Use the current Hyprland session; no nested compositors, workspace changes or
focus theft. Live-display benchmark permission does not change this default.

Run finite builds and validation through `dev-check`. `test:gtk` uses an ordinary
synthetic card window and checks its placement. `test:native` covers isolated provider
protocols without contacting real device services. Historical isolated-compositor tests
are not approved for this desktop session. Build, install using the supported installer,
and verify installed bytes; preserve managed configuration and active workers.
