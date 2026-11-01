package hk.type.notjik

internal data class CardPosition(val id: String, val index: Int, val top: Int)

/** Only a forward scroll crossing counts; initial layout and expansion do not. */
internal class ReadBoundary {
    private var previous = emptyList<CardPosition>()
    fun update(positions: List<CardPosition>, boundary: Int, scrollingForward: Boolean, firstVisibleIndex: Int): Set<String> {
        val current = positions.associateBy { it.id }
        val crossed = if (scrollingForward) previous.filter { old ->
            old.top > boundary && (current[old.id]?.let { it.top <= boundary }
                ?: (old.index < firstVisibleIndex))
        }.map { it.id }.toSet() else emptySet()
        previous = positions
        return crossed
    }
}
