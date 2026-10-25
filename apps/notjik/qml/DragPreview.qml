import QtQuick
import Quickshell
import Quickshell.Wayland

// Only the initial drag uses a layer surface. The dropped card is a toplevel.
PanelWindow {
    id: preview
    required property var controller
    readonly property var position: controller.floatingPositions[controller.dragPreviewId]
    readonly property var notice: controller.find(controller.dragPreviewId)
    visible: controller.ready && !controller.locked && controller.dragPreviewId.length > 0
    anchors { left: true; right: true; top: true; bottom: true }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.namespace: "notjik-floating"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    mask: Region { width: 0; height: 0 }
    Loader {
        active: preview.controller.ready && !!preview.notice
        sourceComponent: NotificationCard {
            visible: preview.position?.screen === preview.screen.name
            x: (preview.position?.x || 0) - preview.screen.x
            y: (preview.position?.y || 0) - preview.screen.y
            width: preview.position?.width || 420
            height: implicitHeight
            notice: preview.notice || ({id:"",title:"",body:"",app:"",urgency:"normal",icon:"",image:"",actions:[],data:{},type:"standard"})
            controller: preview.controller
            monitorName: preview.screen.name
            floating: true
            Component.onCompleted: restoreState(controller.floatingStates[notice.id])
            enabled: false
        }
    }
}
