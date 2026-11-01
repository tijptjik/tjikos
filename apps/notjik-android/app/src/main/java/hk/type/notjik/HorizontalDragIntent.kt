package hk.type.notjik

import kotlin.math.abs
import kotlin.math.sign

/** Detect a sustained sideways segment, even after the same touch has scrolled. */
internal class HorizontalDragIntent(private val slop: Float) {
    private var sideways = 0f
    private var lastTime = 0L

    fun update(dx: Float, dy: Float, time: Long): Boolean {
        if (time - lastTime > 120 || abs(dx) <= abs(dy) * 1.5f ||
            (sideways != 0f && dx != 0f && sign(dx) != sign(sideways))) sideways = 0f
        lastTime = time
        if (abs(dx) > abs(dy) * 1.5f) sideways += dx
        return abs(sideways) >= slop
    }
}
