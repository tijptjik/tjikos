// Clipboard content describes the message, never its controls or private metadata.
function text(value) {
    return String(value ?? "").replace(/\r\n?/g, "\n").trim();
}

function inline(value) {
    return text(value).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
        .replace(/([\\`*_\[\]{}|])/g, "\\$1").replace(/\n/g, "<br>");
}

function field(label, value) {
    return "**" + inline(label) + ":** " + inline(value);
}

function stamp(seconds) {
    return Math.floor(seconds / 60).toString().padStart(2, "0") + ":"
        + (seconds % 60).toString().padStart(2, "0");
}

function table(rows) {
    return rows.length ? ["| Field | Value |", "| --- | --- |"].concat(
        rows.map(row => "| " + inline(row[0]) + " | " + inline(row[1]) + " |")
    ).join("\n") : "";
}

function markdown(notice, now, presentation) {
    const data = presentation ? presentation.data : (notice.data || {});
    const kind = presentation ? presentation.type : (notice.type || "standard");
    // A parsed presentation replaces the raw body, just as it does on the card.
    const parts = presentation ? [] : [text(notice.body)];
    switch (kind) {
    case "status":
        parts.push(field("Status", data.state || "Status"), text(data.detail));
        break;
    case "progress":
        parts.push(field(data.label || "Progress", Math.round(data.value || 0) + "%"));
        break;
    case "timer": {
        const seconds = Math.max(0, Math.ceil(((notice.updatedAt || notice.received)
            + (data.seconds || 0) * 1000 - now) / 1000));
        parts.push(field(seconds === 0 ? (data.doneLabel || "Ready") : (data.label || "Remaining"), stamp(seconds)));
        break;
    }
    case "results-table": {
        const rows = [];
        for (const result of data.results || []) {
            rows.push([result.label || "Result", result.value ?? ""]);
            for (const ingredient of result.ingredients || [])
                rows.push(["↳ " + (ingredient.label || ""), ingredient.value ?? ""]);
        }
        parts.push(table(rows), field("Status", data.state || "Running"));
        if (data.duration) parts.push(field("Duration", data.duration));
        break;
    }
    case "key-value":
        if (data.heading) parts.push("### " + inline(data.heading));
        parts.push(table((data.fields || []).map(item => [item.label, item.value])));
        break;
    case "todo":
        parts.push(field("Group", data.group || "General"), field("Due", data.due || "today"));
        break;
    case "telegram": {
        if (!text(notice.body) && data.caption) parts.push(text(data.caption));
        const media = {photo: "Photo", voice: "Voice message", sticker: "Sticker", emoji: "Animated emoji", animation: "Animation"};
        if (media[data.kind]) {
            let description = media[data.kind];
            if (data.duration > 0) description += " (" + stamp(Math.round(data.duration)) + ")";
            if (data.fallback) description += " · " + data.fallback;
            parts.push(field("Media", description));
        }
        if (data.transcript) parts.push("**Transcript:**\n\n" + text(data.transcript));
        break;
    }
    }
    return parts.filter(Boolean).join("\n\n") || text(notice.title);
}
