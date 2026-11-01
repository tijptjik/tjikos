package hk.type.notjik

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** Keep local intent over stale reloads, even after the outbox write completes. */
internal class DismissalOverlay {
    private var ids by mutableStateOf<Set<String>>(emptySet())
    fun dismiss(id: String) { ids = ids + id }
    fun restore(id: String) { ids = ids - id }
    fun apply(cards: List<Notice>): List<Notice> = cards.map {
        if (it.id in ids) it.copy(dismissed = true) else it
    }
}
