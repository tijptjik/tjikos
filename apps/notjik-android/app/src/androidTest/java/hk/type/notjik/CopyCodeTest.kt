package hk.type.notjik

import android.app.NotificationManager
import android.content.ClipboardManager
import android.content.ClipData
import android.content.Intent
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class CopyCodeTest {
    @Test fun structuredCodeSupportsServersWithoutCopyActions() {
        val notice=JSONObject("""{"id":"synthetic-structured-code","type":"auth-code","data":{"code":"004281"}}""")
        assertEquals("004281",CodeClipboard.code(notice))
        assertEquals("004281",Notice.from(notice).actions.single().copy)
        notice.put("actions",org.json.JSONArray("""[{"label":"Copy code","copy":"004281"}]"""))
        assertEquals(1,Notice.from(notice).actions.size)
        notice.remove("actions")
        notice.put("type","standard")
        assertNull(CodeClipboard.code(notice))
        assertTrue(Notice.from(notice).actions.isEmpty())
        notice.put("type","auth-code").getJSONObject("data").put("code","<invalid>")
        assertNull(CodeClipboard.code(notice))
        assertTrue(Notice.from(notice).actions.isEmpty())
    }
    @Test fun nativePrimaryActionCopiesOnThisDevice() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val id="synthetic-copy-action-test"
        val notice=JSONObject("""{"id":"synthetic-copy-action-test","type":"auth-code","title":"Synthetic authentication code","body":"Local action test","actions":[{"label":"Copy code","copy":"004281"}]}""")
        val store=Store(context)
        var previous:ClipData?=null
        ActivityScenario.launch<MainActivity>(Intent(context,MainActivity::class.java)).use { scene ->
            scene.onActivity {previous=it.getSystemService(ClipboardManager::class.java).primaryClip}
            try {
                store.save(notice)
                Cards.show(context,notice)
                val native=context.getSystemService(NotificationManager::class.java).activeNotifications.first {it.tag==id}.notification
                assertEquals("Copy code",native.actions.first().title.toString())
                native.actions.first().actionIntent.send()
                Thread.sleep(1000)
                scene.onActivity {
                    assertEquals("004281",it.getSystemService(ClipboardManager::class.java).primaryClip?.getItemAt(0)?.text.toString())
                }
            } finally {
                scene.onActivity {
                    val clipboard=it.getSystemService(ClipboardManager::class.java)
                    previous?.let(clipboard::setPrimaryClip) ?: clipboard.setPrimaryClip(ClipData.newPlainText("",""))
                }
                Cards.cancel(context,id)
                store.writableDatabase.delete("cards","id=?",arrayOf(id))
                store.close()
            }
        }
    }
}
