package hk.type.notjik

import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test

class NotificationCardTest {
    @Test fun tapActionsAndMomentumBasedRelease() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        var notice by mutableStateOf(Notice("synthetic-card", title = "Authentication code · preview",
            body = "Example sender", app = "Notjik preview", device = "demo",
            actions = listOf(NoticeAction("", "Copy code", "", "004281"))))
        var reads = 0
        var dismissals = 0
        var copies = 0
        ActivityScenario.launch<MainActivity>(Intent(context, MainActivity::class.java)).use { scene ->
            scene.onActivity { activity ->
                activity.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                activity.setShowWhenLocked(true)
                activity.setTurnScreenOn(true)
                activity.setContent {
                MaterialTheme(colorScheme = darkColorScheme(primary = Color(0xFFF1A7C4))) {
                    Surface(Modifier.fillMaxSize(), color = Color(0xFF17131D), contentColor = Color(0xFFF2EDF4)) {
                        NotificationList(false, {}) {
                            item {
                                NotificationCard(notice, saving = false,
                                    onRead = { reads++; notice = notice.copy(read = true) },
                                    onDismiss = { dismissals++; notice = notice.copy(dismissed = true) },
                                    onAction = { assertEquals("004281", it.copy); copies++ })
                            }
                        }
                    }
                }
            } }
            fun root(view: View): ViewRootForTest? {
                if (view is ViewRootForTest) return view
                if (view is ViewGroup) for (index in 0 until view.childCount) root(view.getChildAt(index))?.let { return it }
                return null
            }
            fun settle() { instrumentation.waitForIdleSync(); Thread.sleep(750) }
            fun nodes(): List<SemanticsNode> {
                var result = emptyList<SemanticsNode>()
                scene.onActivity { result = root(it.window.decorView)!!.semanticsOwner.getAllSemanticsNodes(mergingEnabled = false) }
                return result
            }
            fun text(node: SemanticsNode) = node.config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }.orEmpty()
            fun bounds(label: String): Rect = nodes().first { text(it) == label }.boundsInRoot
            fun has(label: String) = nodes().any { text(it) == label }
            fun gesture(fromX: Float, fromY: Float, toX: Float, toY: Float, steps: Int = 20, hold: Boolean = false, during: (() -> Unit)? = null) {
                val down = SystemClock.uptimeMillis()
                for (step in 0..steps + 1) {
                    if (step == steps + 1 && hold) Thread.sleep(180)
                    scene.onActivity { activity ->
                        val view = root(activity.window.decorView)!!.view
                        val progress = minOf(step, steps).toFloat() / steps
                        val action = when (step) { 0 -> MotionEvent.ACTION_DOWN; steps + 1 -> MotionEvent.ACTION_UP; else -> MotionEvent.ACTION_MOVE }
                        val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action,
                            fromX + (toX - fromX) * progress, fromY + (toY - fromY) * progress, 0)
                        view.dispatchTouchEvent(event); event.recycle()
                    }
                    Thread.sleep(16)
                    if (step == steps / 2) during?.invoke()
                }
                settle()
            }
            fun tap(label: String) { val b = bounds(label); gesture(b.center.x, b.center.y, b.center.x, b.center.y, 1) }
            fun capture(name: String) {
                scene.onActivity { activity ->
                    val view = activity.window.decorView
                    val image = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
                    view.draw(Canvas(image))
                    java.io.File(context.getExternalFilesDir(null), name).outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
                    image.recycle()
                }
            }
            settle()
            capture("card-collapsed.png")
            assertFalse("Copy action starts hidden", has("Copy code"))
            assertFalse("Read action starts hidden", has("Mark read"))
            assertFalse("Dismiss action starts hidden", has("Dismiss"))
            val title = bounds(notice.title)
            var width = 0f
            scene.onActivity { width = root(it.window.decorView)!!.view.width.toFloat() }
            // Vertical movement and a rightward swipe must not acknowledge or dismiss.
            gesture(width / 2, title.center.y + 60, width / 2, title.center.y)
            gesture(width * 0.2f, title.center.y, width * 0.35f, title.center.y, hold = true)
            assertEquals(0, reads); assertEquals(0, dismissals)
            tap(notice.title)
            assertTrue(has("Copy code")); assertTrue(has("Mark read")); assertTrue(has("Dismiss"))
            capture("card-expanded.png")
            tap("Copy code")
            assertEquals(1, copies)
            assertTrue(has("Copy code")) // Child action must not toggle the card.
            tap("Mark read")
            assertEquals(1, reads)
            tap(notice.title)
            assertFalse(has("Copy code")); assertFalse(has("Dismiss"))
            // Diagonal scrolling remains scrolling; slow releases return to rest.
            gesture(width * 0.8f, title.center.y + 60, width * 0.2f, title.center.y + 60 - width * 0.45f,
                during = { assertEquals("Diagonal scrolling must not drag the card", title.left, bounds(notice.title).left, 2f) })
            assertEquals(0, dismissals)
            assertEquals(title.left, bounds(notice.title).left, 2f)
            gesture(width * 0.8f, title.center.y, width * 0.35f, title.center.y, hold = true)
            assertEquals(0, dismissals)
            // Crossing far left does not clear until release; holding stops momentum.
            gesture(width * 0.8f, title.center.y, width * 0.7f, title.center.y, hold = true)
            assertEquals(0, dismissals)
            gesture(width * 0.9f, title.center.y, width * 0.1f, title.center.y, hold = true,
                during = { assertEquals(0, dismissals); capture("card-dragged.png") })
            assertEquals(0, dismissals)
            // The same short distance now clears with a fast release.
            gesture(width * 0.8f, title.center.y, width * 0.4f, title.center.y, steps = 3)
            assertEquals(1, dismissals)
            assertTrue(notice.dismissed)
            // The same card retained in History must return onscreen and stop swiping.
            assertTrue(bounds(notice.title).left >= 0)
            gesture(width * 0.9f, title.center.y, width * 0.1f, title.center.y)
            assertEquals(1, dismissals)
            scene.onActivity { notice = notice.copy(dismissed = false) }
            settle()
            gesture(width * 0.2f, title.center.y, width * 0.6f, title.center.y, steps = 3)
            assertEquals("A rightward flick clears too", 2, dismissals)
        }
    }
}
