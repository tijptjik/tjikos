# BarTjik legacy history presentation

This package owns the rendering files used by BarTjik's currently supported Quickshell
history panel. It does not contain a notification server, launcher or Notjik frontend.
Its artwork link points to Notjik's shared `assets/icons` directory. BarTjik's build
copies these resources into its own installation.

Notjik is GTK-only. The native BarTjik history panel uses the Rust Notjik card library;
it does not use this package. Retire this package when BarTjik's Quickshell frontend is
removed. Until then, keeping it self-contained avoids breaking the running bar when
Notjik is updated.

Notjik remains the notification and action authority. Both history views use
`notjikctl history-data` and `history-action`; executable actions stay inside Notjik,
and history is unavailable when locked.
