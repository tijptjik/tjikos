//@ pragma RespectSystemStyle
import QtQuick
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Services.Notifications
import "Logic.js" as Logic

ShellRoot {
    id: root
    property var config: ({ placement: {monitors: "primary", primaryMonitor: "", edge: "b", position: "c", offsetX: 0, offsetY: -24, margin: 16, perMonitor: {}},
        timeouts: {low: 5, normal: 8, critical: 0}, style: {}, components: {}, maxVisible: 5, maxQueued: 100, pauseOnHover: true })
    property string lockState: "unknown"
    readonly property bool locked: lockState !== "unlocked"
    readonly property bool conceal: locked && config.privacy?.lockScreen !== "show"
    property bool drawerOpen: false
    property string drawerMonitor: ""
    property var avatarCache: ({byChat: {}, byTitle: {}})
    property var historyItems: []
    property string historyQuery: ""
    readonly property bool historyFiltering: historyQuery.trim().length > 0
    readonly property var filteredHistoryItems: Logic.filterHistory(historyItems, historyQuery)
    property var searchExpandedGroups: ({})
    onHistoryQueryChanged: searchExpandedGroups = ({})
    onFilteredHistoryItemsChanged: {
        if (!filteredHistoryItems.some(n => n.historyKey === historySelectedKey)) historySelectedKey = "";
    }
    property var expandedGroups: ({})
    property string historySelectedKey: ""
    property string selectedId: ""
    property bool keyboardMode: false
    property string keyboardMonitor: ""
    readonly property var historyGroups: Logic.historyGroups(filteredHistoryItems, config.grouping?.enabled !== false)
    readonly property var historyRows: makeHistoryRows()
    onScreensChanged: {
        if (!screens.some(s => s.name === keyboardMonitor)) { keyboardMode = false; selectedId = ""; }
        if (!screens.some(s => s.name === drawerMonitor)) drawerMonitor = screens[0]?.name || "";
    }
    onLockStateChanged: {
        if (lockState !== "unlocked") { drawerOpen = false; keyboardMode = false; selectedId = ""; historySelectedKey = ""; }
    }
    readonly property string focusedAppId: ToplevelManager.activeToplevel?.appId || ""
    onFocusedAppIdChanged: {
        for (const notice of notices.slice())
            if (Logic.suppressForFocus(notice, focusedAppId)) close(notice.id, false);
    }
    property var telegramDelivery: ({nativeReceived: 0, nativeFocusSuppressed: 0, nativeBridgeSuppressed: 0, lastNativeAt: 0})
    property double telegramBridgeUntil: 0
    readonly property bool telegramBridgeActive: telegramBridgeUntil > now
    readonly property alias telegramPlayer: telegramPlayback
    TelegramPlayback { id: telegramPlayback; locked: root.locked || root.paused }
    readonly property alias telegramRecorder: telegramRecording
    TelegramRecorder { id: telegramRecording; controller: root; locked: root.locked || root.paused }
    property bool ready: false
    property bool paused: false
    property string backendError: ""
    property var notices: []
    property var nativeNotices: ({})
    property var hovered: ({})
    // IDs remain stable while positions and sender payloads change.
    property var floatingIds: []
    property var floatingPositions: ({})
    property var floatingStates: ({})
    function saveFloatingState(id, state) { floatingStates = Object.assign({}, floatingStates, {[id]:state}); }
    property string draggingId: ""
    property string dragPreviewId: ""
    property var dragStack: []
    property var dragOrigin: null
    function isFloating(id) { return floatingIds.includes(id); }
    function beginDrag(id, screen, x, y, width, height) {
        if (locked || draggingId || !find(id)) return false;
        Hyprland.refreshMonitors();
        dragStack = visibleNotices.slice();
        dragOrigin = {x:x, y:y, width:width, height:height, screen:screen};
        draggingId = id;
        dragPreviewId = id;
        floatingPositions = Object.assign({}, floatingPositions, {[id]:dragOrigin});
        if (!isFloating(id)) floatingIds = floatingIds.concat([id]);
        return true;
    }
    function moveDrag(id, dx, dy) {
        if (draggingId !== id || !dragOrigin) return;
        const x = dragOrigin.x + dx, y = dragOrigin.y + dy;
        const screen = Quickshell.screens.find(s => x + dragOrigin.width/2 >= s.x && x + dragOrigin.width/2 < s.x+s.width
            && y + Math.min(30, dragOrigin.height/2) >= s.y && y + Math.min(30, dragOrigin.height/2) < s.y+s.height);
        floatingPositions = Object.assign({}, floatingPositions, {[id]:Object.assign({}, dragOrigin,
            {x:x, y:y, screen:screen?.name || floatingPositions[id].screen})});
    }
    function endDrag(id) {
        if (draggingId !== id) return;
        const position = floatingPositions[id];
        const screen = Quickshell.screens.find(s => s.name === position?.screen) || Quickshell.screens[0];
        if (position && screen) floatingPositions = Object.assign({}, floatingPositions, {[id]:Object.assign({}, position,
            {screen:screen.name, x:screen.x + Math.max(0, Math.min(screen.width-position.width, position.x-screen.x)),
             y:screen.y + Math.max(0, Math.min(screen.height-position.height, position.y-screen.y))})});
        if (position && screen) {
            const monitor = Hyprland.monitorFor(screen);
            const special = monitor?.lastIpcObject.specialWorkspace;
            const workspace = special?.id ? special.name : monitor?.activeWorkspace?.name;
            floatingPositions = Object.assign({}, floatingPositions, {[id]:Object.assign({}, floatingPositions[id], {workspace:workspace || ""})});
        }
        draggingId = "";
        dragOrigin = null;
        dragStack = [];
    }
    property var pendingLogs: []
    property int sequence: 0
    property int noticeGeneration: 0
    property double now: Date.now()
    property double lastTick: Date.now()
    property string session: String(Date.now())
    readonly property var theme: config.style
    readonly property string primaryMonitor: Quickshell.screens.find(s => s.name === config.placement.primaryMonitor)?.name || Quickshell.screens[0]?.name || ""
    readonly property var screens: Quickshell.screens.filter((screen, index) => Logic.selected(config, screen, index, primaryMonitor))
    readonly property var visibleNotices: ready && !paused && !conceal && !drawerOpen ? (draggingId ? dragStack : Logic.visibleGroups(notices.filter(n => !isFloating(n.id)), config, telegramRecording.holdsNotice ? telegramRecording.noticeId : (keyboardMode ? selectedId : ""))) : []

    function snapshot(n) {
        return {id: n.id, senderId: n.senderId || "", updatedAt: n.updatedAt || n.received, title: n.title, body: n.body, app: n.app, urgency: n.urgency, type: n.type,
            icon: n.icon, image: n.image, robotIndex: n.robotIndex, fishIndex: n.fishIndex, fishName: n.fishName, data: n.data, transient: n.transient, group: n.group || "", desktopEntry: n.desktopEntry || "",
            actions: Logic.actionsFor(n, config).map(a => ({label: a.label})), received: n.received};
    }

    function log(event, n, extra) {
        const entry = Object.assign({event: event, session: session, notice: snapshot(n)}, extra || {});
        if (backend.running && ready) backend.write(JSON.stringify(entry) + "\n");
        else pendingLogs = pendingLogs.concat([entry]).slice(-1000);
    }

    function find(id) { return notices.find(n => n.id === id); }

    function remove(id, reason) {
        const notice = find(id);
        if (!notice) return;
        endDrag(id);
        if (dragPreviewId === id) dragPreviewId = "";
        floatingIds = floatingIds.filter(value => value !== id);
        const positions = Object.assign({}, floatingPositions);
        delete positions[id];
        floatingPositions = positions;
        const states = Object.assign({}, floatingStates);
        delete states[id];
        floatingStates = states;
        telegramPlayback.stopIf(id);
        telegramRecording.discardIf(id);
        log(reason, notice);
        delete nativeNotices[id];
        notices = notices.filter(n => n.id !== id);
        if (selectedId === id) {
            selectedId = notices.length ? notices[notices.length - 1].id : "";
            if (!selectedId) keyboardMode = false;
        }
        const activeHover = Object.assign({}, hovered);
        for (const key of Object.keys(activeHover)) if (key.endsWith("/" + id)) delete activeHover[key];
        hovered = activeHover;
    }

    function dismissReadIds() {
        if (!ready || paused || locked) return [];
        return (screens.length ? visibleNotices.map(n => n.id) : [])
            .concat(floatingIds.filter(id => id !== draggingId));
    }

    function dismissNotice(id, safeRead, visibleIds) {
        const notice = find(id);
        const shown = visibleIds || dismissReadIds();
        if (notice && !notice.archived && shown.includes(id))
            log(safeRead ? "user-safe-read" : "user-dismissed", notice);
        close(id, false);
    }

    function close(id, expired) {
        const n = nativeNotices[id];
        if (n) { if (expired) n.expire(); else n.dismiss(); }
        else remove(id, expired ? "expired" : "dismissed");
    }

    function put(notice) {
        if (Logic.suppressForFocus(notice, focusedAppId)) {
            log("suppressed", notice);
            close(notice.id, false);
            return true;
        }
        const index = notices.findIndex(n => n.id === notice.id);
        Logic.assignRobot(notice, index >= 0 ? notices[index] : null);
        Logic.assignFish(notice, index >= 0 ? notices[index] : null);
        notice.generation = index >= 0 ? notices[index].generation : ++noticeGeneration;
        if (index >= 0) {
            const next = notices.slice();
            next[index] = notice;
            notices = next;
            log("updated", notice);
        } else {
            while (notices.length >= config.maxQueued) {
                const oldestTimed = notices.find(n => n.remaining > 0 && !isFloating(n.id) && !(telegramRecording.holdsNotice && telegramRecording.noticeId === n.id));
                if (!oldestTimed) {
                    log("overflow", notice);
                    return false;
                }
                close(oldestTimed.id, true);
            }
            notices = notices.concat([notice]);
            log("received", notice);
        }
        return true;
    }

    function nativeRecord(n) {
        const urgency = ["low", "normal", "critical"][n.urgency] || "normal";
        const value = n.hints.value;
        const progress = typeof value === "number";
        return {id: "dbus-" + n.id, title: n.summary, body: n.body, app: n.appName, desktopEntry: n.desktopEntry, group: "",
            urgency: urgency, type: progress ? "progress" : "standard", icon: n.appIcon, image: n.image,
            data: progress ? {value: Math.max(0, Math.min(100, value))} : {},
            actions: n.actions.map(a => ({label: a.text || (a.identifier === "default" ? "Open" : a.identifier), identifier: a.identifier})),
            resident: n.resident, transient: n.transient, received: Date.now(), requestedTimeout: n.expireTimeout,
            remaining: Logic.timeout(n.expireTimeout, urgency, config)};
    }

    NotificationServer {
        id: server
        keepOnReload: false
        bodySupported: true
        bodyMarkupSupported: false
        bodyHyperlinksSupported: false
        bodyImagesSupported: false
        imageSupported: true
        actionsSupported: true
        actionIconsSupported: false
        persistenceSupported: true
        inlineReplySupported: false
        onNotification: n => {
            n.tracked = true;
            const telegram = Logic.telegramNotice({app:n.appName, desktopEntry:n.desktopEntry});
            const suppressed = Logic.suppressForFocus({app:n.appName, desktopEntry:n.desktopEntry}, root.focusedAppId);
            const bridgeSuppressed = !suppressed && telegram && n.hints.category === "im.received" && root.telegramBridgeActive && !root.locked && !root.paused;
            if (telegram) root.telegramDelivery = {
                nativeReceived: root.telegramDelivery.nativeReceived + 1,
                nativeFocusSuppressed: root.telegramDelivery.nativeFocusSuppressed + (suppressed ? 1 : 0),
                nativeBridgeSuppressed: root.telegramDelivery.nativeBridgeSuppressed + (bridgeSuppressed ? 1 : 0),
                lastNativeAt: Date.now()
            };
            if (suppressed || bridgeSuppressed) {
                root.log("suppressed", root.nativeRecord(n));
                n.expire(); return;
            }
            // The bridge owns chat alerts only while its online delivery lease is
            // healthy. Calls and other Desktop categories retain their own route.
            const id = "dbus-" + n.id;
            root.nativeNotices[id] = n;
            n.closed.connect(reason => root.remove(id, reason === NotificationCloseReason.Expired ? "expired"
                : reason === NotificationCloseReason.Dismissed ? "dismissed" : "closed"));
            const watchedActions = [];
            const refresh = function() {
                if (root.nativeNotices[id]) {
                    watchActions();
                    root.put(root.nativeRecord(n));
                }
            };
            const schedule = function() { Qt.callLater(refresh); };
            const watchActions = function() {
                for (const action of n.actions) if (!watchedActions.includes(action)) {
                    watchedActions.push(action);
                    action.textChanged.connect(schedule);
                }
            };
            watchActions();
            n.summaryChanged.connect(schedule);
            n.bodyChanged.connect(schedule);
            n.appNameChanged.connect(schedule);
            n.appIconChanged.connect(schedule);
            n.imageChanged.connect(schedule);
            n.urgencyChanged.connect(schedule);
            n.expireTimeoutChanged.connect(schedule);
            n.actionsChanged.connect(schedule);
            n.hintsChanged.connect(schedule);
            if (!root.put(root.nativeRecord(n))) {
                n.dismiss();
                delete root.nativeNotices[id];
            }
        }
    }

    function send(n) {
        Logic.validateNotice(n);
        const kind = n.type || "standard";
        if (!["standard", "progress", "status", "timer", "telegram", "results-table"].includes(kind) && !config.components[kind]) throw Error("Unknown component: " + kind);
        if (n.replace && (!find(n.replace) || nativeNotices[n.replace])) throw Error("--replace requires an existing local notification ID");
        const app = n.app || "notjikctl";
        const senderId = n.id || "";
        const id = senderId ? "named-" + encodeURIComponent(app) + "/" + encodeURIComponent(senderId) : n.replace || "local-" + (++sequence);
        const existing = find(id);
        const timestamp = Date.now();
        const lifetime = senderId && n.timeout === undefined ? 0 : n.timeout;
        const urgency = n.urgency || "normal";
        const accepted = put({id: id, title: n.title, body: n.body || "", app: app, senderId: senderId, urgency: urgency,
            type: kind, data: n.data || {}, icon: n.icon || "", image: n.image || "",
            actions: n.actions || [], resident: !!n.resident, transient: !!n.transient, group: n.group || "",
            received: existing ? existing.received : timestamp, updatedAt: timestamp, remaining: Logic.timeout(lifetime, urgency, config)});
        if (!accepted) throw Error("Queue is full of persistent notices; dismiss one before sending");
        return id;
    }

    function invoke(id, index) {
        if (locked) throw Error("Notification actions are disabled while locked");
        const notice = find(id);
        if (!notice || !Number.isInteger(index) || index < 0 || index >= notice.actions.length) throw Error("Action not found");
        if (!Logic.actionsFor(notice, config).length) throw Error("Notification actions are disabled for this caller");
        const action = notice.actions[index];
        log("action", notice, {action: action.label});
        const native = nativeNotices[id];
        if (native) {
            const handle = native.actions.find(a => a.identifier === action.identifier);
            if (!handle) throw Error("Action no longer available");
            const hooks = Logic.nativeActionHooks(notice, action, config);
            handle.invoke();
            if (Logic.telegramChatAction(notice) === index) finishTelegramOpen(id);
            for (const command of hooks) Quickshell.execDetached(command);
        } else {
            Quickshell.execDetached(action.command);
            if (!notice.resident) close(id, false);
        }
    }

    function finishTelegramOpen(id) {
        const windows = Hyprland.toplevels.values.filter(window =>
            Logic.telegramWindow(window.lastIpcObject.class));
        const target = windows.find(window => !window.lastIpcObject.title?.includes("Media viewer")) || windows[0];
        if (target) {
            const address = "address:" + target.lastIpcObject.address;
            if (Hyprland.usingLua)
                Quickshell.execDetached(["hyprctl", "eval", "local w=hl.get_window(" + JSON.stringify(address)
                    + "); if w then hl.dispatch(hl.dsp.focus({window=w})) end"]);
            else Hyprland.dispatch("focuswindow " + address);
        }
        close(id, false);
    }

    function openTelegramChat(id) {
        if(locked)throw Error("Notification actions are disabled while locked");
        const notice=find(id),native=nativeNotices[id];
        if(!notice || !native || Logic.telegramChatAction(notice)<0)throw Error("Telegram chat action unavailable");
        const action=native.actions.find(a=>a.identifier==='default');
        if(!action)throw Error("Telegram chat action unavailable");
        log("action",notice,{action:"Open Telegram chat"});
        action.invoke();
        finishTelegramOpen(id);
    }

    function setHover(screen, id, value) {
        const next = Object.assign({}, hovered);
        if (value) next[screen + "/" + id] = true;
        else delete next[screen + "/" + id];
        hovered = next;
    }

    function targetMonitor() {
        const focused = Hyprland.focusedMonitor?.name;
        return screens.some(s => s.name === focused) ? focused : (screens[0]?.name || "");
    }

    function makeHistoryRows() {
        const rows = [];
        for (const group of historyGroups) {
            rows.push({kind: "group", key: group.key, app: group.app, label: group.label, count: group.notices.length});
            if (historyGroupExpanded(group.key)) for (const notice of group.notices)
                rows.push({kind: "notice", key: notice.historyKey, notice: notice});
        }
        return rows;
    }

    function historyGroupExpanded(key) {
        return historyFiltering ? searchExpandedGroups[key] !== false : !!expandedGroups[key];
    }

    function toggleGroup(key) {
        if (historyFiltering) searchExpandedGroups = Object.assign({}, searchExpandedGroups, {[key]: !historyGroupExpanded(key)});
        else expandedGroups = Object.assign({}, expandedGroups, {[key]: !expandedGroups[key]});
    }

    function selectHistory(key) {
        historySelectedKey = key;
        const notice = historyItems.find(n => n.historyKey === key);
        if (notice) {
            const group = historyGroups.find(g => g.notices.some(n => n.historyKey === key));
            if (group && !historyGroupExpanded(group.key)) toggleGroup(group.key);
        }
    }

    function historyCard(snapshot) {
        const active = snapshot.session === session ? find(snapshot.id) : null;
        return Object.assign({data: {}, actions: [], icon: "", image: "", type: "standard", urgency: "normal", remaining: 0},
            snapshot, active || {}, {archived: !active, historyKey: snapshot.historyKey, loggedAt: snapshot.loggedAt,
                lastEvent: active ? "active" : snapshot.lastEvent});
    }

    function setDrawer(opened) {
        if (opened && locked) throw Error("History is unavailable while locked");
        drawerOpen = opened;
        keyboardMode = false;
        selectedId = "";
        if (opened) {
            historyQuery = "";
            drawerMonitor = targetMonitor();
            if (backend.running) backend.write(JSON.stringify({op: "history"}) + "\n");
            historySelectedKey = "";
            expandedGroups = ({});
        }
    }

    function navigate(direction) {
        if (locked) throw Error("Notification navigation is unavailable while locked");
        if (drawerOpen || !notices.length) {
            if (!drawerOpen) setDrawer(true);
            if (!filteredHistoryItems.length) return;
            let index = filteredHistoryItems.findIndex(n => n.historyKey === historySelectedKey);
            if (index < 0) index = 0;
            else index = (index + (direction === "previous" ? 1 : -1) + filteredHistoryItems.length) % filteredHistoryItems.length;
            selectHistory(filteredHistoryItems[index].historyKey);
            return;
        }
        const ordered = notices.slice().sort((a, b) => b.received - a.received);
        let index = ordered.findIndex(n => n.id === selectedId);
        if (index < 0) index = direction === "previous" && ordered.length > 1 ? 1 : 0;
        else index = (index + (direction === "previous" ? 1 : -1) + ordered.length) % ordered.length;
        selectedId = ordered[index].id;
        keyboardMode = true;
        keyboardMonitor = floatingPositions[selectedId]?.screen || targetMonitor();
    }

    function unfocus() { keyboardMode = false; selectedId = ""; }

    function activateSelected() {
        const active = drawerOpen ? historyCard(historyItems.find(n => n.historyKey === historySelectedKey) || {}) : find(selectedId);
        if (active && Logic.actionsFor(active, config).length) invoke(active.id, 0);
    }

    function dismissSelected() {
        const active = drawerOpen ? historyCard(historyItems.find(n => n.historyKey === historySelectedKey) || {}) : find(selectedId);
        if (active && !active.archived && find(active.id)) dismissNotice(active.id);
    }

    function setPaused(value) {
        if (!backend.running) throw Error("Cannot save notification mode while the helper is stopped");
        backend.write(JSON.stringify({op: "pausePreference", paused: value}) + "\n");
        paused = value;
    }

    function handle(request) {
        if (!ready) throw Error("notjik configuration is not ready: " + backendError);
        switch (request.op) {
        case "history":
            if (!["show", "hide", "toggle"].includes(request.mode)) throw Error("Invalid history mode");
            setDrawer(request.mode === "show" || (request.mode === "toggle" && !drawerOpen)); break;
        case "navigate":
            if (!["previous", "next"].includes(request.direction)) throw Error("Invalid navigation direction");
            navigate(request.direction); break;
        case "unfocus": unfocus(); break;
        case "telegramBridge":
            telegramBridgeUntil = request.active === true ? Date.now() + 12000 : 0;
            break;
        case "send": return {ok: true, id: send(request.notice)};
        case "pause": setPaused(true); break;
        case "resume": setPaused(false); break;
        case "toggle": setPaused(!paused); break;
        case "telegramRemove":
            if (find(request.id)?.app === "Telegram via Notjik") remove(request.id, "sender-closed");
            break;
        case "dismiss": {
            if (request.id !== "all" && !find(request.id)) throw Error("Notification not found");
            // Freeze eligibility before removing cards reveals queued/grouped ones.
            const shown = dismissReadIds();
            for (const n of notices.slice()) if (request.id === "all" || request.id === n.id) dismissNotice(n.id, false, shown);
            break;
        }
        case "openTelegramChat": openTelegramChat(request.id); break;
        case "action": invoke(request.id, request.index); break;
        case "reload": backend.running = false; backend.running = true; break;
        case "list": return {ok: true, notices: notices.map(snapshot)};
        case "monitors": return {ok: true, monitors: Quickshell.screens.map((s, i) => ({index: i + 1, name: s.name,
            width: s.width, height: s.height, scale: s.devicePixelRatio, selected: Logic.selected(config, s, i, primaryMonitor)}))};
        case "status": break;
        default: throw Error("Unknown operation");
        }
        return {ok: true, paused: paused, queued: notices.length, visible: visibleNotices.length, floating: floatingIds.length, pinned: floatingIds.length,
            telegramDelivery: telegramDelivery, telegramBridgeActive: telegramBridgeActive, focusedAppId: focusedAppId, groupingEnabled: config.grouping.enabled, theme: config.theme, lockState: lockState, drawerOpen: drawerOpen, historyCount: historyItems.length, selectedId: selectedId, historySelectedKey: historySelectedKey, keyboardMode: keyboardMode, historyError: backendError, monitors: screens.map(s => s.name)};
    }

    FileView {
        id: ipcRequestFile
        blockLoading: true
        printErrors: false
    }

    IpcHandler {
        target: "notjik"
        function requestFile(path: string): string {
            try {
                ipcRequestFile.path = path;
                const payload = ipcRequestFile.text();
                return request(payload);
            } catch (error) { return JSON.stringify({ok: false, error: "Cannot read private IPC request"}); }
            finally { ipcRequestFile.path = ""; }
        }
        function request(payload: string): string {
            try {
                if (payload.length > 131072) throw Error("Request exceeds 128 KiB");
                return JSON.stringify(root.handle(JSON.parse(payload)));
            } catch (error) { return JSON.stringify({ok: false, error: String(error)}); }
        }
    }

    Process {
        id: backend
        command: ["python3", "-B", Quickshell.shellDir + "/../notjik.py"]
        running: true
        stdinEnabled: true
        stderr: SplitParser { onRead: line => console.warn("notjik helper:", line) }
        stdout: SplitParser {
            onRead: line => {
                try {
                    const response = JSON.parse(line);
                    if (typeof response.paused === "boolean") {
                        root.paused = response.paused;
                    } else if (response.config) {
                        root.config = response.config;
                        if (!root.ready) for (const n of root.notices) n.remaining = Logic.timeout(n.requestedTimeout, n.urgency, root.config);
                        root.ready = true;
                        root.backendError = "";
                        for (const entry of root.pendingLogs) backend.write(JSON.stringify(entry) + "\n");
                        root.pendingLogs = [];
                    } else if (response.lockState) {
                        root.lockState = response.lockState;
                    } else if (response.history) {
                        root.avatarCache = response.avatars || {byChat: {}, byTitle: {}};
                        root.historyItems = response.history;
                        if (root.historySelectedKey && !root.historyItems.some(n => n.historyKey === root.historySelectedKey))
                            root.historySelectedKey = "";
                    } else if (response.error) { root.backendError = response.error; console.warn(response.error); }
                } catch (error) { console.warn("notjik backend:", String(error)); }
            }
        }
        onExited: (code, status) => { root.lockState = "unknown"; root.backendError = "History/config helper exited (" + code + ")"; retry.restart(); }
    }
    Timer { id: retry; interval: 2000; onTriggered: { if (!backend.running) backend.running = true; } }

    Timer {
        interval: 100
        running: true
        repeat: true
        onTriggered: {
            const time = Date.now();
            const elapsed = Math.min(1000, Math.max(0, time - root.lastTick));
            root.now = time;
            root.lastTick = time;
            if (!root.ready || root.paused || root.locked || root.drawerOpen || root.screens.length === 0) return;
            for (const n of root.notices.slice()) {
                if (root.isFloating(n.id)) continue;
                if (telegramRecording.holdsNotice && telegramRecording.noticeId === n.id) continue;
                if (telegramPlayback.noticeId === n.id && telegramPlayback.playing) continue;
                if (n.remaining <= 0 || (root.keyboardMode && n.id === root.selectedId)) continue;
                if (root.config.pauseOnHover && Object.keys(root.hovered).some(key => key.endsWith("/" + n.id))) continue;
                n.remaining -= elapsed;
                if (n.remaining <= 0) root.close(n.id, true);
            }
        }
    }

    HistoryDrawer { controller: root }

    Variants {
        id: dragPreviews
        model: Quickshell.screens
        DragPreview { required property var modelData; screen: modelData; controller: root }
    }

    Variants {
        id: floatingPanels
        model: root.floatingIds
        FloatingPanel { required property string modelData; noticeId: modelData; controller: root }
    }

    Variants {
        model: root.screens
        NotificationPanel { required property var modelData; screen: modelData; controller: root }
    }
}
