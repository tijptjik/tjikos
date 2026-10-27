import QtQuick
import QtQuick.Controls
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland
import "Logic.js" as Logic

DetachablePopup {
    id:popup
    detachedTitle:'Workspace preview'
    stableSurface:true
    panelShell:controller.panelShell
    panelBody:body
    panelOpen:opened
    dragFromBody:true
    onDetachedChanged:if(detached){hoverKey='';pending=null;delay.stop();hide.stop();}
    positionDuration:options.slideDuration ?? 120
    onDismissed:close()
    required property var controller
    property bool opened: false
    property string hoverKey: ''
    property string selectedKey: ''
    property var pending: null
    property var selected: null
    property var options: ({})
    property real anchorX: 0
    property real anchorY: 0
    readonly property bool horizontal: !selected?.window?.bar || selected.window.bar.side==='t' || selected.window.bar.side==='b'
    readonly property var theme: selected?.window?.bar.style || {panelBorder:'#393546',border:'#393546',surface:'#26233a',background:'#1f1d2e',text:'#e5e5e5',muted:'#a5a0ba',accent:'#ff53a6',font:'sans-serif'}
    readonly property var targetScreen: selected?.window?.screen || null
    readonly property real pixelScale: panelScreen?.devicePixelRatio || 1
    function snap(value) {return Math.round(value*pixelScale)/pixelScale;}
    readonly property var previewRow: selected ? controller.items(selected.window.bar,selected.context.module).find(r=>r.id===selected.row.id) || selected.row : null
    readonly property var offscreenWindows: previewRow?.offscreen || {left:[],right:[]}
    readonly property bool hasOffscreen: offscreenWindows.left.length>0 || offscreenWindows.right.length>0
    readonly property var monitor: targetScreen ? Hyprland.monitorFor(targetScreen) : null
    readonly property var sourceList: !opened ? [] : controller.demo ? [
        {address:'demo-editor',workspace:{name:String(selected.row.workspace || selected.row.id)},lastIpcObject:{at:[0,0],size:[targetScreen.width*.65,targetScreen.height],class:'Editor',focusHistoryID:1}},
        {address:'demo-terminal',workspace:{name:String(selected.row.workspace || selected.row.id)},lastIpcObject:{at:[targetScreen.width*.65,0],size:[targetScreen.width*.35,targetScreen.height],class:'Terminal',focusHistoryID:0}}
    ] : Hyprland.toplevels.values.filter(t=>t.workspace && t.monitor?.name===targetScreen?.name && (String(t.workspace.id)===selected.row.id || t.workspace.name===String(selected.row.workspace ?? selected.row.id) || t.lastIpcObject.pinned===true) && t.lastIpcObject.mapped!==false && t.lastIpcObject.hidden!==true)
        .sort((a,b)=>(a.lastIpcObject.focusHistoryID ?? 999)-(b.lastIpcObject.focusHistoryID ?? 999))
        .slice(0,options.maxWindows ?? 12).reverse()
    visible:false
    implicitWidth: Math.min(20+(implicitHeight-20)*(targetScreen?.width || 1920)/(targetScreen?.height || 1080),(targetScreen?.width || 1920)-24)
    implicitHeight: Logic.previewHeight(theme,targetScreen?.height)
    color:'transparent'
    grabFocus:false
    anchor.edges: Edges.Top | Edges.Left
    anchor.gravity: horizontal ? selected?.window?.bar.side==='t' ? Edges.Bottom|Edges.Right : Edges.Top|Edges.Right : selected?.window?.bar.side==='l' ? Edges.Bottom|Edges.Right : Edges.Bottom|Edges.Left
    anchor.adjustment: PopupAdjustment.Flip | PopupAdjustment.Slide
    anchor.rect.x: anchorX
    anchor.rect.y: anchorY
    anchor.rect.width:0
    anchor.rect.height:0
    function key(context) {return context.barKey+'/'+context.module+'/'+context.item+(context.part?'/'+context.part:'');}
    function enter(context,window,row,settings) {
        if(detached || dragging)return;
        const same=hoverKey===key(context);
        if(opened && selected?.window!==window)close();
        hide.stop();hoverKey=key(context);pending={context:context,window:window,row:row};options=settings;
        if(opened && selected?.window===window){
            panelShell.show(popup);
            delay.stop();
            if(same){selected=pending;locate(context.anchor.x+context.anchor.width/2,context.anchor.y+context.anchor.height/2);}
            else reveal();
        }
        else if(panelShell.canCross(window)){delay.stop();reveal();}
        else if(!same || !delay.running)delay.restart();
    }
    function leave(context) {
        if(key(context)!==hoverKey)return;
        hoverKey='';delay.stop();pending=null;hide.restart();
    }
    function reveal() {
        if(!pending?.window?.bar || !hoverKey)return;
        selected=pending;selectedKey=hoverKey;anchor.window=selected.window;
        locate(selected.context.anchor.x+selected.context.anchor.width/2,selected.context.anchor.y+selected.context.anchor.height/2);
        opened=true;
        if(!controller.demo)Hyprland.refreshToplevels();
    }
    function locate(x,y) {
        if(!selected || detached || dragging)return;
        const point=Logic.panelAnchor({x:x,y:y,width:0,height:0},selected.window.geometry,selected.window.bar.side,targetScreen.width,targetScreen.height,implicitWidth,implicitHeight);
        anchorX=point.x;anchorY=point.y;
    }
    function followPointer() {
        if(detached || dragging)return;
        const p=controller.pointer;
        const region=selected?.context.hoverRegion;
        if(opened && hoverKey && p.valid && p.monitor===targetScreen?.name && (!region || (p.x>=region.x && p.x<=region.x+region.width && p.y>=region.y && p.y<=region.y+region.height)))locate(p.x,p.y);
    }
    function pointerHeld() {
        const pos=attachedPosition(),side=selected?.window?.bar.side,w=body.width,h=body.height;
        return Logic.panelHeld(Date.now()-controller.pointerUpdated<1500?controller.pointer:null,targetScreen?.name,selected?.context.hoverRegion,
            {x:pos.x+(side==='r'?w:0),y:pos.y+(side==='b'?h:0)},w,h,side);
    }
    function focusOffscreen(address) {
        if(!/^0x[0-9a-fA-F]+$/.test(address))return;
        if(!controller.demo)Hyprland.dispatch('hl.dsp.focus({ window = "address:'+address+'" })');
        close();
    }
    function close() {opened=false;resetDetach();delay.stop();hide.stop();selectedKey='';hoverKey='';pending=null;}
    function capture(path) {body.grabToImage(result=>result.saveToFile(path));}
    function inspect() {return {card:{x:attachedPosition().x,y:attachedPosition().y,width:body.width,height:body.height},opened:panelVisible,detached:detached,dragging:dragging,placement:placement(),sourceMonitor:targetScreen?.name || '',width:implicitWidth,height:implicitHeight,desktopWidth:desktop.width,desktopHeight:desktop.height,key:selectedKey,pending:delay.running,anchorX:anchorX,anchorY:anchorY,windows:sourceList.length,captured:thumbs.children.filter(c=>c.captured===true).length,
        offscreen:{visible:offscreenInset.visible,x:offscreenInset.x,y:offscreenInset.y,width:offscreenInset.width,height:offscreenInset.height,left:offscreenWindows.left.length,right:offscreenWindows.right.length,
            notches:offscreenInset.children.filter(c=>c.objectName?.startsWith('workspace-offscreen-notch-')).map(c=>({side:c.modelData,visible:c.visible,x:c.x,width:c.width}))}};}
    Connections {
        target:popup.controller
        function onPointerChanged(){popup.followPointer();}
        function onBarsChanged(){if(popup.selected && !popup.controller.bars.some(b=>b.key===popup.selected.context.barKey))popup.close();}
        function onVisibilityChanged(){if(!popup.detached && !popup.dragging && popup.selected?.window?.bar && !popup.controller.shown(popup.selected.window.bar))popup.close();}
    }
    Timer {id:delay;interval:popup.options.delay ?? 300;onTriggered:popup.reveal()}
    Timer {id:hide;repeat:true;interval:Logic.panelCloseDelay(popup.options);onTriggered:if(!popup.detached && !popup.dragging && !popup.panelShell.holding(popup) && !inside.hovered && !popup.hoverKey && !popup.pointerHeld())popup.close()}
    Timer {interval:1000;running:popup.opened && !popup.controller.demo;repeat:true;onTriggered:Hyprland.refreshToplevels()}
    Component.onDestruction:panelShell.release(popup)
    onSourceListChanged:Logic.sync(windowKeys,sourceList.map(t=>t.address))
    ListModel {id:windowKeys}
    Rectangle {
        id:body;parent:popup.bodyParent;opacity:popup.bodyOpacity;anchors.fill:parent;color:popup.theme.surface;radius:12
        border.width:1;border.color:popup.theme.panelBorder || popup.theme.border || "#393546"
        HoverHandler {id:inside;onHoveredChanged:{if(hovered)hide.stop();else if(!popup.hoverKey)hide.restart();}}
        Rectangle {
            id:desktop
            x:popup.snap((parent.width-width)/2);y:popup.snap((parent.height-height)/2)
            width:popup.snap(Math.min(parent.width-20,(parent.height-20)*(popup.targetScreen?.width || 1920)/(popup.targetScreen?.height || 1080)))
            height:popup.snap(width*(popup.targetScreen?.height || 1080)/(popup.targetScreen?.width || 1920));color:popup.theme.background;clip:true;radius:4
            Item {
                id:thumbs;anchors.fill:parent
                Repeater {
                    model:windowKeys
                    delegate: Rectangle {
                        id:thumb
                        required property string key
                        readonly property var source: popup.sourceList.find(t=>t.address===key)
                        readonly property var clientData: source?.lastIpcObject || {}
                        readonly property var rect: Logic.previewRect(clientData, popup.controller.demo ? {x:0,y:0} : popup.monitor || {x:0,y:0},popup.targetScreen?.width || 1,popup.targetScreen?.height || 1,desktop.width,desktop.height,popup.pixelScale)
                        readonly property bool captured: stream.hasContent
                        x:rect.x;y:rect.y;width:rect.width;height:rect.height
                        color:popup.controller.demo?(key==='demo-editor'?'#2b293c':'#17151f'):popup.theme.surface
                        border.width:0;clip:true
                        ScreencopyView {
                            id:stream;anchors.fill:parent
                            captureSource:popup.opened && !popup.controller.demo ? thumb.source?.wayland || null : null
                            live:false;paintCursor:false
                            // Preserve source detail in the offscreen texture, then use
                            // mipmaps for minification instead of undersampling fine lines.
                            smooth:true
                            layer.enabled:hasContent
                            layer.textureSize:Qt.size(Math.max(1,sourceSize.width),Math.max(1,sourceSize.height))
                            layer.mipmap:true
                            layer.smooth:true
                        }
                        Timer {interval:popup.options.refreshInterval ?? 50;running:popup.opened && !!stream.captureSource && stream.hasContent;repeat:true;onTriggered:stream.captureFrame()}
                        Text {
                            anchors.fill:parent;anchors.margins:6
                            visible:popup.controller.demo || !stream.hasContent
                            text:popup.controller.demo ? thumb.clientData.class+'\n\n'+(thumb.key==='demo-editor'?'function hello() {\n  return "preview";\n}':'$ build\n✓ complete') : thumb.clientData.class || 'Preview unavailable'
                            textFormat:Text.PlainText;color:popup.theme.text;font.family:'monospace';font.pixelSize:Math.max(7,desktop.width/40);wrapMode:Text.Wrap;elide:Text.ElideRight
                        }
                    }
                }
            }
            Text {anchors.centerIn:parent;visible:popup.sourceList.length===0;text:'desktop_windows';color:popup.theme.muted;font.family:'Material Symbols Rounded';font.pixelSize:36;Accessible.name:'Empty workspace'}
        }
        Item {
            id:offscreenInset
            objectName:'workspace-offscreen-inset'
            visible:popup.hasOffscreen
            x:desktop.x;y:desktop.y+desktop.height-height
            width:desktop.width;height:36
            Repeater {
                model:['left','right']
                delegate:Item {
                    id:notch
                    required property string modelData
                    objectName:'workspace-offscreen-notch-'+modelData
                    readonly property bool onLeft:modelData==='left'
                    readonly property var windows:popup.offscreenWindows[modelData]
                    visible:windows.length>0
                    width:Math.min(offscreenInset.width/2-20,offscreenIcons.implicitWidth+24)
                    height:offscreenInset.height
                    x:onLeft ? offscreenInset.width/2-10-width : offscreenInset.width/2+10
                    Shape {
                        anchors.fill:parent
                        preferredRendererType:Shape.CurveRenderer
                        ShapePath {
                            strokeWidth:-1;fillColor:popup.theme.surface
                            startX:-8;startY:notch.height
                            PathQuad {x:0;y:notch.height-8;controlX:0;controlY:notch.height}
                            PathLine {x:0;y:10}
                            PathQuad {x:10;y:0;controlX:0;controlY:0}
                            PathLine {x:notch.width-10;y:0}
                            PathQuad {x:notch.width;y:10;controlX:notch.width;controlY:0}
                            PathLine {x:notch.width;y:notch.height-8}
                            PathQuad {x:notch.width+8;y:notch.height;controlX:notch.width;controlY:notch.height}
                            PathLine {x:-8;y:notch.height}
                        }
                    }
                    OffscreenWindows {
                        id:offscreenIcons
                        anchors.centerIn:parent
                        width:implicitWidth;height:implicitHeight
                        scale:Math.min(1,(notch.width-24)/Math.max(1,implicitWidth))
                        windows:notch.windows
                        side:notch.modelData
                        theme:popup.theme
                        onActivated:address=>popup.focusOffscreen(address)
                    }
                }
            }
        }
        PanelHandle {
            visible:popup.detached;popup:popup;theme:popup.theme
            anchors.top:desktop.top;anchors.right:desktop.right
            width:32;height:28
            // Join the frame with concave fillets; leave the top-right solid.
            background: Shape {
                id:closeBackground
                readonly property real joinRadius:8
                preferredRendererType:Shape.CurveRenderer
                ShapePath {
                    strokeWidth:-1;fillColor:popup.theme.surface
                    startX:-closeBackground.joinRadius;startY:0
                    PathQuad {x:0;y:closeBackground.joinRadius;controlX:0;controlY:0}
                    PathLine {x:0;y:closeBackground.height-closeBackground.joinRadius}
                    PathQuad {x:closeBackground.joinRadius;y:closeBackground.height;controlX:0;controlY:closeBackground.height}
                    PathLine {x:closeBackground.width-closeBackground.joinRadius;y:closeBackground.height}
                    PathQuad {x:closeBackground.width;y:closeBackground.height+closeBackground.joinRadius;controlX:closeBackground.width;controlY:closeBackground.height}
                    PathLine {x:closeBackground.width;y:0}
                    PathLine {x:-closeBackground.joinRadius;y:0}
                }
            }
        }
    }
}
