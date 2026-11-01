package hk.type.notjik

import android.app.Activity
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.PersistableBundle
import org.json.JSONObject

object CodeClipboard {
    fun valid(code: String) = Regex("[A-Za-z0-9 -]{4,32}").matches(code)
    fun code(notice: JSONObject): String? {
        val actions=notice.optJSONArray("actions")
        for(i in 0 until (actions?.length() ?: 0)) {
            val code=actions?.optJSONObject(i)?.optString("copy").orEmpty()
            if(valid(code))return code
        }
        if(notice.optString("type")=="auth-code") {
            val code=notice.optJSONObject("data")?.optString("code").orEmpty()
            if(valid(code))return code
        }
        return null
    }
    fun intent(context: Context,id: String): PendingIntent = PendingIntent.getActivity(context,0,
        Intent(context,CopyCodeActivity::class.java).putExtra("id",id).setData(android.net.Uri.parse("notjik://copy/$id")),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    fun copy(context: Context,code: String) {
        require(valid(code))
        val clip=ClipData.newPlainText("Authentication code",code)
        if(Build.VERSION.SDK_INT>=24)clip.description.extras=PersistableBundle().apply {putBoolean("android.content.extra.IS_SENSITIVE",true)}
        context.getSystemService(ClipboardManager::class.java).setPrimaryClip(clip)
    }
}
// Android restricts background clipboard access. A native notification action opens
// this minimal foreground activity, copies locally, and immediately returns.
class CopyCodeActivity: Activity() {
    private var copied=false
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if(!hasFocus || copied)return
        copied=true
        val id=intent.getStringExtra("id")
        if(id!=null)Store(this).use { store ->
            store.card(id)?.let { notice -> CodeClipboard.code(notice)?.let { code ->
                CodeClipboard.copy(this,code)
                Cards.cancel(this,id)
            } }
        }
        finish()
    }
}
