import QtQuick
import QtQuick.Controls

Button {
    id:control
    required property var theme
    required property string country
    property bool active:false
    implicitWidth:72
    implicitHeight:54
    padding:2
    hoverEnabled:true
    focusPolicy:Qt.NoFocus
    Accessible.name:text
    Accessible.checkable:true
    Accessible.checked:active
    scale:down?.97:1
    Behavior on scale {NumberAnimation {duration:90}}
    background:Rectangle {
        radius:3
        color:control.theme.surface
        border.width:control.hovered?2:1
        border.color:control.hovered?control.theme.border:control.theme.panelBorder || control.theme.border
    }
    contentItem:Image {
        opacity:.84
        source:'icons/flags/'+control.country+'.svg'
        fillMode:Image.PreserveAspectFit
        sourceSize:Qt.size(144,108)
        horizontalAlignment:Image.AlignHCenter
        verticalAlignment:Image.AlignVCenter
    }
}
