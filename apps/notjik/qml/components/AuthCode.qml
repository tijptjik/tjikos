import QtQuick
import QtQuick.Layouts

Rectangle {
    id: card
    required property var notice
    required property var theme
    required property double now
    implicitHeight: content.implicitHeight + 24
    radius: Math.max(4, theme.radius - 6)
    color: Qt.alpha(theme.accent, 0.09)
    border.color: Qt.alpha(theme.accent, 0.35)
    ColumnLayout {
        id: content
        anchors.fill: parent; anchors.margins: 12; spacing: 4
        Text {
            Layout.fillWidth: true
            text: String(card.notice.data.code || '')
            textFormat: Text.PlainText; color: card.theme.text
            font.family: 'monospace'; font.pixelSize: 30; font.bold: true; font.letterSpacing: 4
            horizontalAlignment: Text.AlignHCenter
        }
        Text {
            Layout.fillWidth: true
            text: 'Authentication code · Copy code below'
            textFormat: Text.PlainText; color: card.theme.muted
            font.family: card.theme.font; font.pixelSize: 12
            horizontalAlignment: Text.AlignHCenter
        }
    }
}
