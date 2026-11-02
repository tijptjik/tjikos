import QtQuick
import QtQuick.Controls

Button {
    id:control
    property var theme
    implicitWidth:26;implicitHeight:26;padding:0
    hoverEnabled:true
    Accessible.name:'Add subtask'
    contentItem:Item {
        Canvas {
            anchors.centerIn:parent;width:12;height:12
            property color ink:!control.enabled?control.theme.border:control.hovered || control.visualFocus?control.theme.accent:control.theme.muted
            onInkChanged:requestPaint()
            onPaint:{const c=getContext('2d');c.reset();c.strokeStyle=ink;c.lineWidth=1.5;c.lineCap='round';c.beginPath();c.moveTo(6,2);c.lineTo(6,10);c.moveTo(2,6);c.lineTo(10,6);c.stroke();}
        }
    }
    background:Rectangle {
        radius:7
        color:Qt.alpha(control.theme.accent,!control.enabled?0:control.down?0.18:control.hovered?0.09:0)
        border.width:1;border.color:control.visualFocus?Qt.alpha(control.theme.accent,0.65):'transparent'
        Behavior on color {ColorAnimation {duration:100}}
    }
    TaskToolTip {theme:control.theme;visible:control.hovered;text:'Add subtask'}
}
