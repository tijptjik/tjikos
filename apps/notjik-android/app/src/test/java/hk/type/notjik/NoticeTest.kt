package hk.type.notjik

import org.junit.Assert.*
import org.junit.Test

class NoticeTest {
    @Test fun readAndDismissHaveDistinctVisibleEffects() {
        val unread = Notice("sample")
        val read = unread.copy(read = true)
        val dismissed = unread.copy(dismissed = true)
        assertTrue(DrawerFilter.INBOX.accepts(read))
        assertFalse(DrawerFilter.UNREAD.accepts(read))
        assertFalse(DrawerFilter.INBOX.accepts(dismissed))
        assertFalse(DrawerFilter.UNREAD.accepts(dismissed))
        assertTrue(DrawerFilter.HISTORY.accepts(dismissed))
        assertTrue(DrawerFilter.UNREAD.accepts(unread))
    }
    @Test fun searchRemainsScopedAcrossFilters() {
        val n = Notice("sample", title = "Backup complete", app = "Synctjik", device = "si", run = "run-123", dismissed = true)
        assertTrue(n.matches("si backup"))
        assertTrue(n.matches("RUN-123"))
        assertFalse(n.matches("si failure"))
        assertTrue(DrawerFilter.HISTORY.accepts(n))
    }
}
