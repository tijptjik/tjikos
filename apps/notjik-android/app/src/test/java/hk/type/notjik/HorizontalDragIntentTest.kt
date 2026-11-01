package hk.type.notjik

import org.junit.Assert.*
import org.junit.Test

class HorizontalDragIntentTest {
    @Test fun upwardScrollCanTurnIntoHorizontalDragWithoutLifting() {
        val intent = HorizontalDragIntent(24f)
        assertFalse(intent.update(1f, -30f, 10))
        assertFalse(intent.update(0f, -30f, 20))
        assertFalse(intent.update(-12f, -1f, 30))
        assertTrue(intent.update(-13f, 0f, 40))
    }
    @Test fun DiagonalScrollAndAlternatingJitterDoNotAccumulateIntoPickup() {
        val intent = HorizontalDragIntent(24f)
        for (i in 1..20) assertFalse(intent.update(-10f, -8f, i * 10L))
        for (i in 21..40) assertFalse(intent.update(if (i % 2 == 0) 12f else -12f, 0f, i * 10L))
        assertFalse(intent.update(12f, 0f, 600))
        assertTrue(intent.update(13f, 0f, 610))
    }
}
