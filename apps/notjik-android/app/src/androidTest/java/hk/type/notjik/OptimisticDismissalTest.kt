package hk.type.notjik

import android.content.Intent
import android.view.View
import android.view.ViewGroup
import androidx.activity.compose.setContent
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import org.junit.Assert.*
import org.junit.Test

class OptimisticDismissalTest {
    @Test fun gapClosesWhileSaveIsStillPendingAndFailureRestoresCard() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val overlay = DismissalOverlay()
        val save = CompletableDeferred<Boolean>()
        val cards = listOf(Notice("synthetic-first", title = "First card"), Notice("synthetic-second", title = "Second card"))
        ActivityScenario.launch<MainActivity>(Intent(context, MainActivity::class.java)).use { scene ->
            scene.onActivity { activity ->
                activity.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                activity.setShowWhenLocked(true)
                activity.setTurnScreenOn(true)
                activity.setContent {
                    val scope = rememberCoroutineScope()
                    MaterialTheme {
                        NotificationList(false, {}) {
                            items(overlay.apply(cards).filter(DrawerFilter.INBOX::accepts), key = { it.id }) { n ->
                                NotificationCard(n, false, {}, {
                                    overlay.dismiss(n.id)
                                    scope.launch { if (!save.await()) overlay.restore(n.id) }
                                }, {}, modifier = Modifier.animateItem(fadeInSpec = null, placementSpec = tween(220), fadeOutSpec = null))
                            }
                        }
                    }
                }
            }
            fun root(view: View): ViewRootForTest? {
                if (view is ViewRootForTest) return view
                if (view is ViewGroup) for (i in 0 until view.childCount) root(view.getChildAt(i))?.let { return it }
                return null
            }
            fun top(label: String): Float {
                var result = 0f
                scene.onActivity {
                    result = root(it.window.decorView)!!.semanticsOwner.getAllSemanticsNodes(false).first { node ->
                        node.config.getOrNull(SemanticsProperties.Text)?.any { text -> text.text == label } == true
                    }.boundsInRoot.top
                }
                return result
            }
            fun settle() { instrumentation.waitForIdleSync(); Thread.sleep(500) }
            settle()
            val firstTop = top("First card")
            val secondTop = top("Second card")
            scene.onActivity {
                val action = root(it.window.decorView)!!.semanticsOwner.getAllSemanticsNodes(false)
                    .flatMap { node -> node.config.getOrNull(SemanticsActions.CustomActions).orEmpty() }
                    .first { action -> action.label == "Dismiss" }
                assertTrue(action.action())
            }
            settle()
            assertFalse("Persistence is still blocked", save.isCompleted)
            assertTrue(top("Second card") < secondTop)
            assertEquals("The gap closes before saving finishes", firstTop, top("Second card"), 2f)
            save.complete(false)
            settle()
            assertEquals(firstTop, top("First card"), 2f)
            assertEquals("Failed save restores the original ordering", secondTop, top("Second card"), 2f)
        }
    }
}
