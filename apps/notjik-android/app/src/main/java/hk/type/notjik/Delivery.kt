package hk.type.notjik

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.work.*
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.util.concurrent.TimeUnit

object Cards {
    private fun intent(context: Context, id: String, op: String, action: String=""): PendingIntent {
        val i=Intent(context,CardReceiver::class.java).setAction(op).putExtra("id",id).putExtra("action",action)
            .setData(android.net.Uri.parse("notjik://card/$id/$op/$action"))
        return PendingIntent.getBroadcast(context,0,i,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }
    fun show(context: Context,n: JSONObject) {
        val manager=context.getSystemService(NotificationManager::class.java)
        check(androidx.core.app.NotificationManagerCompat.from(context).areNotificationsEnabled()) { "Notification permission required" }
        val urgent=n.optString("urgency")=="critical"
        val channel=if(urgent) "urgent" else "results"
        manager.createNotificationChannel(NotificationChannel(channel,if(urgent)"Urgent results" else "Script results",if(urgent)NotificationManager.IMPORTANCE_HIGH else NotificationManager.IMPORTANCE_DEFAULT))
        val id=n.getString("id")
        val open=Intent(context,MainActivity::class.java).putExtra("id",id).setData(android.net.Uri.parse("notjik://open/$id"))
        val copy=CodeClipboard.code(n)
        val copyIntent=CodeClipboard.intent(context,id)
        val builder=NotificationCompat.Builder(context,channel).setSmallIcon(hk.type.notjik.R.drawable.ic_notification)
            .setContentTitle(n.optString("title")).setContentText(n.optString("body"))
            .setStyle(NotificationCompat.BigTextStyle().bigText(n.optString("body")))
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE).setOnlyAlertOnce(true).setAutoCancel(true)
            .setContentIntent(if(copy!=null) copyIntent else PendingIntent.getActivity(context,0,open,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            .setDeleteIntent(intent(context,id,"dismiss"))
        if(copy!=null)builder.addAction(NotificationCompat.Action.Builder(0,"Copy code",copyIntent).setAuthenticationRequired(true).build())
        builder.addAction(0,"Mark read",intent(context,id,"read"))
        val actions=n.optJSONArray("actions")
        if(actions!=null)for(i in 0 until minOf(2,actions.length())) {
            val a=actions.getJSONObject(i)
            if(a.has("id"))builder.addAction(0,a.optString("label"),intent(context,id,"invoke",a.getString("id")))
        }
        manager.notify(id,0,builder.build())
    }
    fun cancel(context: Context,id: String) { context.getSystemService(NotificationManager::class.java).cancel(id,0) }
}
class CardReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context,intent: Intent) {
        val id=intent.getStringExtra("id") ?: return
        val op=intent.action ?: return
        if(op !in listOf("dismiss","read","invoke"))return
        Transport(context).mutate(op,id,intent.getStringExtra("action"))
    }
}
class PushService : FirebaseMessagingService() {
    override fun onNewToken(token: String) { getSharedPreferences("device",MODE_PRIVATE).edit().putString("push",token).apply();SyncWorker.once(this) }
    override fun onMessageReceived(message: RemoteMessage) {
        // Treat push as an untrusted wakeup only. No content, URL or action is accepted.
        if(message.data.keys==setOf("reference"))SyncWorker.once(this,true)
    }
}
class SyncWorker(context: Context,params: WorkerParameters): CoroutineWorker(context,params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        try { Transport(applicationContext).sync();Result.success() }
        catch (e: kotlinx.coroutines.CancellationException) { throw e }
        catch (e: Exception) {
            // Diagnostics deliberately omit messages, URLs, payloads and credentials.
            val source=e.stackTrace.firstOrNull { it.className.startsWith("hk.type.notjik.") }
            android.util.Log.w("NotjikSync", "Private sync failed: ${e.javaClass.simpleName} at $source")
            Result.retry()
        }
    }
    companion object {
        fun once(context: Context,urgent: Boolean=false) {
            val b=OneTimeWorkRequestBuilder<SyncWorker>().setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL,15,TimeUnit.SECONDS)
            if(urgent)b.setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            // A fresh FCM wakeup must not wait behind an older request's backoff.
            WorkManager.getInstance(context).enqueueUniqueWork(if(urgent)"notification-push" else "notification-delivery",
                if(urgent)ExistingWorkPolicy.REPLACE else ExistingWorkPolicy.APPEND_OR_REPLACE,b.build())
        }
        fun schedule(context: Context) {
            WorkManager.getInstance(context).enqueueUniquePeriodicWork("notification-reconcile",ExistingPeriodicWorkPolicy.UPDATE,
                PeriodicWorkRequestBuilder<SyncWorker>(15,TimeUnit.MINUTES).setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()).build())
        }
    }
}
