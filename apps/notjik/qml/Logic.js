function offset(value, extent, scale) {
    return typeof value === "string" ? parseFloat(value) * extent / 100 : Number(value || 0) / (scale || 1);
}

function placement(config, screen, index) {
    var base = config.placement;
    return Object.assign({}, base, base.perMonitor[String(index + 1)] || {}, base.perMonitor[screen.name] || {});
}

function selected(config, screen, index, primaryName) {
    var selectors = config.placement.monitors;
    var chosen = selectors === "primary" ? (primaryName ? screen.name === primaryName : index === 0)
        : selectors === "all" || selectors.indexOf(index + 1) >= 0 || selectors.indexOf(screen.name) >= 0;
    return chosen && placement(config, screen, index).enabled !== false;
}

function geometry(p, sw, sh, width, height, scale) {
    var margin = p.margin === undefined ? 16 : p.margin;
    var horizontal = p.edge === "t" || p.edge === "b";
    var x = horizontal ? (p.position === "l" ? margin : p.position === "r" ? sw - width - margin : (sw - width) / 2)
                       : (p.edge === "l" ? margin : sw - width - margin);
    var y = horizontal ? (p.edge === "t" ? margin : sh - height - margin)
                       : (p.position === "l" ? margin : p.position === "r" ? sh - height - margin : (sh - height) / 2);
    return {x: Math.round(Math.max(0, Math.min(sw - width, x + offset(p.offsetX, sw, scale)))),
            y: Math.round(Math.max(0, Math.min(sh - height, y + offset(p.offsetY, sh, scale))))};
}

function timeout(value, urgency, config) {
    return value === undefined || value < 0 ? config.timeouts[urgency] * 1000 : value;
}

function validateNotice(n) {
    if (!n || typeof n !== "object" || Array.isArray(n)) throw Error("notice must be an object");
    if (typeof n.title !== "string" || n.title.length > 4096) throw Error("title must be text, at most 4096 characters");
    for (var key of ["body", "app", "type", "icon", "image"])
        if (n[key] !== undefined && (typeof n[key] !== "string" || n[key].length > 65536)) throw Error(key + " must be text, at most 65536 characters");
    if (n.id != null && (typeof n.id !== "string" || !/^[a-zA-Z0-9][a-zA-Z0-9_.:/-]{0,127}$/.test(n.id)))
        throw Error("id must be 1-128 letters, digits, dots, underscores, colons, slashes or hyphens");
    if (n.id != null && n.replace) throw Error("id and replace cannot be combined");
    if (n.urgency !== undefined && ["low", "normal", "critical"].indexOf(n.urgency) < 0) throw Error("invalid urgency");
    if (n.group !== undefined && n.group !== null && (typeof n.group !== "string" || n.group.length > 256)) throw Error("group must be text, at most 256 characters");
    if (n.timeout !== undefined && (typeof n.timeout !== "number" || !isFinite(n.timeout) || n.timeout < -1 || n.timeout > 604800000)) throw Error("timeout must be -1, 0, or milliseconds up to 7 days");
    if (n.data !== undefined && (!n.data || typeof n.data !== "object" || Array.isArray(n.data))) throw Error("data must be an object");
    if (n.actions !== undefined && (!Array.isArray(n.actions) || n.actions.length > 16)) throw Error("actions must be an array of at most 16 actions");
    for (var action of (n.actions || [])) {
        if (!action || typeof action.label !== "string" || !action.label.length || action.label.length > 120) throw Error("each action needs a label (1–120 characters)");
        if (!Array.isArray(action.command) || !action.command.length || action.command.length > 64 ||
            action.command.some(arg => typeof arg !== "string" || arg.indexOf("\0") >= 0 || arg.length > 8192) || !action.command[0])
            throw Error("action.command must be a nonempty array of command arguments");
    }
    if (n.type === "telegram") {
        const d = n.data || {};
        if (!["text", "photo", "voice", "sticker", "emoji", "animation"].includes(d.kind)) throw Error("Invalid Telegram media kind");
        for (const key of ["chatId", "messageId"])
            if (typeof d[key] !== "string" || !/^-?[1-9][0-9]{0,15}$/.test(d[key]) || !Number.isSafeInteger(Number(d[key])))
                throw Error("Invalid Telegram message identity");
        if (typeof d.source !== "string" || (d.source && !d.source.startsWith("file:///"))) throw Error("Telegram media must be a local file");
        if (d.parts !== undefined) {
            if (!Array.isArray(d.parts) || d.parts.length > 4096) throw Error("Invalid Telegram text parts");
            for (const part of d.parts) {
                if (!part || !["text", "emoji"].includes(part.kind) || typeof part.text !== "string") throw Error("Invalid Telegram text part");
                if (part.source && (typeof part.source !== "string" || !part.source.startsWith("file:///"))) throw Error("Emoji media must be local");
            }
        }
        if (d.duration !== undefined && (typeof d.duration !== "number" || !isFinite(d.duration) || d.duration < 0 || d.duration > 86400)) throw Error("Invalid voice duration");
    }
    if (n.type === "progress" && (typeof n.data?.value !== "number" || !isFinite(n.data.value) || n.data.value < 0 || n.data.value > 100))
        throw Error("progress needs data.value between 0 and 100");
    if (n.type === "timer" && (typeof n.data?.seconds !== "number" || !isFinite(n.data.seconds) || n.data.seconds < 0 || n.data.seconds > 604800))
        throw Error("timer needs data.seconds between 0 and 604800");
}

