import QtQuick
import "Logic.js" as Logic

// One passive hover region covers every item and all island padding.
HoverHandler {
    id: root
    blocking: false
    required property var island
    property var current: null
    function leave() {
        if(!current)return;
        if(current.provider==='crypto')island.controller.cryptoLeave(current.context);
        else if(current.provider==='workspaces')island.controller.workspaceLeave(current.context);
        else island.controller.detailLeave(current.context);
        current=null;
    }
    function move(x,y) {
        const target=Logic.nearestTarget(island.previewTargets(),x,y);
        if(!target || !target.enabled){leave();return;}
        const p=island.mapToItem(island.window.contentItem,x,y);
        const origin=island.mapToItem(island.window.contentItem,0,0);
        const context=Object.assign({},target.context,{anchor:{x:p.x,y:p.y,width:0,height:0},hoverRegion:{x:origin.x,y:origin.y,width:island.width,height:island.height}});
        if(current && (current.context.module!==context.module || current.context.item!==context.item))leave();
        current={context:context,provider:target.provider,row:target.row};
        if(target.provider==='crypto')island.controller.cryptoEnter(context,island.window,target.row,target.options);
        else if(target.provider==='workspaces')island.controller.workspaceEnter(context,island.window,target.row,target.options);
        else island.controller.detailEnter(context,island.window,target.row,target.options);
    }
    onHoveredChanged: {if(hovered)root.move(point.position.x,point.position.y);else root.leave();}
    onPointChanged: if(hovered)root.move(point.position.x,point.position.y)
    Component.onDestruction: leave()
}
