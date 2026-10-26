import QtQuick
import Quickshell
import Quickshell.Wayland
import "Logic.js" as Logic

PanelWindow {
    id: panel
    required property var controller
    required property string barKey
    readonly property var bar: controller.bars.find(b=>b.key===barKey)
    readonly property var populatedIslands: (bar?.islands || []).filter(i=>i.modules.some(id=>controller.items(bar,id).some(row=>row.visible!==false))).map(i=>i.id)
    readonly property bool horizontal: bar ? bar.side==='t' || bar.side==='b' : true
    readonly property var geometry: bar && targetScreen ? Logic.geometry(bar,targetScreen.width,targetScreen.height,groups.implicitWidth,groups.implicitHeight,targetScreen.devicePixelRatio) : {x:0,y:0,width:0,height:0}
    readonly property bool concealed: controller.edgeStates[barKey]?.concealed || false
    readonly property bool autoHide: bar?.autoHide || false
    readonly property bool edgeHovered: hover.hovered || !!controller.menuOpen(barKey)
    readonly property bool edgeFading: viewport.opacity>0
    readonly property var targetScreen: Quickshell.screens.find(s=>s.name===bar?.screen) || null
    screen: targetScreen
    visible: !!bar && controller.shown(bar) && !!targetScreen
    color: 'transparent'
    anchors {top:true;left:true;right:true;bottom:true}
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.namespace: 'bartjik-'+(bar?.id || '')
    WlrLayershell.layer: bar?.layer==='overlay'?WlrLayer.Overlay:bar?.layer==='bottom'?WlrLayer.Bottom:bar?.layer==='background'?WlrLayer.Background:WlrLayer.Top
    mask: Region {
        regions: maskRegions.instances
    }
    Variants {
        id: maskRegions
        model: viewport.opacity>0.01 ? panel.populatedIslands : []
        Region {
            required property string modelData
            readonly property var island: groups.children.find(c=>c.islandId===modelData)
            readonly property real left: island ? Math.max(0,island.x) : 0
            readonly property real top: island ? Math.max(0,island.y) : 0
            x: viewport.x+left
            y: viewport.y+top
            width: island ? Math.max(0,Math.min(island.width,viewport.width-left)) : 0
            height: island ? Math.max(0,Math.min(island.height,viewport.height-top)) : 0
            radius: (panel.bar?.style.radius || 0)
        }
    }
    ListModel {id:islandKeys}
    onBarChanged: {
        Logic.sync(islandKeys,(bar?.islands || []).map(i=>i.id));
    }
    Item {
        id: viewport
        x: panel.geometry.x; y: panel.geometry.y
        width: panel.geometry.width; height: panel.geometry.height
        clip: true
        opacity: panel.concealed ? 0 : 1
        visible: opacity>0
        Behavior on opacity {NumberAnimation {duration:panel.bar?.fadeDuration ?? 180;easing.type:Easing.InOutQuad}}
        Grid {
            id: groups
            columns: panel.horizontal ? Math.max(1,panel.populatedIslands.length) : 1
            spacing: panel.bar?.islandGap || 0
            Repeater {
                model: islandKeys
                delegate: Island {
                    required property string key
                    property string islandId: key
                    definition: panel.bar.islands.find(i=>i.id===key) || {modules:[]}
                    bar: panel.bar
                    horizontal: panel.horizontal
                    controller: panel.controller
                    window: panel
                }
            }
        }
        HoverHandler {id: hover;onHoveredChanged:controller.updateEdges()}
    }
    function capture(path) {viewport.grabToImage(result=>result.saveToFile(path));}
    function inspect() {return {key:barKey,geometry:geometry,concealed:concealed,opacity:viewport.opacity,inputRegions:maskRegions.instances.length,visible:visible,islands:groups.children.filter(c=>c.islandId).map(c=>({id:c.islandId,x:c.x,y:c.y,width:c.width,height:c.height,visible:c.visible}))};}
}
