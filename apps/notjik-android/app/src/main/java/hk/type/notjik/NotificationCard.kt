package hk.type.notjik

import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.Role
import kotlin.math.abs
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

private val Rose = Color(0xFFF1A7C4)
private val Teal = Color(0xFF9CCFD8)
private val Muted = Color(0xFFB3AABB)

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun NotificationCard(
    n: Notice,
    saving: Boolean,
    onRead: () -> Unit,
    onDismiss: () -> Unit,
    onAction: (NoticeAction) -> Unit,
    modifier: Modifier = Modifier
) {
    var expanded by rememberSaveable(n.id) { mutableStateOf(false) }
    var resizing by remember(n.id) { mutableStateOf(false) }
    val interaction = remember(n.id) { MutableInteractionSource() }
    val canDismiss by rememberUpdatedState(!n.dismissed && !saving)
    val dismiss by rememberUpdatedState(onDismiss)
    val unread = !n.read && !n.dismissed
    val border = if (unread) Color(0xFF65465E) else Color(0xFF352C3E)
    val shape = RoundedCornerShape(18.dp)
    DismissibleCard(enabled = canDismiss, onDismiss = dismiss, modifier = modifier) { offset ->
        Card(modifier = Modifier.fillMaxWidth().graphicsLayer {
            val lift = (abs(offset) / size.width.coerceAtLeast(1f)).coerceIn(0f, 1f)
            rotationZ = 4f * (offset / size.width.coerceAtLeast(1f)).coerceIn(-1f, 1f)
            translationY = -8.dp.toPx() * lift
            scaleX = 1f + 0.015f * lift
            scaleY = scaleX
            shadowElevation = 18.dp.toPx() * lift
            this.shape = shape
            clip = false
        }.clickable(interactionSource = interaction, indication = null, role = Role.Button,
            onClickLabel = if (expanded) "Hide details and actions" else "Show details and actions") {
            resizing = true
            expanded = !expanded
        }.semantics {
            stateDescription = if (expanded) "Details and actions expanded" else "Details and actions collapsed"
            customActions = buildList {
                if (!n.dismissed && !saving) {
                    add(CustomAccessibilityAction("Dismiss") { onDismiss(); true })
                    if (!n.read) add(CustomAccessibilityAction("Mark read") { onRead(); true })
                }
            }
        }, shape = shape,
            colors = CardDefaults.cardColors(containerColor = if (unread) Color(0xFF2A2331) else Color(0xFF221D28)),
            border = BorderStroke(1.dp, border)) {
            Box(Modifier.animateContentSize(tween(220, easing = FastOutSlowInEasing), finishedListener = { _, _ -> resizing = false })) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (unread) Box(Modifier.size(6.dp).clip(RoundedCornerShape(3.dp)).background(Rose))
                        Text(n.app, color = if (unread) Rose else Muted, fontSize = 12.sp, fontWeight = FontWeight.Medium,
                            maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                        if (n.device.isNotBlank()) Text(n.device.replaceFirstChar { it.uppercase() }, fontSize = 11.sp, color = Teal,
                            modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(Teal.copy(alpha = 0.08f)).padding(horizontal = 6.dp, vertical = 3.dp))
                        Text(timeLabel(n.updated), color = Muted, fontSize = 11.sp)
                    }
                    Text(n.title, fontSize = 16.sp, lineHeight = 22.sp, fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Medium)
                    NoticeContent(n, expanded)
                    val copyAction = n.actions.firstOrNull { it.copy.isNotBlank() }
                    if (copyAction != null) Text(copyAction.copy, fontSize = 28.sp, color = Teal)
                    if (expanded) {
                        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            if (n.run.isNotBlank()) Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                Text("RUN", color = Muted, fontSize = 10.sp)
                                SelectionContainer { Text(n.run, fontSize = 11.sp, lineHeight = 16.sp, color = Muted) }
                            }
                            HorizontalDivider(color = Color(0xFF403447))
                            if (copyAction != null) Button(onClick = { onAction(copyAction) }, enabled = !saving, modifier = Modifier.fillMaxWidth()) { Text("Copy code") }
                            val remoteActions = n.actions.filter { it.copy.isBlank() }
                            if (remoteActions.isNotEmpty()) {
                                Text("Actions · ${n.device.ifBlank { "source device" }}", color = Muted, fontSize = 11.sp)
                                remoteActions.forEach { action ->
                                    OutlinedButton(onClick = { onAction(action) }, enabled = !saving,
                                        modifier = Modifier.fillMaxWidth(), shape = RoundedCornerShape(10.dp)) { Text(action.label) }
                                }
                            }
                            if (n.actionState.isNotBlank()) Text("Last action · ${n.actionState}", color = Teal, fontSize = 12.sp)
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                if (n.dismissed) Text("Dismissed · saved in History", color = Muted, fontSize = 12.sp)
                                else {
                                    TextButton(onClick = onRead, enabled = !n.read && !saving, contentPadding = PaddingValues(horizontal = 4.dp)) {
                                        Icon(painterResource(R.drawable.ic_check), null, modifier = Modifier.size(16.dp))
                                        Spacer(Modifier.width(5.dp))
                                        Text(if (n.read) "Read" else "Mark read", fontSize = 12.sp)
                                    }
                                    Spacer(Modifier.weight(1f))
                                    TextButton(onClick = onDismiss, enabled = !saving, contentPadding = PaddingValues(horizontal = 8.dp)) { Text("Dismiss", fontSize = 12.sp) }
                                }
                            }
                        }
                    }
                }
                // Match the desktop card's centered, 30%-width interaction rail.
                if (!expanded && !resizing) {
                    Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth(0.3f).height(3.dp).clip(RoundedCornerShape(2.dp)).background(border))
                }
            }
        }
    }
}

private fun timeLabel(seconds: Long): String = if (seconds <= 0) "" else Instant.ofEpochSecond(seconds).atZone(ZoneId.systemDefault()).format(DateTimeFormatter.ofPattern("HH:mm"))
