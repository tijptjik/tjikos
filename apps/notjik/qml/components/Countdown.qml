import QtQuick
import QtQuick.Layouts

RowLayout {
    id: component
    required property var notice
    required property var theme
    required property double now
    readonly property int seconds: Math.max(0, Math.ceil(((notice.updatedAt || notice.received) + (notice.data.seconds || 0) * 1000 - now) / 1000))
    spacing: 10
    Text {
        text: Math.floor(component.seconds / 60).toString().padStart(2, "0") + ":" + (component.seconds % 60).toString().padStart(2, "0")
        color: component.theme.accent
        font { family: component.theme.font; pixelSize: component.theme.fontSize + 8; bold: true }
    }
    Text {
        Layout.fillWidth: true
        text: component.seconds === 0 ? (component.notice.data.doneLabel || "Ready") : (component.notice.data.label || "Remaining")
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: component.notice.urgency === "critical" ? component.theme.text : component.theme.muted
        font { family: component.theme.font; pixelSize: component.theme.fontSize - 1 }
    }
}
