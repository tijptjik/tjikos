import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: reserve
    required property var controller
    required property string key
    readonly property string connector: key.split('/')[0]
    readonly property string side: key.split('/')[1]
    readonly property real depth: controller.reserveDepth(connector,side)
    readonly property var targetScreen: Quickshell.screens.find(s=>s.name===connector) || null
    screen: targetScreen
    visible: depth>0 && !!targetScreen
    color: 'transparent'
    anchors {top:reserve.side==='t' || reserve.side==='l' || reserve.side==='r'; bottom:reserve.side==='b' || reserve.side==='l' || reserve.side==='r'; left:reserve.side==='l' || reserve.side==='t' || reserve.side==='b';right:reserve.side==='r' || reserve.side==='t' || reserve.side==='b'}
    implicitWidth: 1; implicitHeight: 1
    exclusiveZone: Math.ceil(depth)
    exclusionMode: ExclusionMode.Normal
    mask: Region {}
    WlrLayershell.namespace: 'bartjik-reservation'
    WlrLayershell.layer: WlrLayer.Top
}
