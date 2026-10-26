import QtQuick
import Quickshell
import Quickshell.Wayland

// Keep the native surface fixed. Only this card and its input region animate.
PanelWindow {
    id:surface
    required property var panelHost
    property real contentProgress:1
    property var transitionImage:null
    property real transitionWidth:0
    property real transitionHeight:0
    required property var modelData
    screen:modelData
    visible:true
    color:'transparent'
    anchors {top:true;right:true;bottom:true;left:true}
    exclusionMode:ExclusionMode.Ignore
    WlrLayershell.namespace:'bartjik-panel-shell'
    WlrLayershell.layer:WlrLayer.Overlay
    WlrLayershell.keyboardFocus:active && panelHost.grabFocus?WlrKeyboardFocus.OnDemand:WlrKeyboardFocus.None
    readonly property bool active:panelHost.panelOpen && !panelHost.detached && panelHost.panelScreen===screen
    readonly property Item frame:card
    readonly property Item bodySlot:slot
    // A handoff keeps animating while the new body is reparented into the slot.
    readonly property bool animate:active && !panelHost.dragging && (panelHost.panelBody?.parent===slot || !!surface.transitionImage)
    mask:Region {
        x:Math.floor(card.x);y:Math.floor(card.y)
        width:surface.active?Math.ceil(card.width):0
        height:surface.active?Math.ceil(card.height):0
        radius:12
    }
    Item {
        id:card
        visible:surface.active
        clip:true
        width:surface.panelHost.implicitWidth;height:surface.panelHost.implicitHeight
        readonly property string side:surface.panelHost.anchor.window?.bar.side || 'b'
        property real edgeX:surface.panelHost.anchor.rect.x
        property real edgeY:surface.panelHost.anchor.rect.y
        property real centerX:surface.panelHost.anchor.rect.x+surface.panelHost.implicitWidth/2
        property real centerY:surface.panelHost.anchor.rect.y+surface.panelHost.implicitHeight/2
        x:surface.panelHost.clamp(side==='r'?edgeX-width:side==='l'?edgeX:centerX-width/2,width,surface.width)
        y:surface.panelHost.clamp(side==='b'?edgeY-height:side==='t'?edgeY:centerY-height/2,height,surface.height)
        Behavior on width {enabled:surface.animate;NumberAnimation {duration:140;easing.type:Easing.OutCubic}}
        Behavior on height {enabled:surface.animate;NumberAnimation {duration:140;easing.type:Easing.OutCubic}}
        Behavior on edgeX {enabled:surface.animate;NumberAnimation {duration:surface.panelHost.positionDuration ?? 140;easing.type:Easing.OutCubic}}
        Behavior on edgeY {enabled:surface.animate;NumberAnimation {duration:surface.panelHost.positionDuration ?? 140;easing.type:Easing.OutCubic}}
        Behavior on centerX {enabled:surface.animate;NumberAnimation {duration:surface.panelHost.positionDuration ?? 140;easing.type:Easing.OutCubic}}
        Behavior on centerY {enabled:surface.animate;NumberAnimation {duration:surface.panelHost.positionDuration ?? 140;easing.type:Easing.OutCubic}}
        Rectangle {anchors.fill:parent;radius:12;color:surface.panelHost.theme.background;opacity:surface.panelHost.panelBody?.opacity ?? 1
            border.width:1;border.color:surface.panelHost.theme.panelBorder || surface.panelHost.theme.border}
        Item {id:slot;anchors.fill:parent;opacity:surface.contentProgress;enabled:surface.contentProgress>=1}
        Image {
            width:surface.transitionWidth;height:surface.transitionHeight
            source:surface.transitionImage?surface.transitionImage.url:''
            visible:!!surface.transitionImage;opacity:1-surface.contentProgress
        }
    }
}
