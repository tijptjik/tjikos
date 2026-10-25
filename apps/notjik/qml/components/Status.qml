import QtQuick
import QtQuick.Layouts

RowLayout {
    id: component
    required property var notice
    required property var theme
    required property double now
    spacing: 8
    Rectangle {
        implicitWidth: 6
        implicitHeight: 6
        radius: 3
        color: component.notice.data.tone === "critical" ? component.theme.critical : component.notice.data.tone === "success" ? component.theme.success : component.theme.accent
    }
    Text {
        Layout.maximumWidth: component.width * 0.55
        text: component.notice.data.state || "Status"
        textFormat: Text.PlainText
        elide: Text.ElideRight
        color: component.theme.text
        font { family: component.theme.font; pixelSize: component.theme.fontSize - 1; bold: true }
    }
    Text {
        Layout.fillWidth: true
        text: component.notice.data.detail || ""
        textFormat: Text.PlainText
        elide: Text.ElideRight
        color: component.notice.urgency === "critical" ? component.theme.text : component.theme.muted
        font { family: component.theme.font; pixelSize: component.theme.fontSize - 2 }
        horizontalAlignment: Text.AlignRight
    }
}
