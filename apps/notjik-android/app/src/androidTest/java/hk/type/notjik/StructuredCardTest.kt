package hk.type.notjik

import android.text.Spanned
import android.text.style.StyleSpan
import android.app.Notification
import android.app.NotificationManager
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.Assume.assumeTrue
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class StructuredCardTest {
    @Test fun renderSyntheticCardsAndCheckSystemNotification() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        // Run visual acceptance in an isolated package, never the user's inbox.
        assumeTrue(context.packageName.endsWith(".cardtest"))
        val result = JSONObject("""{"id":"synthetic-results","type":"results-table","app":"Tjikup","title":"Tjikup · demo","body":"home:notjik:update: Blocked","data":{"state":"FAILED","tone":"critical","duration":"328s","results":[{"label":"Stages completed","value":"94/102"},{"label":"Issues","value":"8"}]}}""")
        val status = Notice("synthetic-status", title = "SnapRAID · Action needed", app = "Synctjik",
            body = "Job failed or a safety check blocked it. Result=exit-code; exit=1.",
            type = "status", state = "Action needed", tone = "critical", detail = "demo · Sync")
        ActivityScenario.launch<MainActivity>(Intent(context, MainActivity::class.java)).use { scene ->
            scene.onActivity { activity ->
                activity.setContent {
                    MaterialTheme(colorScheme = darkColorScheme()) {
                        Surface(Modifier.fillMaxSize(), color = Color(0xFF15111B), contentColor = Color(0xFFF2EDF4)) {
                            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                                Text("Notjik · synthetic runner cards", style = MaterialTheme.typography.titleLarge)
                                listOf(status, Notice.from(result)).forEach { n ->
                                    Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF221D28), contentColor = Color(0xFFF2EDF4))) {
                                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                                            Text(n.title, style = MaterialTheme.typography.titleMedium)
                                            NoticeContent(n, expanded = false)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            instrumentation.waitForIdleSync()
            Thread.sleep(750)
            // Capture only this synthetic view. A locked/asleep phone can make
            // whole-screen screenshots blank, and its other surfaces are private.
            scene.onActivity { activity ->
                val view = activity.window.decorView
                assertTrue(view.width > 0 && view.height > 0)
                val screenshot = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
                view.draw(Canvas(screenshot))
                val colors = mutableSetOf<Int>()
                for (y in 0 until screenshot.height step 4) for (x in 0 until screenshot.width step 4) colors.add(screenshot.getPixel(x, y))
                assertTrue("Synthetic preview must contain rendered content", colors.size > 20)
                java.io.File(context.getExternalFilesDir(null), "runner-cards.png").outputStream().use {
                    screenshot.compress(Bitmap.CompressFormat.PNG, 100, it)
                }
                screenshot.recycle()
            }
            try {
                Cards.show(context, result)
                val native = context.getSystemService(NotificationManager::class.java).activeNotifications.first { it.tag == "synthetic-results" }.notification
                assertEquals("FAILED · 328s", native.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
                assertTrue(native.extras.getCharSequence(Notification.EXTRA_BIG_TEXT).toString().contains("Stages completed: 94/102"))
            } finally { Cards.cancel(context, "synthetic-results") }
        }
    }

    @Test fun resultsReachTheNativeNotificationAndSearch() {
        val n = Notice.from(JSONObject("""{"id":"synthetic-results","type":"results-table","title":"Tjikup · demo","body":"home:notjik:update: Blocked","data":{"state":"FAILED","tone":"critical","duration":"328s","results":[{"label":"Stages completed","value":"94/102"},{"label":"Issues","value":"8","tone":"warning"}]}}"""))
        assertEquals("results-table", n.type)
        assertEquals("critical", n.tone)
        assertEquals("warning", n.resultTones[1])
        assertEquals("FAILED · 328s", n.compactText())
        val text = n.notificationText()
        assertEquals("FAILED\nStages completed: 94/102\nIssues: 8\nDuration: 328s\n\nhome:notjik:update: Blocked", text.toString())
        assertTrue((text as Spanned).getSpans(0, text.length, StyleSpan::class.java).isNotEmpty())
        assertTrue(n.matches("94/102 328s"))
    }

    @Test fun statusIncludesSourceAndFailureDetails() {
        val n = Notice.from(JSONObject("""{"id":"synthetic-status","type":"status","app":"Synctjik","body":"Safety check blocked sync.","data":{"state":"Action needed","tone":"critical","detail":"demo · Sync"}}"""))
        assertEquals("Action needed · demo · Sync", n.compactText())
        assertEquals("Action needed\ndemo · Sync\n\nSafety check blocked sync.", n.notificationText().toString())
    }

    @Test fun historicalTelegramMarkupIsReadableButLogsStayLiteral() {
        val n = Notice("synthetic-legacy", app = "Synctjik", body = "⚠️ <b>SnapRAID · Action needed</b>\n<code>demo</code> · Sync\n\n<b>Review logs</b>\n<pre>journalctl -u demo-sync -n 50</pre>")
        val body = n.formattedBody()
        assertFalse(body.contains("<b>"))
        assertFalse(body.contains("<pre>"))
        assertTrue(body.contains("journalctl -u demo-sync -n 50"))
        assertTrue((body as Spanned).getSpans(0, body.length, StyleSpan::class.java).isNotEmpty())
        assertEquals("literal <b>log</b>", n.copy(app = "Other", body = "literal <b>log</b>").formattedBody())
    }
}
