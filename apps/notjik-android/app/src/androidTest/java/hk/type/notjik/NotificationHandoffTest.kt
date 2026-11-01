package hk.type.notjik

import android.content.Intent
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test

class NotificationHandoffTest {
    @Test fun upwardScrollCanBecomeCardDragDuringTheSameTouch() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        var dismissals = 0
        ActivityScenario.launch<MainActivity>(Intent(context, MainActivity::class.java)).use { scene ->
            scene.onActivity { activity ->
                activity.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                activity.setShowWhenLocked(true)
                activity.setTurnScreenOn(true)
                activity.setContent {
                    MaterialTheme {
                        NotificationList(false, {}) {
                            item {
                                NotificationCard(Notice("synthetic-handoff", title = "Turn gesture",
                                    body = "Synthetic gesture test"), false, {}, { dismissals++ }, {})
                            }
                            item { Spacer(Modifier.height(1800.dp)) }
                        }
                    }
                }
            }
            fun root(view: View): ViewRootForTest? {
                if (view is ViewRootForTest) return view
                if (view is ViewGroup) for (i in 0 until view.childCount) root(view.getChildAt(i))?.let { return it }
                return null
            }
            fun bounds(): Rect {
                var result = Rect.Zero
                scene.onActivity {
                    result = root(it.window.decorView)!!.semanticsOwner.getAllSemanticsNodes(false).first { node ->
                        node.config.getOrNull(SemanticsProperties.Text)?.any { text -> text.text == "Turn gesture" } == true
                    }.boundsInRoot
                }
                return result
            }
            instrumentation.waitForIdleSync()
            Thread.sleep(500)
            val before = bounds()
            var width = 0f
            scene.onActivity { width = root(it.window.decorView)!!.view.width.toFloat() }
            val down = SystemClock.uptimeMillis()
            fun event(action: Int, x: Float, y: Float) {
                scene.onActivity {
                    val motion = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, x, y, 0)
                    root(it.window.decorView)!!.view.dispatchTouchEvent(motion)
                    motion.recycle()
                }
                Thread.sleep(20)
            }
            val startX = width * 0.8f
            val startY = before.center.y + 70
            event(MotionEvent.ACTION_DOWN, startX, startY)
            for (i in 1..8) event(MotionEvent.ACTION_MOVE, startX, startY - i * 8)
            val scrolled = bounds()
            assertTrue("The gesture must actually scroll the list first", scrolled.top < before.top - 15)
            assertEquals(before.left, scrolled.left, 2f)
            // Turn sideways without another ACTION_DOWN. The card should now follow.
            for (i in 1..12) event(MotionEvent.ACTION_MOVE, startX - width * 0.35f * i / 12, startY - 64)
            val dragged = bounds()
            assertTrue("Sideways turn must pick up the card", dragged.right < scrolled.right - width * 0.15f)
            assertEquals("Horizontal pickup must not jump vertically", scrolled.top, dragged.top, 18f)
            assertEquals(0, dismissals)
            Thread.sleep(180)
            event(MotionEvent.ACTION_UP, startX - width * 0.35f, startY - 64)
            instrumentation.waitForIdleSync()
            Thread.sleep(800)
            assertEquals("Slow release returns to the scrolled position", scrolled.left, bounds().left, 2f)
            assertEquals(scrolled.top, bounds().top, 2f)
            assertEquals(0, dismissals)
        }
    }
}
