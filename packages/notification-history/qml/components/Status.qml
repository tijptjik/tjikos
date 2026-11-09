import QtQuick
import QtQuick.Layouts
import "Rainbow.js" as Rainbow

Rectangle {
    id: component
    required property var notice
    required property var theme
    required property double now
    implicitHeight: row.implicitHeight + 20
    radius: Math.max(4, component.theme.radius - 6)
    color: Qt.darker(component.theme.background, 1.22)
    RowLayout {
        id: row
        anchors { fill: parent; leftMargin: 12; rightMargin: 12; topMargin: 10; bottomMargin: 10 }
        spacing: 8
        Rectangle {
            implicitWidth: 6
            implicitHeight: 6
            radius: 3
            color: component.notice.data.tone === "critical" ? component.theme.critical : component.notice.data.tone === "success" ? component.theme.success : component.theme.accent
        }
        Text {
            Layout.maximumWidth: row.width * 0.55
            readonly property string state: component.notice.data.state || "Status"
            readonly property bool rainbow: component.notice.data["rainbow-text"] === true
            text: rainbow ? Rainbow.text(state) : state
            // StyledText keeps the same compact line box as a plain status label.
            textFormat: rainbow ? Text.StyledText : Text.PlainText
            elide: Text.ElideRight
            color: component.notice.data.tone === "critical" ? component.theme.critical : component.notice.data.tone === "success" ? component.theme.success : component.theme.text
            font { family: component.theme.font; pixelSize: component.theme.fontSize - 1; bold: true }
        }
        Text {
            Layout.fillWidth: true
            text: component.notice.data.detail || ""
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: component.theme.text
            opacity: component.notice.urgency === "critical" ? 1 : 0.85
            font { family: component.theme.font; pixelSize: component.theme.fontSize - 1 }
            horizontalAlignment: Text.AlignRight
        }
    }
}
