import QtQuick
import QtQuick.Layouts

// Register with "components": {"issue": "Issue.qml"} in config.json.
RowLayout {
    id: component
    required property var notice
    required property var theme
    required property double now
    spacing: 12
    Text {
        text: "#" + (component.notice.data.number || "—")
        textFormat: Text.PlainText
        color: component.theme.accent
        font { family: component.theme.font; pixelSize: 24; bold: true }
    }
    Text {
        Layout.fillWidth: true
        text: component.notice.data.project || "Issue"
        textFormat: Text.PlainText
        color: component.theme.muted
        font { family: component.theme.font; pixelSize: component.theme.fontSize }
    }
}
