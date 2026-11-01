package hk.type.notjik

import org.junit.Assert.*
import org.junit.Test

class ReadBoundaryTest {
    @Test fun forwardCrossingCountsExactlyOnce() {
        val tracker = ReadBoundary()
        assertTrue(tracker.update(listOf(CardPosition("a", 1, 20)), -18, false, 0).isEmpty())
        assertTrue(tracker.update(listOf(CardPosition("a", 1, -10)), -18, true, 1).isEmpty())
        assertEquals(setOf("a"), tracker.update(listOf(CardPosition("a", 1, -18)), -18, true, 1))
        assertTrue(tracker.update(listOf(CardPosition("a", 1, -30)), -18, true, 1).isEmpty())
    }
    @Test fun initialLayoutExpansionAndBackwardScrollDoNotRead() {
        val tracker = ReadBoundary()
        assertTrue(tracker.update(listOf(CardPosition("a", 1, -30)), 0, true, 1).isEmpty())
        assertTrue(tracker.update(listOf(CardPosition("a", 1, 20)), 0, false, 0).isEmpty())
        assertTrue(tracker.update(listOf(CardPosition("a", 1, -10)), 0, false, 1).isEmpty())
        assertTrue(tracker.update(listOf(CardPosition("a", 1, 20)), 0, false, 0).isEmpty())
    }
    @Test fun fastScrollCountsACardThatLeftTheViewportButNotOneRemovedBelow() {
        val tracker = ReadBoundary()
        tracker.update(listOf(CardPosition("a", 1, 20), CardPosition("b", 2, 180)), 0, false, 0)
        assertEquals(setOf("a"), tracker.update(listOf(CardPosition("b", 2, 10)), 0, true, 2))
        tracker.update(listOf(CardPosition("b", 2, 10), CardPosition("c", 3, 170)), 0, false, 2)
        assertTrue(tracker.update(listOf(CardPosition("b", 2, 10)), 0, true, 2).isEmpty())
    }
}
