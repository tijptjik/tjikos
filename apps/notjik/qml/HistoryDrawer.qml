import QtQuick
import QtQuick.Window
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import "Logic.js" as Logic

PanelWindow {
    id: drawer
    required property var controller
    readonly property var theme: controller.theme
    readonly property var targetScreen: controller.screens.find(s => s.name === controller.drawerMonitor) || controller.screens[0] || null
    readonly property var placement: targetScreen ? Logic.placement(controller.config, targetScreen, Quickshell.screens.indexOf(targetScreen)) : ({})
    readonly property var coordinates: targetScreen ? Logic.geometry(Object.assign({}, placement, {edge: "b"}),
        targetScreen.width, targetScreen.height, width, height, targetScreen.devicePixelRatio) : ({y: 0})
    screen: targetScreen
    visible: controller.ready && controller.drawerOpen && !controller.locked && !!targetScreen
    anchors { bottom: true; left: true }
    implicitWidth: Math.min((theme.width || 420) + 80, (screen?.width || 800) - 32)
    implicitHeight: Math.min(760, (screen?.height || 1000) * 0.72)
    margins { bottom: targetScreen ? targetScreen.height - height - coordinates.y : 16; left: Math.max(0, ((screen?.width || 800) - width) / 2) }
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.namespace: "notjik-history"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None
    property bool hadWindowFocus:false
    function focusHistory(){historyView.focusHistory();}
    function handleEscape(){historyView.handleEscape();}
    Connections {
        target: drawer.contentItem.Window.window
        function onActiveChanged() {
            if (target.active) drawer.hadWindowFocus = true;
            else if (drawer.visible && drawer.hadWindowFocus) drawer.controller.setDrawer(false);
        }
    }
    mask: Region { item: backdrop; radius: backdrop.radius }
    onVisibleChanged: {
        hadWindowFocus=false;
        if(visible)Qt.callLater(()=>historyView.focusHistory());
    }
    Shortcut {sequence:"Escape";enabled:drawer.visible;onActivated:drawer.handleEscape()}
    Rectangle {
        id:backdrop;anchors.fill:parent;radius:12;color:drawer.theme.background
        border.width:1;border.color:drawer.theme.border
        Item {
            x:16;y:16;width:parent.width-32;height:32
            Image {width:22;height:22;anchors.verticalCenter:parent.verticalCenter;source:"history-bell.svg"}
            Text {x:32;anchors.verticalCenter:parent.verticalCenter;text:"Notifications";color:drawer.theme.text;font.family:drawer.theme.font;font.pixelSize:16;font.weight:Font.DemiBold}
            Button {
                id:closeButton
                anchors.right:parent.right;width:32;height:32;padding:6;hoverEnabled:true
                Accessible.name:"Close notification history"
                background:Rectangle {
                    radius:8;color:drawer.theme.surface
                    border.width:closeButton.visualFocus?1:0;border.color:drawer.theme.accent
                }
                contentItem:Text {
                    text:"close";font.family:"Material Symbols Rounded";font.pixelSize:20
                    horizontalAlignment:Text.AlignHCenter;verticalAlignment:Text.AlignVCenter
                    color:closeButton.hovered || closeButton.down?drawer.theme.accent:drawer.theme.text
                }
                onClicked:drawer.controller.setDrawer(false)
            }
        }
        Rectangle {x:16;y:58;width:parent.width-32;height:1;color:drawer.theme.surface}
        HistoryView {
            id:historyView;x:16;y:72;width:parent.width-32;height:parent.height-88
            controller:drawer.controller;monitorName:drawer.screen?.name || ''
            onCloseRequested:drawer.controller.setDrawer(false)
        }
    }
}
