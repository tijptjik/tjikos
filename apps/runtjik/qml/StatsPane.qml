import QtQuick
import QtQuick.Controls

Flickable {
    id: pane
    required property var statistics
    required property var theme
    required property var settings
    property int indexed: 0
    property bool demo: false
    readonly property int weeks: Math.min(26, Math.max(4, Math.floor((width - 68) / 21)))
    readonly property var days: (statistics.calendar || []).slice(-weeks * 7)
    readonly property int peak: Math.max(1, ...days.map(day => day.count))
    property string hoveredDay: ''
    function scrollBy(amount) {
        contentY = Math.max(0, Math.min(Math.max(0, contentHeight - height), contentY + amount));
    }
    function shade(count) {
        return count === 0 ? theme.surface : Qt.rgba(accent.r, accent.g, accent.b, .28 + .72 * Math.ceil(count / peak * 4) / 4);
    }
    readonly property color accent: theme.accent
    function month(date) { return Qt.formatDate(new Date(date + 'T12:00:00'), 'MMM'); }
    function pretty(date) { return Qt.formatDate(new Date(date + 'T12:00:00'), 'd MMM yyyy'); }
    clip: true
    contentHeight: content.height + 40
    boundsBehavior: Flickable.StopAtBounds
    ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

    Column {
        id: content
        x: 20; y: 20; width: parent.width - 40
        spacing: 16
        Row {
            width: parent.width; height: 28
            Text {
                width: parent.width - indexLabel.width
                text: 'Launcher activity'; color: pane.theme.text
                font.family: pane.settings.ui.font; font.pixelSize: 20
                verticalAlignment: Text.AlignVCenter; height: parent.height
            }
            Text {
                id: indexLabel
                text: pane.indexed + ' indexed'; color: pane.theme.muted
                font.family: pane.settings.ui.font; font.pixelSize: 12
                verticalAlignment: Text.AlignVCenter; height: parent.height
            }
        }
        Column {
            width: parent.width; spacing: 8
            Item {
                width: parent.width; height: 16
                Repeater {
                    model: pane.weeks
                    Text {
                        required property int index
                        readonly property var day: pane.days[index * 7]
                        x: 32 + index * calendar.step
                        text: day && (index === 0 || pane.month(day.date) !== pane.month(pane.days[(index - 1) * 7].date)) ? pane.month(day.date) : ''
                        color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10
                    }
                }
            }
            Item {
                id: calendar
                width: parent.width; height: 7 * 20 - 4
                readonly property real step: (width - 32) / pane.weeks
                Repeater {
                    model: ['M', '', 'W', '', 'F', '', 'S']
                    Text {
                        required property int index
                        required property string modelData
                        y: index * 20; height: 16; text: modelData
                        color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10
                        verticalAlignment: Text.AlignVCenter
                    }
                }
                Repeater {
                    model: pane.days
                    Rectangle {
                        required property int index
                        required property var modelData
                        x: 32 + Math.floor(index / 7) * calendar.step; y: (index % 7) * 20
                        width: calendar.step - 4; height: 16; radius: 3
                        color: modelData.tracked ? pane.shade(modelData.count) : 'transparent'
                        border.width: modelData.date === pane.statistics.today || !modelData.tracked ? 1 : 0
                        border.color: modelData.date === pane.statistics.today ? pane.theme.accent : pane.theme.border
                        opacity: modelData.future ? .2 : modelData.tracked ? 1 : .45
                        HoverHandler {
                            onHoveredChanged: {
                                if (hovered) pane.hoveredDay = pane.pretty(modelData.date) + ' · ' + (modelData.future ? 'Upcoming' : !modelData.tracked ? 'Not tracked' : modelData.count + (modelData.count === 1 ? ' open' : ' opens'));
                                else pane.hoveredDay = '';
                            }
                        }
                    }
                }
            }
            Row {
                width: parent.width; height: 16; spacing: 8
                Text {
                    width: parent.width - legend.width - parent.spacing; height: parent.height
                    text: pane.hoveredDay || 'Launcher opens · last ' + pane.weeks + ' weeks'
                    color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 11
                    verticalAlignment: Text.AlignVCenter; elide: Text.ElideRight
                }
                Row {
                    id: legend
                    spacing: 6; anchors.verticalCenter: parent.verticalCenter
                    Text { height: 14; verticalAlignment: Text.AlignVCenter; text: 'Less'; color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10 }
                    Repeater {
                        model: 5
                        Rectangle {
                            required property int index
                            width: 10; height: 10; radius: 2; y: 2
                            color: index === 0 ? pane.theme.surface : Qt.rgba(pane.accent.r, pane.accent.g, pane.accent.b, .28 + .72 * index / 4)
                        }
                    }
                    Text { height: 14; verticalAlignment: Text.AlignVCenter; text: 'More'; color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10 }
                }
            }
        }
        Column {
            width: parent.width; spacing: 0
            Row {
                width: parent.width; height: 24
                Text { width: parent.width * .5; height: parent.height; verticalAlignment: Text.AlignVCenter; text: 'ACTIVITY'; color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10; font.letterSpacing: 1 }
                Text { width: parent.width * .25; height: parent.height; verticalAlignment: Text.AlignVCenter; text: 'DAILY AVG'; horizontalAlignment: Text.AlignRight; color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10; font.letterSpacing: 1 }
                Text { width: parent.width * .25; height: parent.height; verticalAlignment: Text.AlignVCenter; text: 'ALL TIME'; horizontalAlignment: Text.AlignRight; color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10; font.letterSpacing: 1 }
            }
            Repeater {
                model: pane.statistics.metrics || []
                Item {
                    required property var modelData
                    width: parent.width; height: 32
                    Rectangle { width: parent.width; height: 1; color: pane.theme.border; opacity: .45 }
                    Row {
                        anchors.fill: parent
                        Text { width: parent.width * .5; height: parent.height; text: modelData.label; color: pane.theme.text; font.family: pane.settings.ui.font; font.pixelSize: 13; verticalAlignment: Text.AlignVCenter }
                        Text { width: parent.width * .25; height: parent.height; text: modelData.days ? Number(modelData.average).toLocaleString(Qt.locale(), 'f', 1) : '—'; horizontalAlignment: Text.AlignRight; color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 14; verticalAlignment: Text.AlignVCenter }
                        Text { width: parent.width * .25; height: parent.height; text: Number(modelData.total).toLocaleString(Qt.locale(), 'f', 0); horizontalAlignment: Text.AlignRight; color: modelData.id === 'open' ? pane.theme.accent : pane.theme.text; font.family: pane.settings.ui.font; font.pixelSize: 14; verticalAlignment: Text.AlignVCenter }
                    }
                    HoverHandler { id: metricHover }
                    ToolTip.visible: metricHover.hovered
                    ToolTip.text: modelData.since ? 'Daily average across all ' + modelData.days + ' calendar days since ' + pane.pretty(modelData.since) + ', including idle days.' : 'No recorded activity yet.'
                }
            }
        }
        Text {
            width: parent.width
            text: (pane.demo ? 'Sample activity. ' : '') + (pane.settings.history?.enabled === false || pane.settings.stats?.enabled === false ? 'Stats tracking is paused. ' : '')
                + (pane.statistics.opened_since ? 'Opens tracked since ' + pane.pretty(pane.statistics.opened_since) + '. ' : 'No launcher opens recorded yet. ')
                + 'Daily averages include idle days.'
            color: pane.theme.muted; font.family: pane.settings.ui.font; font.pixelSize: 10
            wrapMode: Text.Wrap
        }
    }
}
