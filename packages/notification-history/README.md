# Notification history

Shared search, grouping, navigation and history content for Notjik and Bartjik.
The renderer and artwork links reference Notjik's canonical sources. Both builds
dereference these links into their own distributions, so installed QML and assets
never import another installed application. Changes to history browsing and card
rendering therefore reach both entry points together.

Notjik remains the notification service and action authority. Bartjik uses the
existing `notjikctl` connection with the bounded `history-data` / `history-action`
API; executable actions stay inside Notjik, and history is unavailable when locked.
