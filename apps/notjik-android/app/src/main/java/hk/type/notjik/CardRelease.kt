package hk.type.notjik

/** Project 300ms of momentum; only a landing outside the card's slot clears it. */
internal fun cardReleaseSide(position: Float, velocity: Float, width: Float): Int {
    if (width <= 0f) return 0
    val landing = position + velocity * 0.3f
    return when {
        landing <= -width -> -1
        landing >= width -> 1
        else -> 0
    }
}
