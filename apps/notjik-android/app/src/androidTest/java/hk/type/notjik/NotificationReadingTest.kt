package hk.type.notjik

import android.content.Intent
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test

class NotificationReadingTest {
    @Test fun singleCardHasEnoughOverflowToCrossFixedToolbarAndReadOnce() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        var notice by mutableStateOf(Notice("synthetic-read", title = "Single card", body = "Cross the toolbar to read."))
        var reads = 0
        var loaded by mutableStateOf(false)
        ActivityScenario.launch<MainActivity>(Intent(context, MainActivity::class.java)).use { scene ->
            scene.onActivity { activity -> activity.setContent {
                MaterialTheme {
                    Column(Modifier.fillMaxSize()) {
                        Box(Modifier.fillMaxWidth().height(100.dp)) { Text("Fixed toolbar") }
                        NotificationList(false, {}, Modifier.weight(1f),
                            unreadIds = if (notice.read || !loaded) emptySet() else setOf(notice.id), lastCardId = if (loaded) notice.id else null,
                            onPassed = { assertEquals(setOf(notice.id), it); reads++; notice = notice.copy(read = true) }) {
                            if (loaded) item(key = notice.id) { NotificationCard(notice, false, {}, {}, {}) }
                        }
                    }
                }
            } }
            fun root(view: View): ViewRootForTest? {
                if (view is ViewRootForTest) return view
                if (view is ViewGroup) for (i in 0 until view.childCount) root(view.getChildAt(i))?.let { return it }
                return null
            }
            fun settle() { instrumentation.waitForIdleSync(); Thread.sleep(400) }
            fun swipe() {
                val time = SystemClock.uptimeMillis()
                for (step in 0..21) {
                    scene.onActivity { activity ->
                        val view = root(activity.window.decorView)!!.view
                        val density = activity.resources.displayMetrics.density
                        val action = when (step) { 0 -> MotionEvent.ACTION_DOWN; 21 -> MotionEvent.ACTION_UP; else -> MotionEvent.ACTION_MOVE }
                        val e = MotionEvent.obtain(time, SystemClock.uptimeMillis(), action, view.width / 2f,
                            (250 - minOf(step, 20) * 6) * density, 0)
                        view.dispatchTouchEvent(e); e.recycle()
                    }
                    Thread.sleep(16)
                }
                settle()
            }
            settle()
            scene.onActivity { loaded = true }
            settle()
            assertEquals(0, reads)
            swipe()
            assertEquals("Crossing the toolbar reads a single short card", 1, reads)
            assertTrue(notice.read)
            swipe()
            assertEquals(1, reads)
            scene.onActivity { activity ->
                val nodes = root(activity.window.decorView)!!.semanticsOwner.getAllSemanticsNodes(false)
                val card = nodes.first { it.config.getOrNull(SemanticsProperties.StateDescription)?.startsWith("Details and actions") == true }
                assertTrue("Card's top can pass beneath toolbar", card.positionInRoot.y < 100 * activity.resources.displayMetrics.density)
                assertTrue("Overflow stops just beyond the boundary", card.positionInRoot.y >= 96 * activity.resources.displayMetrics.density)
            }
        }
    }
}
