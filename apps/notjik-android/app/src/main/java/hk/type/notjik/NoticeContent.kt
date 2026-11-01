package hk.type.notjik

import android.graphics.Typeface
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.StyleSpan
import android.text.style.TypefaceSpan
import androidx.core.text.HtmlCompat
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import kotlinx.coroutines.delay
import androidx.compose.material3.LinearProgressIndicator
import kotlin.math.roundToInt
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Historical SnapRAID messages were Telegram HTML sent as a plain body.
// New producers send data fields. Do not interpret arbitrary log text as HTML.
internal fun Notice.formattedBody(): CharSequence = if (bodyFormat == "html" ||
    (app.equals("Synctjik", ignoreCase = true) && body.contains("<b>SnapRAID ·"))) {
    HtmlCompat.fromHtml(body.replace("\n", "<br>"), HtmlCompat.FROM_HTML_MODE_COMPACT).trim()
} else body

internal fun Notice.compactText(): CharSequence = listOf(state, detail, duration)
    .filter(String::isNotBlank).joinToString(" · ").ifBlank { formattedBody().toString() }

// Android owns the shade layout. Native spans and BigTextStyle preserve the
// information without depending on HTML parsing or custom RemoteViews layouts.
internal fun Notice.notificationText(): CharSequence {
    val text = SpannableStringBuilder()
    fun line(label: String, value: String = "") {
        if (text.isNotEmpty()) text.append('\n')
        val start = text.length
        text.append(label)
        text.setSpan(StyleSpan(Typeface.BOLD), start, text.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        if (value.isNotBlank()) text.append(": ").append(value)
    }
    if (state.isNotBlank()) line(state)
    if (detail.isNotBlank()) { if (text.isNotEmpty()) text.append('\n'); text.append(detail) }
    results.forEach { (label, value) -> line(label, value) }
    if (duration.isNotBlank()) line("Duration", duration)
    progress?.let { line(progressLabel, "${it.roundToInt()}%") }
    timerSeconds?.let { line(timerLabel, timerText(it)) }
    if (body.isNotBlank()) { if (text.isNotEmpty()) text.append("\n\n"); text.append(formattedBody()) }
    return text
}

private fun CharSequence.annotated() = buildAnnotatedString {
    append(this@annotated.toString())
    val spanned = this@annotated as? Spanned ?: return@buildAnnotatedString
    spanned.getSpans(0, spanned.length, Any::class.java).forEach { span ->
        val style = when (span) {
            is StyleSpan -> SpanStyle(
                fontWeight = if (span.style and Typeface.BOLD != 0) FontWeight.Bold else null,
                fontStyle = if (span.style and Typeface.ITALIC != 0) FontStyle.Italic else null)
            is TypefaceSpan -> if (span.family == "monospace") SpanStyle(fontFamily = FontFamily.Monospace) else null
            else -> null
        }
        if (style != null) addStyle(style, spanned.getSpanStart(span), spanned.getSpanEnd(span))
    }
}

private fun toneColor(tone: String) = when (tone) {
    "critical" -> Color(0xFFFF819D)
    "warning" -> Color(0xFFF2C879)
    "success" -> Color(0xFF9CD6C9)
    else -> Color(0xFFCBC2D1)
}

@Composable internal fun NoticeContent(n: Notice, expanded: Boolean) {
    val ink = toneColor(n.tone)
    n.progress?.let { progress ->
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(n.progressLabel, color = ink, fontSize = 13.sp)
            Text("${progress.roundToInt()}%", color = ink, fontSize = 13.sp)
        }
        LinearProgressIndicator(progress = { (progress / 100).toFloat() }, modifier = Modifier.fillMaxWidth(), color = Color(0xFF9CCFD8))
    }
    n.timerSeconds?.let { total ->
        val remaining by produceState(initialValue = (n.updated + total - System.currentTimeMillis() / 1000).coerceAtLeast(0), n.id, n.updated, total) {
            do {
                value = (n.updated + total - System.currentTimeMillis() / 1000).coerceAtLeast(0)
                if (value > 0) delay(1000)
            } while (value > 0)
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(timerText(remaining), color = Color(0xFF9CCFD8), fontSize = 28.sp)
            Text(if (remaining == 0L) n.timerDoneLabel else n.timerLabel, color = ink, fontSize = 13.sp)
        }
    }
    if (n.state.isNotBlank() || n.detail.isNotBlank() || n.duration.isNotBlank()) {
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(ink.copy(alpha = 0.07f)).padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(n.state, Modifier.weight(1f), color = ink, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                if (n.duration.isNotBlank()) Text(n.duration, color = Color(0xFFB5AABC), fontSize = 12.sp)
            }
            if (n.detail.isNotBlank()) Text(n.detail, color = Color(0xFFCBC2D1), fontSize = 12.sp)
        }
    }
    // Results remain visible when collapsed; expansion reveals long body text
    // and actions, rather than hiding the card's primary information.
    if (n.results.isNotEmpty()) Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        n.results.forEachIndexed { index, (label, value) ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(label, Modifier.weight(1f), color = Color(0xFFB5AABC), fontSize = 13.sp)
                Text(value, Modifier.weight(1f), color = toneColor(n.resultTones.getOrElse(index) { "" }),
                    fontSize = 13.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.End)
            }
        }
        if (n.body.isNotBlank()) HorizontalDivider(color = Color(0xFF403447))
    }
    if (n.body.isNotBlank()) SelectionContainer {
        Text(n.formattedBody().annotated(), fontSize = 14.sp, lineHeight = 21.sp, color = Color(0xFFCBC2D1),
            maxLines = if (expanded) Int.MAX_VALUE else 2, overflow = TextOverflow.Ellipsis)
    }
}

private fun timerText(seconds: Long): String = "%02d:%02d".format(seconds / 60, seconds % 60)