// Apply at presentation/invocation time so config reloads also affect existing cards.
function callerBlocked(notice, config) {
    var identities = [notice.app, notice.desktopEntry].filter(value => typeof value === "string")
        .map(value => value.trim().toLowerCase());
    var blocked = (config.actions?.blacklist || []).some(caller => identities.includes(caller.trim().toLowerCase()));
    return blocked;
}

function actionsFor(notice, config) {
    return notice.archived || callerBlocked(notice, config) ? [] : (notice.actions || []);
}

// Button layout excludes unnamed actions, but invocation keeps sender indexes.
function actionButtonsFor(notice, config) {
    return actionsFor(notice, config)
        .map((action, index) => Object.assign({}, action, {actionIndex: index}))
        .filter(action => String(action.label || "").trim().length > 0);
}

function groupKey(n) {
    return JSON.stringify([n.desktopEntry || n.app || "Notification", n.group || n.senderId || ""]);
}

// Oldest to newest input; preserve every original notice and its native actions.
function grouped(notices, config, selectedId) {
    var groups = [];
    var last = {};
    var ordered = notices.slice().sort((a, b) => a.received - b.received);
    for (var notice of ordered) {
        var key = groupKey(notice);
        var group = config.grouping.enabled ? last[key] : null;
        var window = config.grouping.windowSeconds * 1000;
        if (!group || (window && notice.received - group.latest.received > window)) {
            group = {key: key, id: notice.id, generation: notice.generation, members: [], latest: notice};
            groups.push(group);
            last[key] = group;
        }
        group.members.push(notice);
        group.latest = notice;
    }
    return groups.sort((a, b) => b.latest.received - a.latest.received).map(group => {
        var representative = group.members.find(n => n.id === selectedId) || group.latest;
        return Object.assign({}, representative, {groupId: group.id, groupGeneration: group.generation, groupKey: group.key,
            groupCount: group.members.length, memberIds: group.members.map(n => n.id)});
    });
}

function visibleGroups(notices, config, selectedId) {
    var groups = grouped(notices, config, selectedId);
    var visible = groups.slice(0, Math.min(4, config.maxVisible));
    var selected = groups.find(group => group.memberIds.includes(selectedId));
    if (selected && !visible.includes(selected)) visible[visible.length - 1] = selected;
    return visible;
}

