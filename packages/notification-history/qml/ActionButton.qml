import QtQuick
import QtQuick.Controls

// Quiet, shared notification actions; keep keyboard focus and wrapped labels clear.
Button {
    id: button
    required property var theme
    property int labelSize: theme.fontSize - 2
    property color disabledColor: theme.border
    implicitWidth: Math.ceil(Math.max(label.implicitWidth, metrics.boundingRect(text).width)) + leftPadding + rightPadding
    implicitHeight: Math.max(26, contentItem.implicitHeight + topPadding + bottomPadding)
    padding: 2
    leftPadding: 2
    rightPadding: 2
    topPadding: 2
    bottomPadding: 2
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    Accessible.name: text
    FontMetrics { id: metrics; font { family: button.theme.font; pixelSize: button.labelSize } }
    contentItem: Text {
        id: label
        text: button.text
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        color: !button.enabled ? button.disabledColor : button.hovered || button.visualFocus ? button.theme.text : button.theme.muted
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignHCenter
        font { family: button.theme.font; pixelSize: button.labelSize }
    }
    background: Rectangle {
        radius: 4
        color: button.down ? button.theme.surface : "transparent"
        border.width: button.visualFocus ? 1 : 0
        border.color: button.theme.accent
    }
}
