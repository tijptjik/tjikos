package hk.type.notjik

import android.app.Application
import android.content.Context
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.TimeUnit

class NotjikApp : Application() {
    override fun onCreate() { super.onCreate(); Transport(this).initFirebase(); SyncWorker.schedule(this) }
}
class Transport(val context: Context) {
    val prefs = context.getSharedPreferences("device",Context.MODE_PRIVATE)
    val store = Store(context)
    private val http = OkHttpClient.Builder().callTimeout(15,TimeUnit.SECONDS).followRedirects(false).build()
    val connected get() = prefs.getBoolean("enrolled",false)
    fun request(value: JSONObject): JSONObject {
        val url = prefs.getString("url", "")!!
        require(url.startsWith("https://")) { "Private HTTPS endpoint required" }
        val req = Request.Builder().url("${url.trimEnd('/')}/api/v1/request")
            .header("X-Notjik-Request","1").header("X-Notjik-Device",prefs.getString("id","")!!)
            .header("X-Notjik-Key",prefs.getString("secret","")!!)
            .post(value.toString().toRequestBody("application/json".toMediaType())).build()
        return http.newCall(req).execute().use { r ->
            check(r.isSuccessful) { "Si unavailable or device access rejected (${r.code}). Check Tailscale." }
            JSONObject(r.body!!.string())
        }
    }
    fun connect(url: String) {
        require(url.startsWith("https://") && android.net.Uri.parse(url).userInfo == null) { "Use the private HTTPS URL" }
        val id = prefs.getString("id",null) ?: UUID.randomUUID().toString()
        val secret = prefs.getString("secret",null) ?: UUID.randomUUID().toString()+UUID.randomUUID().toString()
        prefs.edit().putString("url",url.trimEnd('/')).putString("id",id).putString("secret",secret).commit()
        request(JSONObject().put("op","enroll").put("name","pi").put("kind","android"))
        prefs.edit().putBoolean("enrolled",true).commit()
        fetchFirebaseConfig()
        initFirebase()
    }
    private fun fetchFirebaseConfig() {
        try {
            val config=request(JSONObject().put("op","android-config"))
            prefs.edit().putString("firebase",config.toString()).remove("error").commit()
        } catch (_: Exception) {
            prefs.edit().putString("error","Push setup pending on Si · private drawer is available").apply()
        }
    }
    fun initFirebase() {
        val raw=prefs.getString("firebase",null) ?: return
        try {
            val f=JSONObject(raw)
            if(FirebaseApp.getApps(context).isEmpty()) FirebaseApp.initializeApp(context,FirebaseOptions.Builder()
                .setApplicationId(f.getString("app_id")).setApiKey(f.getString("api_key"))
                .setGcmSenderId(f.getString("sender_id")).setProjectId(f.getString("project_id")).build())
            FirebaseMessaging.getInstance().isAutoInitEnabled=true
            FirebaseMessaging.getInstance().token.addOnSuccessListener { prefs.edit().putString("push",it).apply(); SyncWorker.once(context) }
        } catch (_: Exception) { prefs.edit().putString("error","Firebase setup is incomplete on Si").apply() }
    }
    fun presence() {
        val allowed=androidx.core.app.NotificationManagerCompat.from(context).areNotificationsEnabled()
        val interactive=context.getSystemService(android.os.PowerManager::class.java).isInteractive
        val unlocked=!context.getSystemService(android.app.KeyguardManager::class.java).isKeyguardLocked
        val activity=DrawerPresence.current.snapshot(android.os.SystemClock.elapsedRealtime(),interactive && unlocked)
        request(JSONObject().put("op","presence").put("ready",activity.ready).put("active",activity.active).put("idle_seconds",activity.idleSeconds)
            .put("paused",prefs.getBoolean("paused",false)||!allowed).put("push_token",if(allowed) prefs.getString("push","") else ""))
    }
    fun flush() { for(v in store.pending()) { request(v);store.sent(v.getString("request_id")) } }
    fun history(query: String="", offset: Int=0): JSONObject {
        val response=request(JSONObject().put("op","history").put("query",query).put("offset",offset))
        val cards=response.getJSONArray("notices");for(i in 0 until cards.length()) cards.put(i,store.save(cards.getJSONObject(i)))
        return response
    }
    fun sync() {
        if(!connected) return
        synchronized(syncLock) {
            if(!prefs.contains("firebase")) { fetchFirebaseConfig();initFirebase() }
            presence();flush()
            val ids=JSONArray(context.getSystemService(android.app.NotificationManager::class.java).activeNotifications.mapNotNull { it.tag }.take(1000))
            val poll=request(JSONObject().put("op","poll").put("visible",ids))
            prefs.edit().putString("live",poll.optString("live")).apply()
            val notices=poll.getJSONArray("notices")
            if(!prefs.getBoolean("paused",false)) for(i in 0 until notices.length()) {
                val n=store.save(notices.getJSONObject(i));val id=n.getString("id");val rev=n.optInt("revision",1)
                if(!n.optBoolean("dismissed") && store.revision(id)<rev) { Cards.show(context,n);store.displayed(id,rev) }
                request(JSONObject().put("op","ack").put("id",id).put("revision",rev))
            }
            val dismissed=poll.getJSONArray("dismissed");for(i in 0 until dismissed.length()) {val id=dismissed.getString(i);store.mark(id,true);Cards.cancel(context,id)}
            history()
        }
    }
    fun mutate(op: String,id: String,action: String?=null) {
        val v=JSONObject().put("op",op).put("id",id);action?.let{v.put("action",it)}
        store.enqueue(v)
        if(op=="dismiss"||op=="read")Cards.cancel(context,id)
        SyncWorker.once(context)
    }
    companion object { private val syncLock=Any() }
}
