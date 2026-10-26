import QtQuick

PanelButton {
    required property var popup
    symbol:'close'
    iconOnlyHighlight:true
    text:'Close'
    onClicked:popup.dismissed()
}
