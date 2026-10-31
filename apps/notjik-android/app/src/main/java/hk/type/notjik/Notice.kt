package hk.type.notjik

import org.json.JSONArray
import org.json.JSONObject

internal data class NoticeAction(val id: String, val label: String, val url: String)
internal data class Notice(
    val id: String, val title: String = "", val body: String = "", val app: String = "Notjik",
    val device: String = "", val run: String = "", val updated: Long = 0,
    val read: Boolean = false, val dismissed: Boolean = false,
    val state: String = "", val detail: String = "", val results: List<Pair<String, String>> = emptyList(),
    val actions: List<NoticeAction> = emptyList(), val actionState: String = ""
) {
    fun matches(query: String): Boolean {
        val text = listOf(title, body, app, device, run, state, detail).joinToString(" ").lowercase()
        return query.lowercase().split(Regex("\\s+")).filter(String::isNotBlank).all(text::contains)
    }
    companion object {
        fun from(n: JSONObject): Notice {
            val data = n.optJSONObject("data")
            return Notice(n.getString("id"), n.optString("title"), n.optString("body"), n.optString("app", "Notjik"),
                n.optString("device"), n.optString("run"), n.optLong("updated", n.optLong("created")),
                n.optBoolean("read"), n.optBoolean("dismissed"), data?.optString("state").orEmpty(), data?.optString("detail").orEmpty(),
                data?.optJSONArray("results").objects().map { it.optString("label") to it.optString("value") },
                n.optJSONArray("actions").objects().map { NoticeAction(it.optString("id"), it.optString("label"), it.optString("url")) },
                n.optJSONArray("actionJobs")?.optJSONObject(0)?.optString("state").orEmpty())
        }
    }
}
private fun JSONArray?.objects(): List<JSONObject> = if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it) }
internal enum class DrawerFilter(val label: String) {
    INBOX("Inbox"), UNREAD("Unread"), HISTORY("History");
    fun accepts(n: Notice): Boolean = when (this) {
        INBOX -> !n.dismissed
        UNREAD -> !n.dismissed && !n.read
        HISTORY -> true
    }
}
