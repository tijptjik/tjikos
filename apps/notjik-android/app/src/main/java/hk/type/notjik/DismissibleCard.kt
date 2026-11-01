package hk.type.notjik

import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlin.math.abs

/** Acquire horizontal intent, then follow freely until release chooses a destination. */
@Composable internal fun DismissibleCard(
    enabled: Boolean,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable (Float) -> Unit
) {
    var offset by remember { mutableStateOf(Offset.Zero) }
    var settling by remember { mutableStateOf(false) }
    val dismiss by rememberUpdatedState(onDismiss)
    val scope = rememberCoroutineScope()
    LaunchedEffect(enabled) { if (!enabled) offset = Offset.Zero }
    var touchOrigin by remember { mutableStateOf(Offset.Zero) }
    Box(modifier.onGloballyPositioned { touchOrigin = it.positionInRoot() }.pointerInput(enabled) {
        val horizontalSlop = maxOf(24.dp.toPx(), viewConfiguration.touchSlop * 2)
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            if (!enabled || settling) return@awaitEachGesture
            val velocity = VelocityTracker().apply { addPosition(down.uptimeMillis, down.position + touchOrigin) }
            val intent = HorizontalDragIntent(horizontalSlop)
            var previous = down.position + touchOrigin
            var origin = Offset.Zero
            var dragging = false
            var released = false
            var lastMovement = down.uptimeMillis
            var releaseVelocity = Offset.Zero
            try {
                while (true) {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (event.changes.count { it.pressed } > 1) break
                    val position = change.position + touchOrigin
                    if (change.pressed) velocity.addPosition(change.uptimeMillis, position)
                    if (!change.pressed) {
                        released = true
                        if (dragging) {
                            change.consume()
                            if (change.uptimeMillis - lastMovement < 100) {
                                val v = velocity.calculateVelocity()
                                releaseVelocity = Offset(v.x, v.y)
                            }
                        }
                        break
                    }
                    val delta = position - previous
                    previous = position
                    if (delta != Offset.Zero) lastMovement = change.uptimeMillis
                    if (!dragging && abs(delta.y) > abs(delta.x)) {
                        velocity.resetTracking()
                        velocity.addPosition(change.uptimeMillis, position)
                    }
                    if (!dragging && intent.update(delta.x, delta.y, change.uptimeMillis)) {
                        // Observe throughout scrolling, and claim a clear sideways turn before
                        // the list's Main-pass drag handler sees it. Do not replay prior scrolling.
                        dragging = true
                        origin = position
                    }
                    if (dragging) {
                        change.consume()
                        offset = position - origin
                    }
                }
            } finally {
                if (dragging) {
                    val side = if (released) cardReleaseSide(offset.x, releaseVelocity.x, size.width.toFloat()) else 0
                    val targetX = side * size.width * 1.3f
                    val targetY = if (side == 0) 0f else offset.y + releaseVelocity.y * 0.15f
                    settling = true
                    scope.launch {
                        try {
                            coroutineScope {
                                launch {
                                    animate(offset.x, targetX, initialVelocity = releaseVelocity.x,
                                        animationSpec = spring(dampingRatio = 1f, stiffness = 350f)) { value, _ ->
                                        offset = offset.copy(x = value)
                                    }
                                }
                                launch {
                                    animate(offset.y, targetY, initialVelocity = releaseVelocity.y,
                                        animationSpec = spring(dampingRatio = 1f, stiffness = 350f)) { value, _ ->
                                        offset = offset.copy(y = value)
                                    }
                                }
                            }
                            if (side != 0) dismiss()
                        } finally {
                            offset = Offset.Zero
                            settling = false
                        }
                    }
                }
            }
        }
    }) {
        // Keep pointer coordinates stationary while only the visual card moves.
        Box(Modifier.graphicsLayer { translationX = offset.x; translationY = offset.y }) { content(offset.x) }
    }
}
