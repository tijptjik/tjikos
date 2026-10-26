import QtQuick
import "Logic.js" as Logic

Rectangle {
    id: island
    required property var definition
    required property var bar
    required property var controller
    required property var window
    required property bool horizontal
    readonly property var rawTheme: Logic.merge(bar.style,definition.style || {})
    readonly property var theme: horizontal ? rawTheme : Logic.merge(rawTheme,{maxTextWidth:Math.max(0,Math.min(rawTheme.maxTextWidth,bar.verticalWidth-2*rawTheme.padding-rawTheme.iconSize-rawTheme.iconTextGap))})
    readonly property real paddingX: theme.paddingX ?? theme.padding
    readonly property real paddingY: theme.paddingY ?? theme.padding
    readonly property bool populated: definition.modules.some(id=>controller.displayItems(bar,id).some(i=>i.visible!==false))
    visible: populated
    implicitWidth: !populated ? 0 : horizontal ? modules.implicitWidth+2*paddingX : bar.verticalWidth
    implicitHeight: !populated ? 0 : horizontal ? Math.max(bar.thickness,modules.implicitHeight+2*paddingY) : modules.implicitHeight+2*paddingY
    clip: !horizontal
    radius: theme.radius
    color: theme.background
    border {color: island.theme.border; width: island.theme.borderWidth}
    function previewTargets() {
        return [].concat(...modules.children.filter(m=>m.moduleId!==undefined).map(m=>m.previewTargets(island)));
    }
    PreviewHover {island:parent}
    ListModel {id:moduleKeys}
    onDefinitionChanged: Logic.sync(moduleKeys,definition.modules)
    Component.onCompleted: Logic.sync(moduleKeys,definition.modules)
    // Parts handle their own bindings; this fills the spaces between them and outer padding.
    MouseArea {
        anchors.fill:parent
        acceptedButtons:Qt.MiddleButton
        enabled:island.definition.modules.some(id=>island.bar.moduleConfigs[id]?.provider==='playerctl')
        property var target:null
        onPressed:event=>{
            target=Logic.nearestTarget(island.previewTargets(),event.x,event.y);
            event.accepted=!!target && target.provider==='playerctl' && target.row.enabled!==false
                && !!Logic.choose(island.bar.moduleConfigs[target.context.module],target.row,'item','middleClick');
        }
        onClicked:event=>{
            if(!target)return;
            const action=Logic.choose(island.bar.moduleConfigs[target.context.module],target.row,'item','middleClick');
            const origin=island.mapToItem(island.window.contentItem,target.x,target.y);
            const context=Object.assign({},target.context,{event:'middleClick',anchor:{x:origin.x,y:origin.y,width:target.width,height:target.height}});
            if(action)island.controller.act(action,context,island.window,target.row);
            target=null;
        }
        onCanceled:target=null
        onWheel:event=>{event.accepted=false;}
    }
    Grid {
        id: modules
        x: island.paddingX
        y: (island.height-height)/2
        verticalItemAlignment: Grid.AlignVCenter
        columns: island.horizontal ? Math.max(1,island.definition.modules.filter(id=>island.controller.displayItems(island.bar,id).some(row=>row.visible!==false)).length) : 1
        columnSpacing: island.theme.moduleGap
        rowSpacing: island.theme.moduleGap
        Repeater {
            model: moduleKeys
            delegate: ModuleView {
                required property string key
                moduleId: key
                bar: island.bar
                theme: island.theme
                horizontal: island.horizontal
                controller: island.controller
                window: island.window
            }
        }
    }
}
