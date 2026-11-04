import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id:confirmation
    required property var theme
    signal accepted()
    signal cancelled()
    visible:false
    implicitWidth:Math.min(420,screen.width-24);implicitHeight:190
    exclusionMode:ExclusionMode.Ignore;color:'transparent'
    WlrLayershell.namespace:'bartjik-power-confirmation'
    WlrLayershell.layer:WlrLayer.Overlay
    WlrLayershell.keyboardFocus:visible?WlrKeyboardFocus.Exclusive:WlrKeyboardFocus.None
    onVisibleChanged:if(visible)Qt.callLater(()=>body.forceActiveFocus())
    Rectangle {
        id:body;anchors.fill:parent;radius:12;color:confirmation.theme.background
        border.width:1;border.color:confirmation.theme.accent;focus:true
        Keys.onEscapePressed:confirmation.cancelled()
        ColumnLayout {
            anchors.fill:parent;anchors.margins:20;spacing:12
            Text {text:'Power off now?';font.family:confirmation.theme.font;font.pixelSize:16;color:confirmation.theme.text;Layout.fillWidth:true;horizontalAlignment:Text.AlignHCenter}
            Text {text:'This closes your apps and turns off the computer. Save your work before continuing.';font.family:confirmation.theme.font;font.pixelSize:12;color:confirmation.theme.muted;Layout.fillWidth:true;wrapMode:Text.Wrap;horizontalAlignment:Text.AlignHCenter}
            RowLayout {
                Layout.alignment:Qt.AlignHCenter;spacing:12
                PanelButton {objectName:'power-cancel';theme:confirmation.theme;showLabel:true;text:'Cancel';onClicked:confirmation.cancelled()}
                PanelButton {objectName:'power-confirm';theme:confirmation.theme;showLabel:true;text:'Power off now';accented:true;onClicked:confirmation.accepted()}
            }
        }
    }
}
