package hk.type.notjik

import org.junit.Assert.*
import org.junit.Test

class DrawerPresenceTest {
    @Test fun refreshDoesNotAdvanceActivity() {
        val presence = DrawerPresence()
        presence.opened(1000)
        assertEquals(10L, presence.snapshot(11000, true).idleSeconds)
        assertEquals(20L, presence.snapshot(21000, true).idleSeconds)
        assertFalse(presence.snapshot(121000, true).active)
        presence.interacted(122000)
        assertTrue(presence.snapshot(122000, true).active)
        assertEquals(0L, presence.snapshot(122000, true).idleSeconds)
    }

    @Test fun backgroundPhoneUseDoesNotCompete() {
        val presence = DrawerPresence()
        presence.opened(1000)
        presence.closed()
        presence.interacted(9000)
        val background = presence.snapshot(11000, true)
        assertFalse(background.ready)
        assertFalse(background.active)
        assertEquals(10L, background.idleSeconds)
        presence.opened(12000)
        assertTrue(presence.snapshot(12000, true).active)
    }

    @Test fun LockedOrSleepingDeviceIsNotForegroundEligible() {
        val presence = DrawerPresence()
        assertFalse(presence.snapshot(0, true).active)
        presence.opened(1000)
        assertFalse(presence.snapshot(1000, false).ready)
        assertFalse(presence.snapshot(1000, false).active)
    }
}
