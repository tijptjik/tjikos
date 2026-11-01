package hk.type.notjik

import org.json.JSONArray
import org.json.JSONObject

internal data class NoticeAction(val id: String, val label: String, val url: String, val copy: String = "")
internal data class Notice(
    val id: String, val title: String = "", val body: String = "", val app: String = "Notjik",
    val device: String = "", val run: String = "", val updated: Long = 0,
    val read: Boolean = false, val dismissed: Boolean = false,
    val state: String = "", val detail: String = "", val results: List<Pair<String, String>> = emptyList(),
    val actions: List<NoticeAction> = emptyList(), val actionState: String = "",
    val type: String = "standard", val tone: String = "", val duration: String = "",
    val resultTones: List<String> = emptyList(), val bodyFormat: String = "plain",
    val progress: Double? = null, val progressLabel: String = "Progress",
    val timerSeconds: Long? = null, val timerLabel: String = "Remaining", val timerDoneLabel: String = "Ready"
) {
    fun matches(query: String): Boolean {
        val text = (listOf(title, body, app, device, run, state, detail, duration) + results.flatMap { listOf(it.first, it.second) }).joinToString(" ").lowercase()
        return query.lowercase().split(Regex("\\s+")).filter(String::isNotBlank).all(text::contains)
    }
    companion object {
        fun from(n: JSONObject): Notice {
            val data = n.optJSONObject("data")
            val type = n.optString("type", "standard")
            val actions = n.optJSONArray("actions").objects().map {
                NoticeAction(it.optString("id"), it.optString("label"), it.optString("url"), it.optString("copy"))
            }.toMutableList()
            if (type == "auth-code" && actions.none { CodeClipboard.valid(it.copy) }) {
                CodeClipboard.code(n)?.let { actions.add(NoticeAction("", "Copy code", "", it)) }
            }
            val rows = data?.optJSONArray("results") ?: if (type == "key-value") data?.optJSONArray("fields") else null
            val detail = data?.optString("detail").orEmpty().ifBlank {
                when (type) {
                    "todo" -> "${data?.optString("group", "General")} · Due ${data?.optString("due", "today")}"
                    "key-value" -> data?.optString("heading").orEmpty()
                    else -> ""
                }
            }
            return Notice(n.getString("id"), n.optString("title"), n.optString("body"), n.optString("app", "Notjik"),
                n.optString("device"), n.optString("run"), n.optLong("updated", n.optLong("created")),
                n.optBoolean("read"), n.optBoolean("dismissed"), data?.optString("state").orEmpty(), detail,
                rows.objects().map { it.optString("label") to it.optString("value") },
                actions,
                n.optJSONArray("actionJobs")?.optJSONObject(0)?.optString("state").orEmpty(),
                type = type, tone = data?.optString("tone").orEmpty(),
                duration = data?.optString("duration").orEmpty(),
                resultTones = data?.optJSONArray("results").objects().map { it.optString("tone") },
                bodyFormat = n.optString("bodyFormat", "plain"),
                progress = if (type == "progress") data?.optDouble("value", 0.0)?.takeIf { it.isFinite() }?.coerceIn(0.0, 100.0) else null,
                progressLabel = data?.optString("label", "Progress") ?: "Progress",
                timerSeconds = if (type == "timer") data?.optLong("seconds", 0)?.coerceIn(0, 31536000) else null,
                timerLabel = data?.optString("label", "Remaining") ?: "Remaining",
                timerDoneLabel = data?.optString("doneLabel", "Ready") ?: "Ready")
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
