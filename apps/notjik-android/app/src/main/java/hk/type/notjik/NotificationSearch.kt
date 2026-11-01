package hk.type.notjik

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable internal fun NotificationSearch(
    query: String,
    onQuery: (String) -> Unit,
    selected: DrawerFilter,
    onSelect: (DrawerFilter) -> Unit,
    matching: List<Notice>
) {
    var filtersOpen by rememberSaveable { mutableStateOf(false) }
    BackHandler(filtersOpen) { filtersOpen = false }
    Column(Modifier.fillMaxWidth()) {
        OutlinedTextField(query, onQuery, modifier = Modifier.fillMaxWidth(), singleLine = true,
            shape = RoundedCornerShape(14.dp), placeholder = { Text("Search notifications", fontSize = 14.sp) },
            leadingIcon = { Icon(painterResource(R.drawable.ic_search), null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
            trailingIcon = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (query.isNotEmpty()) IconButton(onClick = { onQuery("") }) {
                        Icon(painterResource(R.drawable.ic_close), "Clear search")
                    }
                    IconToggleButton(checked = filtersOpen, onCheckedChange = { filtersOpen = it },
                        modifier = Modifier.semantics { stateDescription = "${selected.label}, ${if (filtersOpen) "expanded" else "collapsed"}" }) {
                        Icon(painterResource(R.drawable.ic_filter), "Filter notifications",
                            tint = if (filtersOpen || selected != DrawerFilter.INBOX) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            })
        AnimatedVisibility(filtersOpen,
            enter = expandVertically(tween(220), expandFrom = Alignment.Top) + slideInVertically(tween(220)) { -it / 2 } + fadeIn(tween(180)),
            exit = shrinkVertically(tween(180), shrinkTowards = Alignment.Top) + slideOutVertically(tween(180)) { -it / 2 } + fadeOut(tween(120))) {
            Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                DrawerFilter.entries.forEach { choice ->
                    FilterChip(selected = choice == selected, onClick = {
                        onSelect(choice)
                        filtersOpen = false
                    }, label = { Text("${choice.label} ${matching.count(choice::accepts)}", fontSize = 12.sp) })
                }
            }
        }
    }
}
