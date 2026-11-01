package hk.type.notjik

import org.junit.Assert.*
import org.junit.Test

class DismissalOverlayTest {
    @Test fun removesImmediatelyAndStaleReloadCannotRestoreIt() {
        val overlay = DismissalOverlay()
        val stale = listOf(Notice("a"), Notice("b"))
        overlay.dismiss("a")
        assertEquals(listOf("b"), overlay.apply(stale).filter(DrawerFilter.INBOX::accepts).map { it.id })
        assertTrue(overlay.apply(stale).first().dismissed)
        assertEquals(2, overlay.apply(stale).filter(DrawerFilter.HISTORY::accepts).size)
    }
    @Test fun failedSaveRestoresOnlyItsCardWithoutRevertingOtherChanges() {
        val overlay = DismissalOverlay()
        overlay.dismiss("a")
        overlay.dismiss("b")
        overlay.restore("a")
        val latest = overlay.apply(listOf(Notice("a", read = true), Notice("b")))
        assertFalse(latest.first().dismissed)
        assertTrue(latest.first().read)
        assertTrue(latest.last().dismissed)
    }
}
