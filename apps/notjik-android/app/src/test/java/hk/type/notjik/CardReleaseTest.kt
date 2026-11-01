package hk.type.notjik

import org.junit.Assert.assertEquals
import org.junit.Test

class CardReleaseTest {
    @Test fun shortSlowReleaseReturnsButFastFlickClearsEitherSide() {
        assertEquals(0, cardReleaseSide(-150f, -100f, 400f))
        assertEquals(-1, cardReleaseSide(-150f, -1800f, 400f))
        assertEquals(1, cardReleaseSide(150f, 1800f, 400f))
    }
    @Test fun reversingVelocityCanRescueACardBeyondTheEdge() {
        assertEquals(-1, cardReleaseSide(-450f, 0f, 400f))
        assertEquals(0, cardReleaseSide(-450f, 1000f, 400f))
        assertEquals(0, cardReleaseSide(450f, -1000f, 400f))
    }
}
