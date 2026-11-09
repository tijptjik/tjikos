# Native runtime

Notjik uses Rust and GTK for notification delivery, cards, demos and recordings. The
CLI, configuration, history, avatar cache, lock observer, Telegram bridge, media
renderer and voice recorder are native Rust services. TDLib and rlottie use native C
APIs; FFmpeg handles media conversion and playback. Python is used only for development
test drivers and is not installed with the application.

The GTK host communicates with the backend through a bounded in-process channel. The
stdio worker command remains available for isolated persistence tests and diagnostic
tools. Account data, credentials, configuration and history stay outside the repository.
Updates preserve existing SQLite state and Telegram authentication.

See [GTK architecture](gtk-architecture.md) for ownership, lifecycle behavior and
current validation commands. The synthetic tests do not establish real microphone
capture or Telegram delivery; those require live acceptance.
