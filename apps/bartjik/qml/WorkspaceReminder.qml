import QtQuick
import Quickshell
import Quickshell.Wayland
import "Logic.js" as Logic

// Reuse the configured workspace modules and styling, without a hover/input area.
PanelWindow {
    id: panel
    required property var controller
    required property string barKey
    readonly property var bar: controller.bars.find(b=>b.key===barKey)
    readonly property bool horizontal: bar?.side==='t' || bar?.side==='b'
    readonly property var definitions: (bar?.islands || []).map(i=>Object.assign({},i,{modules:i.modules.filter(id=>bar.moduleConfigs[id]?.provider==='workspaces')})).filter(i=>i.modules.length)
    readonly property var geometry: bar && screen ? Logic.geometry(bar,screen.width,screen.height,groups.implicitWidth,groups.implicitHeight,screen.devicePixelRatio) : {x:0,y:0,width:0,height:0}
    readonly property var targetScreen: Quickshell.screens.find(s=>s.name===bar?.screen) || null
    screen: targetScreen
    visible: !!targetScreen && (controller.workspaceReminder || groups.opacity>0)
    color: 'transparent'
    anchors {top:true;left:true;right:true;bottom:true}
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.namespace: 'bartjik-workspace-reminder'
    WlrLayershell.layer: WlrLayer.Overlay
    mask: Region {}
    Grid {
        id: groups
        x: panel.geometry.x; y: panel.geometry.y
        columns: panel.horizontal ? Math.max(1,panel.definitions.length) : 1
        spacing: panel.bar?.islandGap || 0
        opacity: panel.controller.workspaceReminder ? 1 : 0
        Behavior on opacity {NumberAnimation {duration:180;easing.type:Easing.InOutQuad}}
        Repeater {
            model: panel.definitions
            Island {
                required property var modelData
                definition: modelData
                bar: panel.bar
                horizontal: panel.horizontal
                controller: panel.controller
                window: panel
            }
        }
    }
}
