import QtQuick
import QtQuick.Controls
import Quickshell

Item {
    id:strip
    objectName:'offscreen-windows'
    required property var windows
    required property var theme
    required property string side
    signal activated(string address)
    implicitWidth:Math.max(0,windows.length*28-4)
    implicitHeight:24
    Row {
        anchors.centerIn:parent
        spacing:4
        Repeater {
            model:strip.windows
            delegate:PanelButton {
                id:app
                objectName:'offscreen-app-button'
                required property var modelData
                readonly property var desktop:DesktopEntries.heuristicLookup(modelData.app) || DesktopEntries.heuristicLookup(modelData.app.toLowerCase())
                readonly property string iconName:desktop?.icon || modelData.app
                readonly property string iconSource:Quickshell.iconPath(iconName,true) || Quickshell.iconPath('application-x-executable')
                theme:strip.theme
                text:modelData.title || desktop?.name || modelData.app
                enabled:/^0x[0-9a-fA-F]+$/.test(modelData.address || '')
                implicitWidth:24;implicitHeight:24;padding:2
                background:Rectangle {
                    radius:5
                    color:app.down ? Qt.tint(strip.theme.surface,Qt.rgba(1,1,1,.12)) : app.hovered ? Qt.tint(strip.theme.surface,Qt.rgba(1,1,1,.06)) : 'transparent'
                }
                contentItem:Image {
                    objectName:'offscreen-app-icon'
                    source:app.iconSource
                    sourceSize.width:40;sourceSize.height:40
                    fillMode:Image.PreserveAspectFit;smooth:true
                }
                onClicked:strip.activated(modelData.address)
            }
        }
    }
}
