package hk.type.notjik

import androidx.compose.animation.core.animate
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import kotlin.math.abs

/** Lock direction before moving; a fast flick alone must never dismiss a card. */
@Composable internal fun DismissibleCard(
    enabled: Boolean,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable (Float) -> Unit
) {
    var offset by remember { mutableFloatStateOf(0f) }
    var settling by remember { mutableStateOf(false) }
    val dismiss by rememberUpdatedState(onDismiss)
    val scope = rememberCoroutineScope()
    LaunchedEffect(enabled) { if (!enabled) offset = 0f }
    Box(modifier.pointerInput(enabled) {
        val horizontalSlop = maxOf(24.dp.toPx(), viewConfiguration.touchSlop * 2)
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false)
            if (!enabled || settling) return@awaitEachGesture
            var x = 0f
            var y = 0f
            var dragging = false
            var released = false
            try {
                while (true) {
                    val event = awaitPointerEvent()
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (event.changes.count { it.pressed } > 1 || change.isConsumed) break
                    if (!change.pressed) {
                        released = true
                        if (dragging) change.consume()
                        break
                    }
                    val delta = change.positionChange()
                    x += delta.x
                    y += delta.y
                    if (!dragging) {
                        // Yield to the list as soon as vertical intent is apparent.
                        if (abs(y) > viewConfiguration.touchSlop || x > horizontalSlop) break
                        if (x < -horizontalSlop && abs(x) > abs(y) * 2) dragging = true
                    }
                    if (dragging) {
                        change.consume()
                        offset = (x + horizontalSlop).coerceIn(-size.width.toFloat(), 0f)
                    }
                }
            } finally {
                if (dragging) {
                    val complete = released && offset < -size.width * 0.55f
                    val target = if (complete) -size.width.toFloat() else 0f
                    settling = true
                    scope.launch {
                        try {
                            animate(offset, target, animationSpec = tween(180)) { value, _ -> offset = value }
                            if (complete) dismiss()
                        } finally {
                            offset = 0f
                            settling = false
                        }
                    }
                }
            }
        }
    }.graphicsLayer { translationX = offset }) { content(offset) }
}
