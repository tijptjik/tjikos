import QtQuick
import QtQuick.Controls

ToolTip {
    id:tip
    required property var theme
    delay:650
    padding:8
    leftPadding:10
    rightPadding:10
    margins:8
    implicitWidth:Math.min(280,implicitContentWidth+leftPadding+rightPadding)
    font.family:theme.font
    font.pixelSize:12
    contentItem:Text {
        text:tip.text
        textFormat:Text.PlainText
        font:tip.font
        color:tip.theme.text
        wrapMode:Text.Wrap
    }
    background:Rectangle {
        radius:8
        color:tip.theme.background
        border.width:1
        border.color:tip.theme.panelBorder || tip.theme.border
    }
}
