import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: drawer
    required property var controller
    readonly property var theme:controller.theme
    property string monitorName:''
    readonly property bool editing:searchInput.activeFocus
    signal closeRequested()
    property bool searchOpen: false
    property bool hadWindowFocus: false
    function focusHistory() {
        searchInput.focus = false;
        focusArea.forceActiveFocus();
    }
    function startSearch() {
        searchOpen = true;
        searchInput.forceActiveFocus();
        searchInput.selectAll();
    }
    function clearSearch() {
        controller.historyQuery = "";
        searchOpen = false;
        focusHistory();
    }
    function browseSearch(backward) {
        const items = controller.filteredHistoryItems;
        if (!items.length) return;
        const index = items.findIndex(n => n.historyKey === controller.historySelectedKey);
        const next = index < 0 ? (backward ? items.length - 1 : 0) :
            (index + (backward ? -1 : 1) + items.length) % items.length;
        controller.selectHistory(items[next].historyKey);
    }
    function handleSearchTab(event) {
        if (!searchOpen || (event.key !== Qt.Key_Tab && event.key !== Qt.Key_Backtab)) return false;
        browseSearch(event.key === Qt.Key_Backtab || !!(event.modifiers & Qt.ShiftModifier));
        event.accepted = true;
        return true;
    }
    function handleEscape() {
        if (searchOpen || controller.historyFiltering) clearSearch();
        else drawer.closeRequested();
    }
    function revealSelection() {
        const index = controller.historyRows.findIndex(row => row.kind === "notice" && row.key === controller.historySelectedKey);
        if (index >= 0) Qt.callLater(() => historyList.positionViewAtIndex(index, ListView.Contain));
    }
    Connections {
        target: drawer.controller
        function onHistorySelectedKeyChanged() { drawer.revealSelection(); }
        function onHistoryRowsChanged() { drawer.revealSelection(); }
        function onHistoryQueryChanged() { historyList.positionViewAtBeginning(); }
    }

    onVisibleChanged:if(visible){searchOpen=false;historyList.positionViewAtBeginning();}
        FocusScope {
            id: focusArea
            anchors.fill: parent
            Keys.onPressed: event => {
                if (drawer.handleSearchTab(event)) return;
                if (event.key === Qt.Key_Escape) {
                    drawer.handleEscape();
                    event.accepted = true;
                    return;
                }
                if (searchInput.activeFocus) return;
                if (event.key === Qt.Key_Slash) drawer.startSearch();
                else if (event.key === Qt.Key_Up) drawer.controller.navigate("previous");
                else if (event.key === Qt.Key_Down) drawer.controller.navigate("next");
                else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) drawer.controller.activateSelected();
                else if (event.key === Qt.Key_Delete || event.key === Qt.Key_Backspace) drawer.controller.dismissSelected();
                else return;
                event.accepted = true;
            }
            ColumnLayout {
                anchors.fill: parent
                spacing: 12
                RowLayout {
                    Layout.fillWidth:true
                    Text {
                        Layout.fillWidth:true
                        text:(drawer.controller.historyFiltering?drawer.controller.filteredHistoryItems.length+' / ':'')+drawer.controller.historyItems.length+' notifications'
                        color:drawer.theme.muted;font.family:drawer.theme.font;font.pixelSize:12
                    }
                    Button {
                        text:drawer.searchOpen?'Clear':'Search';Accessible.name:drawer.searchOpen?'Clear history filter':'Search notification history'
                        onClicked:drawer.searchOpen?drawer.clearSearch():drawer.startSearch()
                        background:Rectangle {radius:8;color:drawer.theme.surface}
                        contentItem:Text {text:parent.text;color:drawer.theme.text;font.family:drawer.theme.font;font.pixelSize:12;padding:8}
                    }
                }
                Text {
                    Layout.fillWidth: true
                    text: drawer.searchOpen ? "Tab next · Shift+Tab previous · Esc clear" :
                        "↑ ↓ browse · / filter · Enter open · Delete dismiss"
                    color: drawer.theme.muted || "#aaa"
                    font { family: drawer.theme.font || "monospace"; pixelSize: (drawer.theme.fontSize || 14) - 3 }
                    MouseArea { anchors.fill: parent; onClicked: drawer.focusHistory() }
                }
                RowLayout {
                    visible: drawer.searchOpen
                    Layout.fillWidth: true
                    TextField {
                        id: searchInput
                        Layout.fillWidth: true
                        text: drawer.controller.historyQuery
                        placeholderText: "Filter messages…"
                        Accessible.name: "Filter notification history"
                        color: drawer.theme.text || "#eee"
                        placeholderTextColor: drawer.theme.muted || "#aaa"
                        selectionColor: drawer.theme.accent || "#fff"
                        selectedTextColor: drawer.theme.background || "#191919"
                        font { family: drawer.theme.font || "monospace"; pixelSize: drawer.theme.fontSize || 14 }
                        padding: 10
                        background: Rectangle {
                            color: drawer.theme.surface || "#242424"
                            radius: 6
                            border.color: searchInput.activeFocus ? (drawer.theme.accent || "#fff") : (drawer.theme.border || "#505050")
                        }
                        onTextEdited: drawer.controller.historyQuery = text
                        Keys.onPressed: event => {
                            if (drawer.handleSearchTab(event)) return;
                            if (event.key === Qt.Key_Up) drawer.controller.navigate("previous");
                            else if (event.key === Qt.Key_Down) drawer.controller.navigate("next");
                            else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                                if (!drawer.controller.historySelectedKey) drawer.controller.navigate("next");
                                drawer.focusHistory();
                            } else return;
                            event.accepted = true;
                        }
                    }
                    Button {
                        text: "×"
                        Accessible.name: "Clear history filter"
                        padding: 8
                        background: Rectangle { color: drawer.theme.surface || "#242424"; radius: 6 }
                        contentItem: Text { text: parent.text; color: drawer.theme.text || "#eee"; font.pixelSize: 20 }
                        onClicked: drawer.clearSearch()
                    }
                }
                Button {
                    visible: drawer.controller.paused
                    Layout.fillWidth: true
                    text: "Notifications paused · Resume"
                    onClicked: drawer.controller.handle({op: "resume"})
                    background: Rectangle { color: drawer.theme.surface || "#242424"; radius: 6 }
                    contentItem: Text { text: parent.text; color: drawer.theme.accent || "#fff"; font.pixelSize: drawer.theme.fontSize || 14; padding: 8 }
                }
                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: drawer.theme.border || "#505050" }
                ListView {
                    id: historyList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    spacing: 4
                    model: drawer.controller.historyRows
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
                    delegate: Loader {
                        id: rowLoader
                        required property var modelData
                        width: historyList.width
                        sourceComponent: modelData.kind === "group" ? groupHeading : historicalCard
                        Component {
                            id: groupHeading
                            Button {
                                text: (drawer.controller.historyGroupExpanded(rowLoader.modelData.key) ? "▾  " : "▸  ") + rowLoader.modelData.app +
                                    (rowLoader.modelData.label ? " · " + rowLoader.modelData.label : "") + "  (" + rowLoader.modelData.count + ")"
                                padding: 12
                                background: Rectangle { color: drawer.theme.surface || "#242424"; radius: 8 }
                                contentItem: Text {
                                    text: parent.text
                                    textFormat: Text.PlainText
                                    color: drawer.theme.accent || "#fff"
                                    elide: Text.ElideRight
                                    font { family: drawer.theme.font || "monospace"; pixelSize: drawer.theme.fontSize || 14 }
                                }
                                onClicked: drawer.controller.toggleGroup(rowLoader.modelData.key)
                            }
                        }
                        Component {
                            id: historicalCard
                            Column {
                                id: historyEntry
                                readonly property var notice: rowLoader.modelData.notice
                                readonly property bool selected: drawer.controller.historySelectedKey === rowLoader.modelData.key
                                spacing: 4
                                Button {
                                    visible: !historyEntry.selected
                                    width: parent.width
                                    padding: 10
                                    onClicked: {
                                        if (historyEntry.selected) drawer.controller.historySelectedKey = "";
                                        else drawer.controller.selectHistory(rowLoader.modelData.key);
                                    }
                                    background: Rectangle {
                                        color: parent.hovered || historyEntry.selected ? drawer.theme.surface : "transparent"
                                        radius: 6
                                    }
                                    contentItem: ColumnLayout {
                                        spacing: 4
                                        RowLayout {
                                            Layout.fillWidth: true
                                            Text {
                                                Layout.fillWidth: true
                                                text: historyEntry.notice.title || "Untitled notification"
                                                textFormat: Text.PlainText
                                                elide: Text.ElideRight
                                                color: drawer.theme.text
                                                font { family: drawer.theme.font; pixelSize: drawer.theme.fontSize; bold: true }
                                            }
                                            Text {
                                                text: {
                                                    const date = new Date(historyEntry.notice.received || 0);
                                                    return Qt.formatDateTime(date, date.toDateString() === new Date(drawer.controller.now).toDateString() ? "HH:mm" : "dd MMM HH:mm");
                                                }
                                                color: drawer.theme.muted
                                                font { family: drawer.theme.font; pixelSize: drawer.theme.fontSize - 3 }
                                            }
                                        }
                                        Text {
                                            Layout.fillWidth: true
                                            visible: !historyEntry.selected && text.length > 0
                                            text: (historyEntry.notice.body || "").replace(/\s+/g, " ")
                                            textFormat: Text.PlainText
                                            elide: Text.ElideRight
                                            color: drawer.theme.muted
                                            font { family: drawer.theme.font; pixelSize: drawer.theme.fontSize - 1 }
                                        }
                                    }
                                }
                                Loader {
                                    width: parent.width
                                    active: historyEntry.selected
                                    visible: active
                                    sourceComponent: NotificationCard {
                                        notice: drawer.controller.historyCard(historyEntry.notice)
                                        controller: drawer.controller
                                        monitorName: drawer.monitorName
                                        historyMode: true
                                        selected: false
                                        border.width: 0
                                    }
                                }
                            }
                        }
                    }
                    Text {
                        anchors.centerIn: parent
                        visible: drawer.controller.filteredHistoryItems.length === 0
                        text: drawer.controller.historyFiltering ? "No matching notifications" : "No saved notifications yet"
                        color: drawer.theme.muted || "#aaa"
                        font { family: drawer.theme.font || "monospace"; pixelSize: drawer.theme.fontSize || 14 }
                    }
                }
            }
        }
}
