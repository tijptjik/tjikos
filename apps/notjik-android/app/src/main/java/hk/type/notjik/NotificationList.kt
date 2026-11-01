package hk.type.notjik

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun NotificationList(
    refreshing: Boolean,
    onRefresh: () -> Unit,
    modifier: Modifier = Modifier,
    unreadIds: Set<String> = emptySet(),
    lastCardId: String? = null,
    feedKey: String = "",
    onPassed: (Set<String>) -> Unit = {},
    content: LazyListScope.() -> Unit
) {
    val list = rememberLazyListState()
    val currentUnread by rememberUpdatedState(unreadIds)
    val currentLast by rememberUpdatedState(lastCardId)
    val passed by rememberUpdatedState(onPassed)
    var measuredLast by remember { mutableStateOf<String?>(null) }
    var lastHeight by remember { mutableIntStateOf(0) }
    LaunchedEffect(list, feedKey) {
        val tracker = ReadBoundary()
        snapshotFlow {
            val info = list.layoutInfo
            Triple(info, list.isScrollInProgress && list.lastScrolledForward, info.visibleItemsInfo.map {
                CardPosition(it.key.toString(), it.index, it.offset)
            })
        }.collect { (info, forward, positions) ->
            info.visibleItemsInfo.firstOrNull { it.key == currentLast }?.let {
                measuredLast = currentLast
                lastHeight = it.size
            }
            val crossed = tracker.update(positions, info.viewportStartOffset, forward,
                info.visibleItemsInfo.firstOrNull()?.index ?: 0).intersect(currentUnread)
            if (crossed.isNotEmpty()) passed(crossed)
        }
    }
    BoxWithConstraints(modifier) {
        val lastHeightDp = with(LocalDensity.current) { (if (measuredLast == lastCardId) lastHeight else 0).toDp() }
        // Enough space to move the final card's top just under the fixed toolbar.
        val bottom = if (lastCardId == null) 28.dp else maxOf(28.dp, maxHeight - lastHeightDp + 2.dp)
        PullToRefreshBox(isRefreshing = refreshing, onRefresh = onRefresh) {
            LazyColumn(Modifier.fillMaxSize().semantics {
                customActions = listOf(CustomAccessibilityAction("Refresh notifications") {
                    onRefresh()
                    true
                })
            }, state = list, contentPadding = PaddingValues(start = 20.dp, end = 20.dp, top = 18.dp, bottom = bottom),
                verticalArrangement = Arrangement.spacedBy(12.dp), content = content)
        }
    }
}
