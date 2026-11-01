package hk.type.notjik

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.items
import androidx.compose.animation.core.tween
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter

private val Rose = Color(0xFFF1A7C4)
private val Teal = Color(0xFF9CCFD8)
private val Muted = Color(0xFFB3AABB)
private val Canvas = Color(0xFF17131D)

class MainActivity : ComponentActivity() {
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { SyncWorker.once(this) }
    private lateinit var api: Transport
    private var cards by mutableStateOf<List<Notice>>(emptyList())
    private var connected by mutableStateOf(false)
    private var offline by mutableStateOf(false)
    private var error by mutableStateOf<String?>(null)
    private var pending by mutableIntStateOf(0)
    private var search by mutableStateOf("")
    private var filter by mutableStateOf(DrawerFilter.INBOX)
    private var busy by mutableStateOf(false)
    private var pullRefreshing by mutableStateOf(false)
    private var saving by mutableStateOf<Set<String>>(emptySet())
    private var nextOffset by mutableStateOf<Int?>(null)
    private var paginationInitialized = false
    private val refreshMutex = Mutex()
    private val snackbar = SnackbarHostState()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        api = Transport(this)
        connected = api.connected
        intent.getStringExtra("id")?.let { api.mutate("read", it) }
        lifecycleScope.launch {
            reloadSaved()
            repeatOnLifecycle(Lifecycle.State.RESUMED) {
                while (isActive) { if (api.connected) refresh(); delay(10000) }
            }
        }
        if (Build.VERSION.SDK_INT >= 33) permission.launch(Manifest.permission.POST_NOTIFICATIONS)
        setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Rose, secondary = Teal, background = Canvas,
                surface = Canvas, surfaceVariant = Color(0xFF28222F), onSurface = Color(0xFFF2EDF4),
                onSurfaceVariant = Muted, outline = Color(0xFF514657))) {
                Scaffold(containerColor = Canvas, snackbarHost = {
                    SnackbarHost(snackbar) { data ->
                        Snackbar(data,
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                            shape = RoundedCornerShape(16.dp),
                            containerColor = Color(0xFF35263D),
                            contentColor = Color(0xFFF2EDF4),
                            actionColor = Rose,
                            dismissActionContentColor = Rose)
                    }
                }) { padding ->
                    Box(Modifier.fillMaxSize().padding(padding)) { if (connected) Drawer() else Enroll() }
                }
            }
        }
    }
    override fun onResume() { DrawerPresence.current.opened(android.os.SystemClock.elapsedRealtime()); super.onResume() }
    override fun onUserInteraction() { super.onUserInteraction(); DrawerPresence.current.interacted(android.os.SystemClock.elapsedRealtime()) }
    override fun onPause() { DrawerPresence.current.closed(); super.onPause(); if (api.connected) SyncWorker.once(this) }
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.getStringExtra("id")?.let { mutate("read", it) }
    }
    private fun message(text: String) { lifecycleScope.launch { snackbar.showSnackbar(text) } }
    private suspend fun reloadSaved() {
        val saved = withContext(Dispatchers.IO) { api.store.cards().map(Notice::from) to api.store.pending().size }
        cards = saved.first; pending = saved.second
    }
    private suspend fun refresh(reset: Boolean = false, append: Boolean = false) = refreshMutex.withLock {
        val query = search
        val offset = if (append) nextOffset ?: return@withLock else 0
        busy = true
        try {
            val result = withContext(Dispatchers.IO) {
                api.sync()
                val page = api.history(query, offset)
                page to api.prefs.getString("error", null)
            }
            if (search == query && (reset || append || !paginationInitialized)) {
                nextOffset = if (result.first.isNull("next")) null else result.first.getInt("next")
                paginationInitialized = true
            }
            offline = false; error = result.second
        } catch (e: CancellationException) { throw e }
        catch (_: Exception) { offline = true; error = "Si is unreachable. Check Tailscale, then retry." }
        finally { withContext(NonCancellable) { reloadSaved() }; busy = false }
    }
    private fun mutate(op: String, id: String, action: String? = null) {
        if (id in saving) return
        saving = saving + id
        lifecycleScope.launch {
            try {
                withContext(Dispatchers.IO) { api.mutate(op, id, action) }
                reloadSaved()
                message(when (op) { "read" -> "Marked as read"; "dismiss" -> "Dismissed · saved in History"; else -> "Action queued on the source device" })
            } catch (_: Exception) { message("Couldn't save that change. Please try again.") }
            finally { saving = saving - id }
            refresh()
        }
    }
    private fun refreshFromGesture() {
        if (pullRefreshing) return
        pullRefreshing = true
        lifecycleScope.launch {
            try { refresh(reset = true) }
            finally { pullRefreshing = false }
        }
    }
    private fun markPassedCards(ids: Set<String>) {
        val pendingIds = ids.filter { id -> id !in saving && cards.any { it.id == id && !it.read && !it.dismissed } }.toSet()
        if (pendingIds.isEmpty()) return
        saving = saving + pendingIds
        lifecycleScope.launch {
            try {
                withContext(Dispatchers.IO) { api.markRead(pendingIds) }
                reloadSaved()
            } catch (_: Exception) { message("Couldn't save read status. Please try again.") }
            finally { saving = saving - pendingIds }
        }
    }

    @Composable private fun Enroll() {
        var url by rememberSaveable { mutableStateOf(api.prefs.getString("url", "https://si.tailb49776.ts.net/notjik")!!) }
        Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
            Spacer(Modifier.height(36.dp)); Brand()
            Text("Your notifications, wherever you are.", style = MaterialTheme.typography.headlineSmall)
            Text("Connect Tailscale, then connect Pi to your private notification history.", color = Muted)
            OutlinedTextField(url, { url = it }, label = { Text("Private service URL") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            Button(enabled = !busy, onClick = {
                lifecycleScope.launch {
                    busy = true
                    try { withContext(Dispatchers.IO) { api.connect(url) }; connected = true; refresh(reset = true) }
                    catch (_: Exception) { error = "Couldn't connect. Check the address and Tailscale." }
                    finally { busy = false }
                }
            }, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(if (busy) "Connecting…" else "Connect Pi") }
            error?.let { Text(it, color = Rose) }
        }
    }
    @Composable private fun Brand() {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.size(44.dp).clip(RoundedCornerShape(14.dp)).background(Color(0xFF26233A)), contentAlignment = Alignment.Center) {
                Glyph(R.drawable.ic_notification, null, Teal, Modifier.size(34.dp))
            }
            Column { Text("Notjik", fontSize = 24.sp, fontWeight = FontWeight.SemiBold); Text("Notifications", color = Muted, fontSize = 12.sp) }
        }
    }
    @Composable private fun Drawer() {
        LaunchedEffect(search) { delay(300); refresh(reset = true) }
        val matching = cards.filter { it.matches(search) }
        val shown = matching.filter(filter::accepts)
        Column(Modifier.fillMaxSize()) {
            Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Brand()
                NotificationSearch(search, { search = it }, filter, { filter = it }, matching)
                val status = buildList {
                    if (pending > 0) add("$pending ${if (pending == 1) "change" else "changes"} waiting to sync")
                    if (offline || error != null) add(error ?: "Offline · changes saved on Pi")
                }
                if (status.isNotEmpty()) Text(status.joinToString(" · "), fontSize = 12.sp, color = if (offline || error != null) Rose else Muted)
            }
            HorizontalDivider(color = Color(0xFF302837))
            NotificationList(pullRefreshing, ::refreshFromGesture, Modifier.weight(1f).fillMaxWidth(),
                unreadIds = shown.filter { !it.read && !it.dismissed }.map { it.id }.toSet(),
                lastCardId = shown.lastOrNull()?.id, feedKey = "${filter.name}:$search", onPassed = ::markPassedCards) {
                if (shown.isEmpty()) item {
                    Column(Modifier.fillMaxWidth().padding(vertical = 32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Glyph(if (filter == DrawerFilter.HISTORY) R.drawable.ic_history else R.drawable.ic_inbox, null, Teal, Modifier.size(36.dp))
                        Text(if (search.isNotBlank()) "No matching notifications" else if (filter == DrawerFilter.HISTORY) "Your history starts here" else "All caught up", fontSize = 20.sp, fontWeight = FontWeight.Medium)
                        Text(if (search.isNotBlank()) "Try an app, device, or result." else if (filter == DrawerFilter.HISTORY) "Script results will appear here." else "Dismissed notifications are kept in History.", color = Muted, fontSize = 13.sp)
                        if (filter != DrawerFilter.HISTORY && search.isBlank()) TextButton(onClick = { filter = DrawerFilter.HISTORY }) { Text("View history") }
                    }
                }
                shown.groupBy { dateLabel(it.updated) }.forEach { (date, notices) ->
                    item(key = "date:$date") { Text(date, color = Muted, fontSize = 12.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(top = 4.dp, bottom = 2.dp)) }
                    items(notices, key = { it.id }) { n ->
                        NotificationCard(n, saving = n.id in saving,
                            onRead = { mutate("read", n.id) },
                            onDismiss = { mutate("dismiss", n.id) },
                            onAction = { performAction(n, it) },
                            modifier = Modifier.animateItem(fadeInSpec = null, placementSpec = tween(220), fadeOutSpec = null))
                    }
                }
                if (nextOffset != null) item { OutlinedButton(onClick = { lifecycleScope.launch { refresh(append = true) } }, enabled = !busy, modifier = Modifier.fillMaxWidth()) { Text("Load earlier notifications") } }
            }
        }
    }
    private fun performAction(n: Notice, action: NoticeAction) {
        if (action.copy.isNotBlank()) {
            CodeClipboard.copy(this, action.copy)
            message("Code copied")
        } else if (action.url.isNotBlank()) {
            val uri = android.net.Uri.parse(action.url)
            try { if (uri.scheme == "https" && uri.userInfo == null) startActivity(Intent(Intent.ACTION_VIEW, uri)) }
            catch (_: Exception) { message("No application can open this link") }
        } else mutate("invoke", n.id, action.id)
    }

}
@Composable private fun Glyph(resource: Int, description: String?, tint: Color = LocalContentColor.current, modifier: Modifier = Modifier.size(20.dp)) {
    Icon(painterResource(resource), contentDescription = description, tint = tint, modifier = modifier)
}
private fun dateLabel(seconds: Long): String {
    if (seconds <= 0) return "Earlier"
    val date = Instant.ofEpochSecond(seconds).atZone(ZoneId.systemDefault()).toLocalDate()
    return when (date) { LocalDate.now() -> "Today"; LocalDate.now().minusDays(1) -> "Yesterday"; else -> date.format(DateTimeFormatter.ofPattern("d MMM yyyy")) }
}