function filterHistory(notices, query) {
    const terms = String(query || "").trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!terms.length) return notices;
    return notices.filter(notice => {
        const text = [notice.title, notice.body, notice.app, notice.desktopEntry, notice.group]
            .map(value => String(value || "")).join("\n").toLowerCase();
        return terms.every(term => text.includes(term));
    });
}

function historyGroups(notices, enabled) {
    var groups = [];
    var lookup = {};
    for (var notice of notices) {
        var key = enabled ? groupKey(notice) : notice.historyKey;
        if (!lookup[key]) {
            lookup[key] = {key: key, app: notice.app || "Notification", label: notice.group || "", notices: []};
            groups.push(lookup[key]);
        }
        lookup[key].notices.push(notice);
    }
    return groups;
}

// Keep sender text literal; add breathing room only at list/prose boundaries.
function spacedBody(body) {
    const lines = String(body || '').split('\n');
    const list = line => /^\s*(?:[-*•✓✔]\s+|\d+[.)]\s+)/.test(line);
    const result = [];
    for (let i = 0; i < lines.length; ++i) {
        if (i && lines[i].trim() && lines[i-1].trim() && list(lines[i]) !== list(lines[i-1]))
            result.push('');
        result.push(lines[i]);
    }
    return result.join('\n');
}
function urgencyColors(urgency, theme) {
    if (urgency === 'critical') return {title:theme.critical, body:theme.text, artwork:theme.text};
    if (urgency === 'low') return {title:theme.muted, body:theme.muted, artwork:theme.muted};
    return {title:theme.text, body:theme.muted, artwork:theme.text};
}

// Telegram owns the mapping from its notification to the originating chat/message.
function telegramNotice(notice) {
    return [notice.app,notice.desktopEntry].some(value=>
        ['telegram','telegram desktop','org.telegram.desktop','telegram via notjik'].includes(String(value || '').toLowerCase()));
}
// Match compositor application IDs, including Telegram's per-account desktop IDs.
function telegramWindow(appId) {
    return /^(telegram|telegram-desktop|org\.telegram\.desktop)(?:$|[._-])/i.test(String(appId || ""));
}

function suppressForFocus(notice, appId) {
    return telegramWindow(appId) && (telegramNotice(notice) || notice.type === "telegram");
}

function telegramChatAction(notice) {
    return !notice.archived && telegramNotice(notice) && String(notice.id || '').startsWith('dbus-')
        ? (notice.actions || []).findIndex(action=>action.identifier==='default') : -1;
}

// Commands come only from local configuration, never notification hints.
function nativeActionHooks(notice, action, config) {
    if (notice.archived || callerBlocked(notice, config)) return [];
    const identities = [notice.app, notice.desktopEntry].filter(v => typeof v === "string").map(v => v.trim().toLowerCase());
    return (config.actions?.nativeHooks || []).filter(h => identities.includes(h.caller.trim().toLowerCase())
        && h.identifier === action.identifier).map(h => h.command);
}

