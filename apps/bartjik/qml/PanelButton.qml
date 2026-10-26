import QtQuick
import QtQuick.Controls

Button {
    id:control
    required property var theme
    property string symbol:''
    property string imageSource:''
    property bool showLabel:false
    property bool accented:false
    property bool iconOnlyHighlight:false
    property bool toolTipEnabled:true
    implicitWidth:showLabel?label.implicitWidth+24+(symbol || imageSource?26:0):32
    implicitHeight:32
    padding:6;hoverEnabled:true;focusPolicy:Qt.NoFocus
    opacity:enabled?1:.4
    Accessible.name:text
    background:Rectangle {
        radius:8
        color:control.iconOnlyHighlight?control.theme.surface:control.down?Qt.tint(control.theme.surface,Qt.rgba(1,1,1,.09)):control.hovered?Qt.tint(control.theme.surface,Qt.rgba(1,1,1,.04)):control.theme.surface
        border.width:!control.iconOnlyHighlight && (control.hovered || control.down)?1:0
        border.color:control.down?control.theme.accent:control.theme.border
    }
    contentItem:Item {
        Row {
            anchors.centerIn:parent;spacing:6
            Item {
                width:20;height:20;visible:!!control.symbol || !!control.imageSource
                Image {anchors.fill:parent;visible:!!control.imageSource;source:control.imageSource;fillMode:Image.PreserveAspectFit;sourceSize:Qt.size(40,40)}
                Text {anchors.centerIn:parent;visible:!control.imageSource;text:control.symbol;font.family:'Material Symbols Rounded';font.pixelSize:20;color:control.accented || (control.iconOnlyHighlight && (control.hovered || control.down))?control.theme.accent:control.theme.text}
            }
            Text {id:label;visible:control.showLabel;text:control.text;font.family:control.theme.font;font.pixelSize:12;color:control.theme.text;anchors.verticalCenter:parent.verticalCenter;textFormat:Text.PlainText}
        }
    }
    PanelToolTip {
        theme:control.theme
        visible:control.hovered && control.enabled && control.toolTipEnabled && !control.showLabel && !!control.text
        text:control.text
    }
}
