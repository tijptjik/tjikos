package hk.type.notjik

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class StoreTest {
    private lateinit var context: Context
    private lateinit var name: String
    private lateinit var store: Store
    private fun card() = JSONObject().put("id", "synthetic").put("title", "Test only").put("read", false).put("dismissed", false).put("updated", 100)
    @Before fun setup() {
        context = ApplicationProvider.getApplicationContext()
        name = "test-notjik-${UUID.randomUUID()}.db"
        store = Store(context, name)
        store.save(card())
    }
    @After fun cleanup() { store.close(); context.deleteDatabase(name) }
    @Test fun offlineReadSurvivesRestartAndStaleHistoryAfterAck() {
        store.enqueue(JSONObject().put("op", "read").put("id", "synthetic"))
        store.close(); store = Store(context, name)
        assertTrue(store.cards().single().getBoolean("read"))
        val pending = store.pending().single()
        assertEquals("read", pending.getString("op"))
        store.sent(pending.getString("request_id"))
        val stale = store.save(card())
        assertTrue(stale.getBoolean("read"))
        assertTrue(store.pending().isEmpty())
    }
    @Test fun dismissalIsDurableAndIndependentOfRead() {
        store.displayed("synthetic", 4)
        store.enqueue(JSONObject().put("op", "dismiss").put("id", "synthetic"))
        val another = Store(context, name)
        try {
            val refreshed = another.save(card())
            assertTrue(refreshed.getBoolean("dismissed"))
            assertFalse(refreshed.getBoolean("read"))
            assertEquals(4, another.revision("synthetic"))
            assertEquals("dismiss", another.pending().single().getString("op"))
        } finally { another.close() }
    }
}
