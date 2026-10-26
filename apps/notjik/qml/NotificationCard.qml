import QtQuick
import QtQuick.Window
import QtQuick.Controls
import QtQuick.Templates as T
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Quickshell
import "Logic.js" as Logic

Rectangle {
    id: card
    required property var notice
    required property var controller
    required property string monitorName
    property bool floating: false
    readonly property bool dragging: cardDrag.active
    signal dragStarted(real x, real y)
    signal dragMoved(real dx, real dy)
    signal dragFinished()
    // Walk the hit branch, preserving native button, slider and editor gestures.
    function interactiveAt(item, point) {
        if (!item || !item.visible || !item.enabled) return false;
        if (item !== card && (item instanceof T.AbstractButton || item instanceof T.Slider
                || item instanceof T.RangeSlider || item instanceof T.ComboBox || item instanceof T.SpinBox
                || item instanceof T.ScrollBar || item instanceof MouseArea
                || item instanceof TextInput || item instanceof TextEdit)) return true;
        const child = item.childAt(point.x, point.y);
        return child ? interactiveAt(child, child.mapFromItem(item, point.x, point.y)) : false;
    }
    property var savedState: null
    function saveState() { return {page: actions.page, detail: detail.item?.saveState ? detail.item.saveState() : {}}; }
    function restoreState(state) {
        if (!state) return;
        savedState = state;
        actions.page = state.page || 0;
        if (detail.item?.restoreState) { detail.item.restoreState(state.detail || {}); savedState = null; }
    }
    property bool historyMode: false
    property bool selected: false
    readonly property bool hovered: hover.hovered
    readonly property bool controlsRevealed: hovered || (card.Window.active && selected && controller.keyboardMode)
        || !!detail.item?.interactionHeld
    readonly property bool hasInteractions: !notice.archived && (availableActions.length > 0 || floating
        || !!detail.item?.hasInteractions)
    readonly property var availableActions: Logic.actionButtonsFor(notice, controller.config)
    readonly property bool structuredReady: !!presentation && structuredDetail.status === Loader.Ready
    readonly property var presentation: Logic.notificationPresentation(notice, controller.config)
    readonly property bool richTelegram: notice.type === "telegram"
    readonly property string initials: notice.title.trim().split(/\s+/).slice(0, 2).map(word => word.charAt(0)).join("").toUpperCase()
    readonly property string telegramAge: controller.now - (notice.received || controller.now) < 60000 ? "now" : Qt.formatDateTime(new Date(notice.received), "HH:mm")
    readonly property bool telegram: Logic.telegramNotice(notice)
    readonly property bool opensChat: !historyMode && Logic.telegramChatAction(notice)>=0
    readonly property var theme: controller.theme
    readonly property var urgencyInk: Logic.urgencyColors(notice.urgency, theme)
    readonly property color accent: notice.urgency === "critical" ? theme.critical : theme.accent
    readonly property string fallbackArtwork: Qt.resolvedUrl(Logic.robotArtwork(notice)).toString()
    // Kitty supplies its terminal icon as an image; use portrait artwork for its reports.
    readonly property bool kittyReport: [notice.app, notice.desktopEntry].some(value =>
        /^(kitty|kitty\.desktop)$/i.test(String(value || "")))
    readonly property string cachedAvatar: Logic.cachedAvatar(notice, controller.avatarCache)
    readonly property string artwork: kittyReport ? fallbackArtwork : (cachedAvatar || notice.image || notice.icon || fallbackArtwork)
    readonly property string resolvedArtwork: imageSource(artwork)
    readonly property bool imageCover: !kittyReport && !!(cachedAvatar || notice.image)
    // Rich Telegram cards always use the full-height cover rail. When a preview
    // policy withholds the profile photo, initials occupy the same rail instead
    // of collapsing the card into the ordinary small-icon layout.
    readonly property bool telegramRail: richTelegram || !!cachedAvatar
    readonly property bool avatarCover: telegram && !telegramRail && imageCover
    readonly property bool coverArtwork: telegramRail || imageCover || (!richTelegram && (resolvedArtwork === fallbackArtwork
        || resolvedArtwork === Qt.resolvedUrl("icons/wifi-cover.png").toString()))
    readonly property real coverWidth: 80
    readonly property real bodyHeight: Math.max(avatarCover ? coverWidth + 2 * border.width : (coverArtwork ? 80 : 0), content.implicitHeight + theme.padding * 2)
    // Admission reserves the fully revealed footer even while it is collapsed.
    readonly property real admissionHeight: bodyHeight + (actionFooter.visible ? actions.implicitHeight + 12 : 0)
    implicitHeight: bodyHeight + (actionFooter.visible ? actionFooter.implicitHeight : 0)
    color: theme.background
    radius: theme.radius
    border.width: theme.borderWidth
    border.color: selected ? Qt.tint(theme.border, Qt.rgba(accent.r, accent.g, accent.b, 0.25)) : theme.border
    Item {
        id: coverRail
        objectName: "notification-cover"
        visible: card.coverArtwork
        x: card.border.width
        y: card.border.width
        width: card.coverWidth
        height: card.avatarCover ? width : Math.max(0, card.height - 2 * card.border.width)
        clip: true
        Image {
            id: coverImage
            width: parent.width
            // Reserve the expanded footer in the image canvas. Only the rail's
            // clip changes during reveal; PreserveAspectCrop must not rescale
            // the artwork on every animation frame.
            height: card.avatarCover ? width : Math.max(0, card.admissionHeight - 2 * card.border.width)
            source: coverRail.visible && (!card.telegramRail || card.imageCover) ? card.resolvedArtwork : ""
            sourceSize { width: 320; height: 320 }
            fillMode: card.avatarCover ? Image.PreserveAspectFit : Image.PreserveAspectCrop
            asynchronous: true
        }
        MouseArea {
            anchors.fill: parent
            enabled: !card.richTelegram && card.opensChat
            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
            Accessible.role: Accessible.Button
            Accessible.name: "Open Telegram chat"
            onClicked: card.controller.openTelegramChat(card.notice.id)
        }
        layer.enabled: !card.avatarCover
        layer.effect: OpacityMask {
            maskSource: Item {
                width: coverRail.width
                height: coverRail.height
                Rectangle {
                    width: parent.width + radius
                    height: parent.height
                    radius: Math.max(0, card.radius - card.border.width)
                    color: "white"
                }
                Rectangle {
                    visible: coverRail.height < card.height - 2 * card.border.width
                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                    height: card.radius
                    color: "white"
                }
            }
        }
    }
    Rectangle {
        id: telegramFallbackRail
        objectName: "telegram-fallback-rail"
        visible: card.telegramRail && !card.imageCover
        x: card.border.width
        y: card.border.width
        width: card.coverWidth
        height: Math.max(0, card.height - 2 * card.border.width)
        radius: Math.max(0, card.radius - card.border.width)
        color: card.theme.surface
        Text {
            anchors.centerIn: parent
            text: card.initials
            color: card.accent
            font { family: card.theme.font; pixelSize: 18; bold: true }
        }
        MouseArea {
            anchors.fill: parent
            enabled: card.opensChat
            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
            Accessible.role: Accessible.Button
            Accessible.name: "Open Telegram chat"
            onClicked: card.controller.openTelegramChat(card.notice.id)
        }
    }
    Rectangle {
        objectName: "interaction-hint"
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        width: parent.width * 0.3
        height: Math.max(3, card.border.width + 2)
        radius: height / 2
        color: card.border.color
        // Do not flash the collapsed marker over the still-closing footer.
        visible: card.hasInteractions && !card.controlsRevealed && actionFooter.reveal === 0
    }
    Accessible.role: Accessible.AlertMessage
    Accessible.name: notice.title + ". " + notice.body

    function imageSource(value) {
        if (!value) return "";
        if (value.startsWith("/") || value.startsWith("file:") || value.startsWith("image:")) return value;
        if (value.startsWith("network-wireless")) return Qt.resolvedUrl("icons/wifi-cover.png").toString();
        return Quickshell.iconPath(value, true) || fallbackArtwork;
    }

    HoverHandler {
        id: hover
        onHoveredChanged: card.controller.setHover(card.monitorName, card.notice.id, hovered)
    }
    Component.onDestruction: controller.setHover(monitorName, notice.id, false)
    Item {
        id: dragArea
        anchors.fill: parent
        containmentMask: QtObject {
            function contains(point: point): bool {
                // This item is behind the content; inspect the card's other branches.
                if (point.x < 0 || point.y < 0 || point.x >= card.width || point.y >= card.height) return false;
                for (const child of card.children) {
                    if (child === dragArea || !child.visible || !child.enabled) continue;
                    const local = child.mapFromItem(card, point.x, point.y);
                    if (child.contains(local) && card.interactiveAt(child, local)) return false;
                }
                return true;
            }
        }
        TapHandler {
            gesturePolicy: TapHandler.DragThreshold
            onTapped: {
                if (card.historyMode) {
                    if (card.controller.historySelectedKey === card.notice.historyKey) card.controller.historySelectedKey = "";
                    else card.controller.selectHistory(card.notice.historyKey);
                } else if (!card.floating) card.controller.close(card.notice.id, false);
            }
        }
        DragHandler {
            id: cardDrag
            enabled: !card.historyMode && !card.notice.archived && !card.controller.locked && !detail.item?.busy
            target: null
            acceptedButtons: Qt.LeftButton
            onActiveChanged: {
                if (active) {
                    const point = card.mapFromItem(null, centroid.scenePressPosition.x, centroid.scenePressPosition.y);
                    card.dragStarted(point.x, point.y);
                } else card.dragFinished();
            }
            onActiveTranslationChanged: if (active) card.dragMoved(activeTranslation.x, activeTranslation.y)
        }
    }

    // Group marks are informational; taps pass through to normal card dismissal.
    Item {
        id: groupIndicator
        anchors { right: parent.right; top: parent.top; bottom: parent.bottom; topMargin: card.radius; bottomMargin: card.radius }
        width: Math.max(12, card.theme.padding)
        visible: !card.historyMode && (groupNotches.grouped || groupNotches.opacity > 0)
        Accessible.role: Accessible.StaticText
        Accessible.name: (card.notice.groupCount || 1) + " messages"
        GroupNotches {
            id: groupNotches
            anchors.fill: parent
            count: card.historyMode ? 1 : (card.notice.groupCount || 1)
            animationMs: card.controller.config.stack?.animationMs ?? 220
            notchWidth: Math.max(3, card.theme.borderWidth + 2)
            color: card.theme.muted
            inkOpacity: 0.75
        }
    }

    ColumnLayout {
        id: content
        anchors {
            left: parent.left; right: parent.right; top: parent.top
            margins: card.theme.padding
            leftMargin: card.coverArtwork ? card.coverWidth + card.theme.padding : card.theme.padding
            topMargin: card.coverArtwork ? Math.max(card.theme.padding,
                (card.bodyHeight - content.implicitHeight) / 2) : card.theme.padding
        }
        spacing: 10
        Text {
            Layout.fillWidth: true
            visible: card.historyMode
            text: card.historyMode ? Qt.formatDateTime(new Date(card.notice.received || 0), "dd MMM HH:mm") + " · " + (card.notice.lastEvent || "archived") : ""
            textFormat: Text.PlainText
            color: card.theme.muted
            font { family: card.theme.font; pixelSize: card.theme.fontSize - 3 }
            elide: Text.ElideRight
        }
        RowLayout {
            id: header
            Layout.fillWidth: true
            spacing: card.richTelegram ? 10 : 14
            Rectangle {
                visible: !card.coverArtwork && card.artwork.length > 0
                Layout.alignment: Qt.AlignTop
                implicitWidth: card.richTelegram ? 32 : card.theme.iconSize
                implicitHeight: card.richTelegram ? 32 : card.theme.iconSize
                radius: card.telegram ? 0 : Math.min(card.theme.radius, 10)
                color: card.theme.surface
                Image {
                    id: artworkImage
                    visible: !card.richTelegram || !!card.notice.image
                    anchors { fill: parent; margins: card.richTelegram ? 0 : 4 }
                    source: card.imageSource(card.artwork)
                    fillMode: Image.PreserveAspectFit
                    sourceSize { width: card.theme.iconSize * 2; height: card.theme.iconSize * 2 }
                    asynchronous: true
                    layer.enabled: card.richTelegram || (!card.notice.image && card.notice.icon.endsWith("-symbolic") && !card.notice.icon.startsWith("network-wireless"))
                    layer.effect: card.richTelegram ? avatarMaskEffect : artworkTintEffect
                    Component {
                        id: avatarMaskEffect
                        OpacityMask { maskSource: Rectangle { width: artworkImage.width; height: artworkImage.height; radius: 0 } }
                    }
                    Component { id: artworkTintEffect; ColorOverlay { color: card.urgencyInk.artwork } }
                }
                MouseArea {
                    objectName: "telegram-chat-avatar"
                    anchors.fill:parent;z:1;enabled:card.opensChat
                    cursorShape:enabled?Qt.PointingHandCursor:Qt.ArrowCursor
                    Accessible.role:Accessible.Button
                    Accessible.name:"Open Telegram chat"
                    onClicked:card.controller.openTelegramChat(card.notice.id)
                }
                Text {
                    anchors.centerIn: parent
                    visible: (card.richTelegram && (!card.notice.image || artworkImage.status === Image.Loading)) || artworkImage.status === Image.Error || artworkImage.source.toString() === ""
                    text: card.richTelegram ? card.initials : "●"
                    font { family: card.theme.font; pixelSize: 11; bold: true }
                    color: card.accent
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: card.richTelegram ? 3 : 7
                Text {
                    Layout.fillWidth: true
                    text: card.notice.title
                    textFormat: Text.PlainText
                    color: card.urgencyInk.title
                    wrapMode: Text.Wrap
                    maximumLineCount: 3
                    elide: Text.ElideRight
                    font { family: card.theme.font; pixelSize: card.theme.fontSize + 1; bold: true }
                }
                Text {
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: card.structuredReady ? (card.telegram ? "Telegram · " + card.telegramAge : "")
                        : card.richTelegram ? "Telegram · " + card.telegramAge : Logic.spacedBody(card.notice.body)
                    textFormat: Text.PlainText
                    color: card.urgencyInk.body
                    lineHeight: 1.22
                    lineHeightMode: Text.ProportionalHeight
                    wrapMode: Text.Wrap
                    maximumLineCount: card.theme.maxBodyLines
                    elide: Text.ElideRight
                    font { family: card.theme.font; pixelSize: card.richTelegram || card.presentation ? card.theme.fontSize - 3 : card.theme.fontSize }
                }
            }
            ActionButton {
                id: closeButton
                objectName: "notification-close"
                theme: card.theme
                text: "×"
                labelSize: 18
                implicitWidth: 20
                Layout.alignment: Qt.AlignTop
                visible: card.floating
                opacity: card.controlsRevealed ? 1 : 0
                enabled: card.controlsRevealed
                Accessible.name: "Dismiss notification"
                onClicked: card.controller.close(card.notice.id, false)
            }
        }
        Loader {
            id: structuredDetail
            Layout.fillWidth: true
            visible: status === Loader.Ready
            Layout.preferredHeight: item ? item.implicitHeight : 0
            function loadComponent() {
                const view = card.presentation;
                if (!view) { source = ""; return; }
                const custom = card.controller.config.components[view.type];
                const url = custom || (view.type === "key-value" ? Qt.resolvedUrl("components/KeyValue.qml") : "");
                const properties = {notice:Object.assign({}, card.notice, {data:view.data}), theme:card.theme, now:card.controller.now};
                if (source.toString() === url.toString() && item) {
                    for (const key of Object.keys(properties)) item[key] = properties[key];
                } else setSource(url, properties);
            }
            Component.onCompleted: loadComponent()
            Connections {
                target: card
                function onPresentationChanged() { structuredDetail.loadComponent(); }
                function onNoticeChanged() { structuredDetail.loadComponent(); }
                function onThemeChanged() { structuredDetail.loadComponent(); }
            }
        }
        Loader {
            id: detail
            Layout.fillWidth: true
            // An empty Telegram detail must not reserve a ColumnLayout spacing slot.
            visible: source.toString().length > 0 && status === Loader.Ready && !!item && item.implicitHeight > 0
            Layout.preferredHeight: item ? item.implicitHeight : 0
            readonly property var builtins: ({progress: "components/Progress.qml", status: "components/Status.qml", timer: "components/Countdown.qml", telegram: "components/Telegram.qml", "results-table": "components/ResultsTable.qml"})
            function loadComponent() {
                const custom = card.controller.config.components[card.notice.type];
                const builtin = builtins[card.notice.type];
                const url = custom || (builtin ? Qt.resolvedUrl(builtin) : "");
                if (url) {
                    const properties = {notice: card.notice, theme: card.theme, now: card.controller.now};
                    if (card.notice.type === "telegram" && !custom) {
                        properties.showBody = Qt.binding(() => !card.structuredReady);
                        properties.controlsRevealed = Qt.binding(() => card.controlsRevealed);
                        properties.controller = card.controller;
                        properties.monitorName = card.monitorName;
                        properties.historyMode = card.historyMode || Logic.callerBlocked(card.notice, card.controller.config);
                    }
                    if (source.toString() === url.toString() && item) {
                        for (const key of Object.keys(properties)) item[key] = properties[key];
                    } else setSource(url, properties);
                }
                else source = "";
            }
            Component.onCompleted: loadComponent()
            onStatusChanged: { if (status === Loader.Ready && card.savedState) card.restoreState(card.savedState); if (status === Loader.Error) console.warn("notjik component failed:", source); }
            Connections {
                target: card
                function onNoticeChanged() { detail.loadComponent(); }
                function onMonitorNameChanged() { detail.loadComponent(); }
            }
            Connections {
                target: card.controller
                function onNowChanged() { if (detail.item) detail.item.now = card.controller.now; }
                function onConfigChanged() { detail.loadComponent(); }
            }
        }
    }
    Rectangle {
        id: actionFooter
        property real reveal: card.controlsRevealed ? 1 : 0
        Behavior on reveal { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
        opacity: reveal
        clip: true
        enabled: card.controlsRevealed
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom; margins: card.border.width; leftMargin: card.coverArtwork ? card.coverWidth + card.border.width : card.border.width }
        visible: !card.notice.archived && card.availableActions.length > 0
        implicitHeight: reveal * (actions.implicitHeight + 12)
        height: implicitHeight
        radius: Math.max(0, card.radius - card.border.width)
        color: "transparent"
        Rectangle {
            anchors { left: parent.left; right: parent.right; top: parent.top; leftMargin: card.theme.padding; rightMargin: card.theme.padding }
            height: 1
            color: card.theme.border
            opacity: 0.6
        }
        Item {
            id: actions
            anchors { left: parent.left; right: parent.right; top: parent.top; leftMargin: card.theme.padding; rightMargin: card.theme.padding; topMargin: 6 }
            implicitHeight: Math.max(26, ...Array.from({length: pages.count}, (_, i) => pages.itemAt(i) ? pages.itemAt(i).implicitHeight : 0))
            height: implicitHeight
            property int page: 0
            // Freedesktop's default action is primary; local actions use the first.
            readonly property int primaryIndex: {
                const index = card.availableActions.findIndex(action => action.identifier === "default");
                return card.availableActions[index < 0 ? 0 : index]?.actionIndex ?? -1;
            }
            readonly property int gap: 12
            readonly property int navigationWidth: 32
            FontMetrics {
                id: actionMetrics
                font { family: card.theme.font; pixelSize: card.theme.fontSize - 2 }
            }
            // Text uses Qt's scene-graph metrics, which can differ from FontMetrics.
            // Include glyph overhang as well as the actual label's natural width.
            property int measureRevision: 0
            Repeater {
                id: actionLabels
                model: card.availableActions
                onItemAdded: actions.measureRevision++
                onItemRemoved: actions.measureRevision++
                Text {
                    required property var modelData
                    visible: false
                    text: modelData.label
                    textFormat: Text.PlainText
                    font { family: card.theme.font; pixelSize: card.theme.fontSize - 2 }
                }
            }
            function buttonWidth(label, index) {
                const revision = measureRevision;
                return Math.ceil(Math.max(actionLabels.itemAt(index)?.implicitWidth || actionMetrics.advanceWidth(label),
                                          actionMetrics.boundingRect(label).width)) + 4;
            }
            readonly property bool overflow: {
                if (card.availableActions.length < 2) return false;
                const font = actionMetrics.font;
                let needed = Math.max(0, card.availableActions.length - 1) * gap;
                for (let i = 0; i < card.availableActions.length; ++i)
                    needed += buttonWidth(card.availableActions[i].label, i);
                return needed > width;
            }
            readonly property var groups: {
                const available = actionViewport.width;
                const font = actionMetrics.font;
                const result = [];
                let row = [], used = 0;
                for (let i = 0; i < card.availableActions.length; ++i) {
                    const action = card.availableActions[i];
                    const size = Math.min(available, buttonWidth(action.label, i));
                    if (row.length && used + gap + size > available) {
                        result.push(row); row = []; used = 0;
                    }
                    used += (row.length ? gap : 0) + size;
                    row.push({label:action.label, actionIndex:action.actionIndex, buttonWidth:size});
                }
                if (row.length) result.push(row);
                return result;
            }
            onGroupsChanged: page = Math.min(page, Math.max(0, groups.length - 1))
            function advance(direction) {
                page = Math.max(0, Math.min(groups.length - 1, page + direction));
            }
            component PageButton: ActionButton {
                required property bool forward
                theme: card.theme
                disabledColor: card.theme.muted
                width: actions.navigationWidth
                height: actions.height
                enabled: forward ? actions.page < actions.groups.length - 1 : actions.page > 0
                opacity: enabled ? 1 : 0
                Accessible.name: forward ? "Next actions" : "Previous actions"
                text: forward ? "›" : "‹"
                labelSize: card.theme.fontSize + 6
                onClicked: actions.advance(forward ? 1 : -1)
            }
            PageButton {
                id: previousActions
                forward: false
                visible: actions.overflow
                anchors.left: parent.left
            }
            Item {
                id: actionViewport
                anchors { left: parent.left; right: parent.right }
                anchors.leftMargin: actions.overflow ? actions.navigationWidth + 4 : 0
                anchors.rightMargin: actions.overflow ? actions.navigationWidth + 4 : 0
                height: actions.height
                clip: true
                Item {
                    id: actionStrip
                    x: -actions.page * actionViewport.width
                    width: actionViewport.width * actions.groups.length
                    height: parent.height
                    Behavior on x { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
                    Repeater {
                        id: pages
                        model: actions.groups
                        Item {
                            id: actionPage
                            required property var modelData
                            required property int index
                            x: index * actionViewport.width
                            width: actionViewport.width
                            height: actionViewport.height
                            implicitHeight: actionRow.implicitHeight
                            enabled: index === actions.page
                            Item {
                                id: actionRow
                                anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter }
                                implicitHeight: Math.max(0, ...Array.from({length: pageActions.count}, (_, i) => pageActions.itemAt(i)?.implicitHeight || 0))
                                height: implicitHeight
                                function buttonX(action) {
                                    if (action.actionIndex === actions.primaryIndex) return width - action.buttonWidth;
                                    let offset = 0;
                                    for (const item of actionPage.modelData) {
                                        if (item.actionIndex === action.actionIndex) break;
                                        if (item.actionIndex !== actions.primaryIndex) offset += item.buttonWidth + actions.gap;
                                    }
                                    return offset;
                                }
                                Repeater {
                                    id: pageActions
                                    model: actionPage.modelData
                                    ActionButton {
                                        id: actionButton
                                        required property var modelData
                                        theme: card.theme
                                        // Disabling input during collapse must not flash disabled ink.
                                        disabledColor: card.theme.muted
                                        text: modelData.label
                                        width: modelData.buttonWidth
                                        x: actionRow.buttonX(modelData)
                                        anchors.verticalCenter: parent.verticalCenter
                                        onClicked: card.controller.invoke(card.notice.id, modelData.actionIndex)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            PageButton {
                id: nextActions
                forward: true
                visible: actions.overflow
                anchors.right: parent.right
            }
        }
    }
}
