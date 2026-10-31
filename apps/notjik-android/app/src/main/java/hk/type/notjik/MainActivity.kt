package hk.type.notjik

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
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
    private var paused by mutableStateOf(false)
    private var offline by mutableStateOf(false)
    private var error by mutableStateOf<String?>(null)
    private var pending by mutableIntStateOf(0)
    private var search by mutableStateOf("")
    private var filter by mutableStateOf(DrawerFilter.INBOX)
    private var busy by mutableStateOf(false)
    private var saving by mutableStateOf<Set<String>>(emptySet())
    private var nextOffset by mutableStateOf<Int?>(null)
    private var paginationInitialized = false
    private val refreshMutex = Mutex()
    private val snackbar = SnackbarHostState()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        api = Transport(this)
        connected = api.connected
        paused = api.prefs.getBoolean("paused", false)
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
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
                                .border(1.dp, Color(0xFF604658), RoundedCornerShape(16.dp)),
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
    private fun togglePause() {
        paused = !paused
        api.prefs.edit().putBoolean("paused", paused).apply()
        lifecycleScope.launch { refresh() }
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
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                    Brand()
                    Surface(shape = RoundedCornerShape(24.dp), color = Color(0xFF221D28), border = BorderStroke(1.dp, Color(0xFF403447))) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            IconToggleButton(checked = paused, onCheckedChange = { togglePause() },
                                modifier = Modifier.size(48.dp).semantics { stateDescription = if (paused) "Notifications paused" else "Notifications enabled" },
                                colors = IconButtonDefaults.iconToggleButtonColors(checkedContainerColor = Rose.copy(alpha = 0.12f), checkedContentColor = Rose)) {
                                Glyph(if (paused) R.drawable.ic_play else R.drawable.ic_pause,
                                    if (paused) "Resume notifications on Pi" else "Pause notifications on Pi")
                            }
                            IconButton(onClick = { lifecycleScope.launch { refresh(reset = true) } }, enabled = !busy,
                                modifier = Modifier.size(48.dp).semantics { contentDescription = if (busy) "Refreshing notifications" else "Refresh notifications" }) {
                                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                                else Glyph(R.drawable.ic_refresh, null)
                            }
                        }
                    }
                }
                OutlinedTextField(search, { search = it }, modifier = Modifier.fillMaxWidth(), singleLine = true,
                    shape = RoundedCornerShape(14.dp), placeholder = { Text("Search notifications", fontSize = 14.sp) },
                    leadingIcon = { Glyph(R.drawable.ic_search, null, Muted) },
                    trailingIcon = { if (search.isNotEmpty()) IconButton(onClick = { search = "" }) { Glyph(R.drawable.ic_close, "Clear search") } })
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    DrawerFilter.entries.forEach { choice ->
                        val count = matching.count(choice::accepts)
                        FilterChip(selected = choice == filter, onClick = { filter = choice }, label = { Text("${choice.label} $count", fontSize = 12.sp) })
                    }
                }
                val status = buildList {
                    if (pending > 0) add("$pending ${if (pending == 1) "change" else "changes"} waiting to sync")
                    if (offline || error != null) add(error ?: "Offline · changes saved on Pi")
                }
                if (status.isNotEmpty()) Text(status.joinToString(" · "), fontSize = 12.sp, color = if (offline || error != null) Rose else Muted)
            }
            HorizontalDivider(color = Color(0xFF302837))
            LazyColumn(Modifier.weight(1f).fillMaxWidth(), contentPadding = PaddingValues(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 28.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
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
                    items(notices, key = { it.id }) { NotificationCard(it) }
                }
                if (nextOffset != null) item { OutlinedButton(onClick = { lifecycleScope.launch { refresh(append = true) } }, enabled = !busy, modifier = Modifier.fillMaxWidth()) { Text("Load earlier notifications") } }
            }
        }
    }
    @Composable private fun NotificationCard(n: Notice) {
        var expanded by rememberSaveable(n.id) { mutableStateOf(false) }
        val unread = !n.read && !n.dismissed
        Card(modifier = Modifier.fillMaxWidth(), shape = RoundedCornerShape(18.dp),
            colors = CardDefaults.cardColors(containerColor = if (unread) Color(0xFF2A2331) else Color(0xFF221D28)),
            border = BorderStroke(1.dp, if (unread) Color(0xFF65465E) else Color(0xFF352C3E))) {
            Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (unread) Box(Modifier.size(6.dp).clip(RoundedCornerShape(3.dp)).background(Rose))
                    Text(n.app, color = if (unread) Rose else Muted, fontSize = 12.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                    if (n.device.isNotBlank()) Text(n.device.replaceFirstChar { it.uppercase() }, fontSize = 11.sp, color = Teal,
                        modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(Teal.copy(alpha = 0.08f)).padding(horizontal = 6.dp, vertical = 3.dp))
                    Text(timeLabel(n.updated), color = Muted, fontSize = 11.sp)
                }
                Text(n.title, fontSize = 16.sp, lineHeight = 22.sp, fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Medium)
                if (n.body.isNotBlank()) SelectionContainer { Text(n.body, fontSize = 14.sp, lineHeight = 21.sp, color = Color(0xFFCBC2D1), maxLines = if (expanded) Int.MAX_VALUE else 2, overflow = TextOverflow.Ellipsis) }
                if (n.state.isNotBlank()) Text(n.state, color = Teal, fontSize = 12.sp)
                if (expanded) {
                    if (n.detail.isNotBlank()) SelectionContainer { Text(n.detail, fontSize = 13.sp, color = Muted) }
                    n.results.forEach { (label, value) ->
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Text(label, Modifier.weight(1f), color = Muted, fontSize = 12.sp)
                            Text(value, Modifier.weight(1f), fontSize = 12.sp)
                        }
                    }
                    if (n.run.isNotBlank()) Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("RUN", color = Muted, fontSize = 10.sp)
                        SelectionContainer { Text(n.run, fontSize = 11.sp, lineHeight = 16.sp, color = Muted) }
                    }
                    if (n.actions.isNotEmpty()) {
                        HorizontalDivider(color = Color(0xFF403447))
                        Text("Actions · ${n.device.ifBlank { "source device" }}", color = Muted, fontSize = 11.sp)
                        n.actions.forEach { action ->
                            OutlinedButton(onClick = {
                                if (action.url.isNotBlank()) {
                                    val uri = android.net.Uri.parse(action.url)
                                    try { if (uri.scheme == "https" && uri.userInfo == null) startActivity(Intent(Intent.ACTION_VIEW, uri)) }
                                    catch (_: Exception) { message("No application can open this link") }
                                } else mutate("invoke", n.id, action.id)
                            }, enabled = n.id !in saving, modifier = Modifier.fillMaxWidth(), shape = RoundedCornerShape(10.dp)) { Text(action.label) }
                        }
                    }
                    if (n.actionState.isNotBlank()) Text("Last action · ${n.actionState}", color = Teal, fontSize = 12.sp)
                }
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    if (n.dismissed) {
                        Glyph(R.drawable.ic_history, null, Muted, Modifier.size(16.dp)); Spacer(Modifier.width(6.dp))
                        Text("Dismissed", color = Muted, fontSize = 12.sp, modifier = Modifier.weight(1f))
                    } else {
                        TextButton(onClick = { mutate("read", n.id) }, enabled = !n.read && n.id !in saving, contentPadding = PaddingValues(horizontal = 4.dp)) {
                            Glyph(R.drawable.ic_check, null, modifier = Modifier.size(16.dp)); Spacer(Modifier.width(5.dp)); Text(if (n.read) "Read" else "Mark read", fontSize = 12.sp)
                        }
                        Spacer(Modifier.weight(1f))
                        TextButton(onClick = { mutate("dismiss", n.id) }, enabled = n.id !in saving, contentPadding = PaddingValues(horizontal = 8.dp)) { Text("Dismiss", fontSize = 12.sp) }
                    }
                    IconButton(onClick = { expanded = !expanded }, modifier = Modifier.size(48.dp)) {
                        Glyph(if (expanded) R.drawable.ic_collapse else R.drawable.ic_expand, if (expanded) "Hide details" else "Show details", Muted)
                    }
                }
            }
        }
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
private fun timeLabel(seconds: Long): String = if (seconds <= 0) "" else Instant.ofEpochSecond(seconds).atZone(ZoneId.systemDefault()).format(DateTimeFormatter.ofPattern("HH:mm"))
