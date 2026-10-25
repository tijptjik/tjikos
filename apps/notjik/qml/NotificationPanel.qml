import QtQuick
import QtQuick.Window
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland
import "Logic.js" as Logic

PanelWindow {
    id: panel
    required property var controller
    readonly property var theme: controller.theme
    readonly property var placement: Logic.placement(controller.config, screen, Quickshell.screens.indexOf(screen))
    readonly property var coordinates: Logic.geometry(placement, screen.width, screen.height, width, layoutHeight, screen.devicePixelRatio)
    readonly property real layoutHeight: Math.max(1, (screen.height - 2 * placement.margin) * (controller.config.stack?.heightPercent || 75) / 100)
    readonly property real outroPadding: Math.max(0, Math.min(32, screen.height - coordinates.y - layoutHeight))
    readonly property bool redacted: controller.ready && controller.locked && controller.config.privacy.lockScreen === "redact" && controller.notices.length > 0
    readonly property bool ownsKeyboard: !controller.isFloating(controller.selectedId) && controller.keyboardMode && controller.keyboardMonitor === screen.name && !controller.locked
    readonly property real stackTop: placement.edge === "t" || ((placement.edge === "l" || placement.edge === "r") && placement.position === "l") ? 0
        : ((placement.edge === "l" || placement.edge === "r") && placement.position === "c") ? Math.max(0, (layoutHeight - stack.implicitHeight) / 2)
        : Math.max(0, layoutHeight - stack.implicitHeight)
    readonly property int animationMs: controller.config.stack?.animationMs ?? 220
    property bool motionActive: false
    property real motionSpeed: 0
    visible: controller.ready && !controller.paused && !controller.drawerOpen &&
        (redacted || (!controller.conceal && cards.count > 0))
    anchors { top: true; left: true }
    implicitWidth: Math.max(1, Math.min(theme.width || 420, screen.width - 2 * placement.margin))
    implicitHeight: layoutHeight + outroPadding
    margins { top: coordinates.y; left: coordinates.x }
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.namespace: "notjik"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: ownsKeyboard ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None
    Connections {
        target: panel.contentItem.Window.window
        function onActiveChanged() {
            if (!target.active && panel.ownsKeyboard) panel.controller.unfocus();
        }
    }
    mask: Region {
        x: 0
        y: panel.redacted ? panel.layoutHeight - 80 : panel.stackTop
        width: panel.width
        height: panel.redacted ? 80 : Math.min(panel.layoutHeight, stack.implicitHeight)
    }

    function keyFor(notice) { return JSON.stringify([notice.groupId, notice.groupGeneration]); }
    function finishRemove(key) {
        for (let i = 0; i < cards.count; ++i)
            if (cards.get(i).noticeKey === key && cards.get(i).retiring) { cards.remove(i); break; }
    }
    function advanceFlow(seconds) {
        const slots = [];
        for (let i = 0; i < cardItems.count; ++i) {
            const item = cardItems.itemAt(i);
            if (!item || !item.initialized) return;
            // Let visible departures clear without pulling their successors around.
            if (item.retiring && item.extent > 0) { motionSpeed = 0; return; }
            if (!item.retiring) slots.push(item);
        }
        const waiting = slots.filter(item => !item.started || item.extent < 1);
        if (!waiting.length) { motionSpeed = 0; motionActive = false; return; }
        if (!animationMs) {
            for (const item of waiting) { item.begin(); item.extent = 1; }
            motionSpeed = 0;
            motionActive = false;
            return;
        }
        // The first card appears at the resting edge before anything pushes it.
        if (!slots.some(item => item.started)) {
            waiting[0].firstArrival = true;
            waiting[0].begin();
            waiting[0].extent = 1;
            return;
        }
        if (slots.some(item => item.firstArrival && item.reveal < 1)) return;

        // One velocity for the entire burst. Queued cards consume distance only
        // when the preceding card has made room; there is no scheduled stagger.
        const duration = animationMs / 1000;
        const dt = Math.min(0.032, Math.max(0, seconds));
        const gap = theme.gap || 0;
        const remaining = waiting.reduce((sum, item) => sum + (item.cardHeight + gap) * (1 - item.extent), 0);
        const maxSpeed = Math.max(120, panel.width * 0.45) * 2 / duration;
        const targetSpeed = Math.min(maxSpeed, remaining * 8 / duration);
        motionSpeed += (targetSpeed - motionSpeed) * (1 - Math.exp(-dt / (duration * 0.06)));
        let distance = motionSpeed * dt;
        for (const item of waiting) {
            if (distance <= 0) break;
            item.begin();
            const cost = item.cardHeight + gap;
            const travel = Math.min(distance, cost * (1 - item.extent));
            item.extent = Math.min(1, item.extent + travel / cost);
            distance -= travel;
            if (cost * (1 - item.extent) < 0.2) item.extent = 1;
            if (item.extent < 1) break;
        }
    }
    FrameAnimation {
        running: panel.motionActive
        onTriggered: panel.advanceFlow(frameTime)
    }
    function sync() {
        // Explicitly measured, oldest-to-newest children. No virtual list height estimates.
        const incoming = controller.visibleNotices.slice().reverse();
        for (let i = 0; i < cards.count; ++i) {
            const retiring = !incoming.some(n => keyFor(n) === cards.get(i).noticeKey);
            cards.setProperty(i, "retiring", retiring);
        }
        for (let i = 0; i < incoming.length; ++i) {
            let found = -1;
            for (let j = 0; j < cards.count; ++j) if (cards.get(j).noticeKey === keyFor(incoming[i])) { found = j; break; }
            if (found < 0) cards.append({noticeKey: keyFor(incoming[i]), retiring: false});
            else cards.setProperty(found, "retiring", false);
        }
        // Keep departing slots in place until their fade completes; order active slots.
        let previous = -1;
        for (const notice of incoming) {
            let found = -1;
            for (let j = 0; j < cards.count; ++j) if (cards.get(j).noticeKey === keyFor(notice)) { found = j; break; }
            if (found < previous) { cards.move(found, previous, 1); found = previous; }
            previous = found;
        }
        motionActive = true;
        if (ownsKeyboard) Qt.callLater(() => keyReceiver.forceActiveFocus());
    }
    Component.onCompleted: sync()
    Connections { target: panel.controller; function onVisibleNoticesChanged() { panel.sync(); } }
    onOwnsKeyboardChanged: { if (ownsKeyboard) Qt.callLater(() => keyReceiver.forceActiveFocus()); }
    ListModel { id: cards }
    Shortcut { sequence: "Escape"; enabled: panel.ownsKeyboard; onActivated: panel.controller.unfocus() }
    Item {
        id: keyReceiver
        anchors.fill: parent
        Keys.onPressed: event => {
            if (!panel.ownsKeyboard) return;
            if (event.key === Qt.Key_Escape) panel.controller.unfocus();
            else if (event.key === Qt.Key_Up) panel.controller.navigate("previous");
            else if (event.key === Qt.Key_Down) panel.controller.navigate("next");
            else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) panel.controller.activateSelected();
            else if (event.key === Qt.Key_Delete || event.key === Qt.Key_Backspace) panel.controller.dismissSelected();
            else return;
            event.accepted = true;
        }
    }
    Item {
        id: notificationScene
        anchors.fill: parent
        // Departures paint behind the live stack, with room below its resting edge.
        Item { id: exitLayer; anchors.fill: parent; visible: !panel.redacted; z: 0 }
        Flickable {
            id: viewport
            z: 1
            anchors.fill: parent
            anchors.bottomMargin: panel.outroPadding
            visible: !panel.redacted
            contentWidth: width
            contentHeight: Math.max(height, stack.implicitHeight)
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            acceptedButtons: Qt.NoButton
            function showNewest() { if (!dragging && !flicking) contentY = Math.max(0, contentHeight - height); }
            onContentHeightChanged: Qt.callLater(showNewest)
            onHeightChanged: Qt.callLater(showNewest)
            Item {
                id: stack
                width: viewport.width
                y: panel.stackTop
                property int layoutRevision: 0
                implicitHeight: Math.max(0, offset(cardItems.count))
                function offset(index) {
                    const revision = layoutRevision;
                    let total = 0;
                    let previousExtent = 0;
                    for (let i = 0; i <= index && i < cardItems.count; ++i) {
                        const item = cardItems.itemAt(i);
                        if (!item) continue;
                        // A gap grows with both neighbours; queued cards reserve nothing.
                        total += (panel.theme.gap || 0) * Math.min(previousExtent, item.extent);
                        if (i === index) break;
                        total += item.height;
                        if (item.extent > 0) previousExtent = item.extent;
                    }
                    return total;
                }
                Repeater {
                    id: cardItems
                    model: cards
                    onItemAdded: stack.layoutRevision++
                    onItemRemoved: stack.layoutRevision++
                    delegate: Item {
                        id: slot
                        required property int index
                        required property string noticeKey
                        required property bool retiring
                        property real entranceAt: 0
                        property bool started: false
                        property bool firstArrival: false
                        readonly property real cardHeight: card.height
                        property var cachedNotice: null
                        readonly property var liveNotice: panel.controller.visibleNotices.find(n => panel.keyFor(n) === noticeKey)
                        property real reveal: 0
                        property real exitScale: 0.97
                        property real exitY: 0
                        property real exitDrop: 0
                        property bool initialized: false
                        property real extent: 0
                        onLiveNoticeChanged: { if (liveNotice) cachedNotice = liveNotice; }
                        function begin() {
                            if (started) return;
                            started = true;
                            entranceAt = Date.now();
                            if (!firstArrival) reveal = Math.max(reveal, 0.15);
                            entrance.start();
                        }
                        function transition() {
                            panel.motionActive = true;
                            entrance.stop();
                            departure.stop();
                            if (retiring) {
                                exitY = slot.mapToItem(exitLayer, 0, slot.height - card.height).y;
                                if (extent <= 0 || reveal <= 0) {
                                    extent = 0;
                                    Qt.callLater(() => panel.finishRemove(noticeKey));
                                } else departure.start();
                            } else {
                                exitDrop = 0;
                                if (started) entrance.start();
                            }
                        }
                        onRetiringChanged: { if (initialized) transition(); }
                        Component.onCompleted: {
                            if (liveNotice) cachedNotice = liveNotice;
                            initialized = true;
                            transition();
                        }
                        width: stack.width
                        y: stack.offset(index)
                        // The incoming face stays whole while older cards move behind it.
                        z: index
                        height: card.height * extent
                        clip: false
                        enabled: !retiring
                        // Fade the complete card; flow controls its layout independently.
                        ParallelAnimation {
                            id: entrance
                            NumberAnimation { target: slot; property: "reveal"; to: 1; duration: panel.animationMs * 0.4; easing.type: Easing.OutCubic }
                            NumberAnimation { target: slot; property: "exitScale"; to: 1; duration: panel.animationMs * 0.4; easing.type: Easing.OutCubic }
                        }
                        SequentialAnimation {
                            id: departure
                            ParallelAnimation {
                                NumberAnimation { target: slot; property: "reveal"; to: 0; duration: panel.animationMs * 0.65; easing.type: Easing.InQuad }
                                NumberAnimation { target: slot; property: "exitScale"; to: 0.9; duration: panel.animationMs * 0.65; easing.type: Easing.InQuad }
                                NumberAnimation { target: slot; property: "exitDrop"; to: Math.min(28, panel.outroPadding + card.height * 0.05); duration: panel.animationMs * 0.65; easing.type: Easing.InQuad }
                            }
                            NumberAnimation { target: slot; property: "extent"; to: 0; duration: panel.animationMs * 0.35; easing.type: Easing.InOutCubic }
                            ScriptAction { script: Qt.callLater(() => panel.finishRemove(slot.noticeKey)) }
                        }
                        // An opaque backing prevents old text bleeding through the new face.
                        Rectangle {
                            visible: slot.started && !slot.firstArrival && !slot.retiring && slot.reveal < 1
                            width: card.width
                            height: card.height
                            anchors.horizontalCenter: parent.horizontalCenter
                            y: card.y
                            scale: card.scale
                            radius: card.radius
                            color: card.color
                        }
                        NotificationCard {
                            id: card
                            parent: slot.retiring ? exitLayer : slot
                            opacity: panel.controller.isFloating(notice.id) ? 0 : slot.reveal
                            enabled: !slot.retiring
                            width: slot.width
                            height: implicitHeight
                            anchors.horizontalCenter: parent.horizontalCenter
                            y: slot.retiring ? slot.exitY + slot.exitDrop : slot.height - height
                            visible: slot.cachedNotice !== null
                            notice: slot.cachedNotice || ({id:"",title:"",body:"",app:"",urgency:"normal",icon:"",image:"",actions:[],data:{},type:"standard"})
                            controller: panel.controller
                            monitorName: panel.screen.name
                            onDragStarted: (x, y) => {
                                panel.controller.saveFloatingState(notice.id, card.saveState());
                                const origin = card.mapToItem(panel.contentItem, 0, 0);
                                panel.controller.beginDrag(notice.id, panel.screen.name,
                                    panel.screen.x + panel.margins.left + origin.x,
                                    panel.screen.y + panel.margins.top + origin.y, card.width, card.height);
                            }
                            onDragMoved: (dx, dy) => panel.controller.moveDrag(notice.id, dx, dy)
                            onDragFinished: panel.controller.endDrag(notice.id)
                            selected: panel.controller.selectedId === notice.id && panel.controller.keyboardMode
                            // Fade/scale the whole card from the start of its upward push.
                            // Settled cards keep their full size and opacity.
                            scale: slot.exitScale
                        }
                    }
                }
            }
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        }
        Rectangle {
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom; bottomMargin: panel.outroPadding }
            height: 80
            visible: panel.redacted
            color: panel.theme.background || "#191919"
            radius: panel.theme.radius || 14
            border.color: panel.theme.border || "#505050"
            Text {
                anchors.centerIn: parent
                text: "Notifications hidden while locked"
                color: panel.theme.muted || "#aaaaaa"
                font { family: panel.theme.font || "monospace"; pixelSize: panel.theme.fontSize || 14 }
            }
        }
    }
}
