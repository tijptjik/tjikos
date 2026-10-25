import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: component
    required property var notice
    required property var theme
    required property double now
    spacing: 8
    RowLayout {
        Layout.fillWidth: true
        Text {
            Layout.fillWidth: true
            text: component.notice.data.label || "Progress"
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: component.notice.urgency === "critical" ? component.theme.text : component.theme.muted
            font { family: component.theme.font; pixelSize: component.theme.fontSize - 2 }
        }
        Text {
            text: Math.round(component.notice.data.value || 0) + "%"
            color: component.theme.accent
            font { family: component.theme.font; pixelSize: component.theme.fontSize }
        }
    }
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: 6
        radius: 3
        color: component.theme.surface
        Rectangle {
            height: parent.height
            width: parent.width * Math.max(0, Math.min(100, component.notice.data.value || 0)) / 100
            radius: 3
            color: component.theme.accent
            Behavior on width { NumberAnimation { duration: 180 } }
        }
    }
}
