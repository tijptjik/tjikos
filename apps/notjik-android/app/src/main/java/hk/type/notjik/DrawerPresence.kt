package hk.type.notjik

/** Only opening or interacting with the drawer advances its activity timestamp. */
class DrawerPresence {
    private var foreground = false
    private var lastInteraction = -1L

    @Synchronized fun opened(now: Long) { foreground = true; lastInteraction = now }
    @Synchronized fun closed() { foreground = false }
    @Synchronized fun interacted(now: Long) { if (foreground) lastInteraction = now }
    @Synchronized fun snapshot(now: Long, available: Boolean): Snapshot {
        val age = if (lastInteraction < 0) 86400L else ((now - lastInteraction).coerceAtLeast(0) / 1000).coerceAtMost(86400)
        val ready = foreground && available
        return Snapshot(ready, ready && age < 120, age)
    }

    data class Snapshot(val ready: Boolean, val active: Boolean, val idleSeconds: Long)
    companion object { val current = DrawerPresence() }
}