// Ordered asset catalog; indices are persisted with notification history.
var robotFiles = [
    "robot-01-bronze-age.png",
    "robot-02-greek.png",
    "robot-03-roman.png",
    "robot-04-astronomer.png",
    "robot-05-renaissance.png",
    "robot-06-porcelain.png",
    "robot-07-clockwork.png",
    "robot-08-steam.png",
    "robot-09-art-nouveau.png",
    "robot-10-telegraph.png",
    "robot-11-silent-cinema.png",
    "robot-12-art-deco.png",
    "robot-13-dieselpunk.png",
    "robot-14-bakelite.png",
    "robot-15-streamliner.png",
    "robot-16-expedition.png",
    "robot-17-riveter.png",
    "robot-18-vacuum-tube.png",
    "robot-19-submarine.png",
    "robot-20-railway.png",
    "robot-21-tin-toy.png",
    "robot-22-atomic.png",
    "robot-23-raygun.png",
    "robot-24-diner.png",
    "robot-25-sputnik.png",
    "robot-26-lunar.png",
    "robot-27-space-age.png",
    "robot-28-mod.png",
    "robot-29-mainframe.png",
    "robot-30-psychedelic.png",
    "robot-31-desert.png",
    "robot-32-cassette.png",
    "robot-33-orbital.png",
    "robot-34-botanical.png",
    "robot-35-ceramic.png",
    "robot-36-analog-synth.png",
    "robot-37-crt.png",
    "robot-38-industrial-arm.png",
    "robot-39-cassette-walkman.png",
    "robot-40-arcade.png",
    "robot-41-rescue.png",
    "robot-42-orbital-shuttle.png",
    "robot-43-beige-computer.png",
    "robot-44-translucent.png",
    "robot-45-mecha-civilian.png",
    "robot-46-portable-console.png",
    "robot-47-y2k.png",
    "robot-48-rover.png",
    "robot-49-humanoid.png",
    "robot-50-solar.png",
    "robot-51-maker.png",
    "robot-52-medical.png",
    "robot-53-underwater.png",
    "robot-54-delivery.png",
    "robot-55-soft-robot.png",
    "robot-56-recycled.png",
    "robot-57-solarpunk.png",
    "robot-58-arctic-future.png",
    "robot-59-interstellar.png",
    "robot-60-monastic.png",
    "robot-61-biomorphic.png",
    "robot-62-crystalline.png",
    "robot-63-ancient-future.png",
    "robot-64-cosmic.png",
    "robot-65-alert-bronze.png",
    "robot-66-alert-clockwork.png",
    "robot-67-alert-art-deco.png",
    "robot-68-alert-atomic.png",
    "robot-69-alert-lunar.png",
    "robot-70-alert-orbital.png",
    "robot-71-alert-crt.png",
    "robot-72-alert-translucent.png",
    "robot-73-alert-medical.png",
    "robot-74-alert-solarpunk.png",
    "robot-75-alert-archive.png",
    "robot-76-alert-cosmic.png",
    "robot-77-dream-bronze.png",
    "robot-78-dream-clockwork.png",
    "robot-79-dream-art-deco.png",
    "robot-80-dream-atomic.png",
    "robot-81-dream-lunar.png",
    "robot-82-dream-orbital.png",
    "robot-83-dream-crt.png",
    "robot-84-dream-translucent.png",
    "robot-85-dream-medical.png",
    "robot-86-dream-solarpunk.png",
    "robot-87-dream-archive.png",
    "robot-88-dream-cosmic.png"
];

// Indices 0-63 remain stable for existing normal-urgency history.
function robotPool(notice) {
    if (notice.urgency === "critical") return {start: 64, count: 12};
    if (notice.urgency === "low") return {start: 76, count: 12};
    return {start: 0, count: 64};
}

function validRobotIndex(value, notice) {
    var pool = robotPool(notice || {});
    return Number.isInteger(value) && value >= pool.start && value < pool.start + pool.count;
}

function assignRobot(notice, previous) {
    var pool = robotPool(notice);
    notice.robotIndex = previous && validRobotIndex(previous.robotIndex, notice)
        ? previous.robotIndex : pool.start + Math.floor(Math.random() * pool.count);
}

function robotArtwork(notice) {
    var pool = robotPool(notice);
    var index = notice.robotIndex;
    if (!validRobotIndex(index, notice)) {
        // Legacy history receives a stable draw from its urgency's collection,
        // without rewriting archives or rerolling when a card is recreated.
        var seed = String(notice.id || "") + ":" + String(notice.received || 0);
        var hash = 2166136261;
        for (var i = 0; i < seed.length; ++i) hash = Math.imul(hash ^ seed.charCodeAt(i), 16777619);
        index = pool.start + (hash >>> 0) % pool.count;
    }
    return "icons/robots/" + robotFiles[index];
}
