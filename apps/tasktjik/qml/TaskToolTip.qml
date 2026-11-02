import QtQuick
import QtQuick.Controls

ToolTip {
    id:tip;objectName:'task-tooltip'
    property var theme
    delay:550;timeout:4500
    padding:9;leftPadding:11;rightPadding:11
    width:Math.min(320,contentItem.implicitWidth+leftPadding+rightPadding)
    contentItem:Text {text:tip.text;textFormat:Text.PlainText;wrapMode:Text.Wrap;font.pixelSize:12;color:tip.theme.text}
    background:Rectangle {radius:7;color:tip.theme.surface;border.color:tip.theme.border;border.width:1}
    enter:Transition {NumberAnimation {property:'opacity';from:0;to:1;duration:100}}
    exit:Transition {NumberAnimation {property:'opacity';to:0;duration:80}}
}
