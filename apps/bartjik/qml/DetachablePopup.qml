import QtQuick
import QtQuick.Window
import QtQuick.Templates as Controls
import Quickshell
import Quickshell.Wayland

// Keep the popup's mouse grab while a snapshot follows the pointer. The same
// layer surface carries that snapshot and the live card after release. Two
// reusable surfaces alternate when moving an already-detached card so its
// original window retains the mouse grab even across outputs. Native detach
// instead hands the live card to an xdg-toplevel after the initial drop.
PopupWindow {
    id:host
    required property Item panelBody
    property bool panelOpen:false
    property bool dragFromBody:false
    property bool stableSurface:false
    property bool nativeDetach:true
    property string detachedTitle:"Panel"
    property var panelShell:null
    property int positionDuration:140
    readonly property var attachedSurface:panelShell?panelShell.surfaceFor(anchor.window?.screen):surfaces.instances.find(s=>s.screen===anchor.window?.screen) || null
    readonly property Item sourceContent:sourceSurface?sourceSurface.contentItem:stableSurface && attachedSurface?attachedSurface.contentItem:contentItem
    property bool detached:false
    property bool dragging:false
    property bool dropping:false
    property bool releasePending:false
    property var destination:null
    property var sourceSurface:null
    property int liveIndex:0
    property int previewIndex:0
    readonly property var floating:nativeDetach?nativeWindow:liveIndex===0?firstSurface:secondSurface
    readonly property var previewWindow:previewIndex===0?firstSurface:secondSurface
    readonly property var preview:previewWindow.previewImage
    property point dropPosition:Qt.point(0,0)
    property point dragOrigin:Qt.point(0,0)
    property point grabOffset:Qt.point(0,0)
    property var dragImage:null
    property int generation:0
    property bool previewReady:false
    property int previewFrames:0
    property int dropFrames:0
    property int sourceFrames:0
    property bool dropReady:false
    readonly property var panelScreen:detached?floating.screen:anchor.window?.screen
    readonly property bool panelVisible:visible || floating.visible || (stableSurface && (!panelShell || panelShell.activePanel===host) && !!attachedSurface?.active)
    readonly property Item bodyParent:detached?floating.liveContent:stableSurface && attachedSurface && (!panelShell || panelShell.activePanel===host)?attachedSurface.bodySlot:contentItem
    readonly property real bodyOpacity:dragging && previewReady?0:1
    signal dismissed()

    function clamp(value,size,extent){return Math.max(12,Math.min(value,extent-size-12));}
    function attachedPosition(){
        if(!panelScreen)return Qt.point(0,0);
        if(stableSurface && attachedSurface)return Qt.point(attachedSurface.frame.x,attachedSurface.frame.y);
        const side=anchor.window?.bar.side;
        return Qt.point(clamp(anchor.rect.x-(side==='r'?width:0),width,panelScreen.width),
            clamp(anchor.rect.y-(side==='b'?height:0),height,panelScreen.height));
    }
    function beginDrag(offset){
        if(dragging || dropping || !panelOpen)return;
        if(detached && nativeDetach){nativeWindow.startSystemMove();return;}
        sourceSurface=detached?floating:stableSurface?attachedSurface:host;
        const pos=detached?Qt.point(floating.margins.left,floating.margins.top):attachedPosition();
        dragOrigin=Qt.point(panelScreen.x+pos.x,panelScreen.y+pos.y);
        grabOffset=offset;destination=panelScreen;dropPosition=pos;
        previewIndex=detached?1-liveIndex:liveIndex;
        previewWindow.screen=destination;previewWindow.cardPosition=dropPosition;
        previewReady=false;previewFrames=0;releasePending=false;dragging=true;
        const current=++generation;
        panelBody.grabToImage(result=>{if(dragging && generation===current)dragImage=result;});
    }
    function moveDrag(point){
        if(!dragging || releasePending)return;
        const x=dragOrigin.x+point.x,y=dragOrigin.y+point.y;
        destination=Quickshell.screens.find(s=>x>=s.x && y>=s.y && x<s.x+s.width && y<s.y+s.height) || destination;
        dropPosition=Qt.point(clamp(x-grabOffset.x-destination.x,width,destination.width),
            clamp(y-grabOffset.y-destination.y,height,destination.height));
        previewWindow.screen=destination;previewWindow.cardPosition=dropPosition;
    }
    function endDrag(point){
        if(!dragging)return;
        moveDrag(point);releasePending=true;
        // A quick release can beat grabToImage or the first preview frame.
        if(previewReady)commitDrop();
    }
    function commitDrop(){
        dropping=true;dropFrames=0;sourceFrames=0;dropReady=false;
        releasePending=false;dragging=false;liveIndex=previewIndex;detached=true;
        Qt.callLater(()=>{
            if(!dropping)return;
            dropReady=true;
            floating.contentItem.Window.window?.update();
            host.sourceContent.Window.window?.update();
        });
    }
    function cancelDrag(){
        ++generation;releasePending=false;dragging=false;previewReady=false;finishDrop();
        if(detached)destination=floating.screen;
    }
    function finishDrop(){dropping=false;dropReady=false;dragImage=null;sourceSurface=null;}
    function completeDrop(){if(dropFrames>=2 && sourceFrames>=2)finishDrop();}
    function resetDetach(){cancelDrag();detached=false;destination=null;}
    function placement(){return {monitor:panelScreen?.name || '',x:detached?floating.margins.left:anchor.rect.x,y:detached?floating.margins.top:anchor.rect.y};}
    Connections {
        target:host.previewWindow.contentItem.Window.window
        function onFrameSwapped(){
            if(!host.previewWindow.backingWindowVisible || host.previewWindow.contentItem.Screen.name!==host.destination?.name)return;
            if(host.dragging && !host.previewReady && preview.status===Image.Ready){
                if(++host.previewFrames<2)host.previewWindow.contentItem.Window.window.update();
                else {host.previewReady=true;if(host.releasePending)host.commitDrop();}
            } else if(!host.nativeDetach && host.dropping && host.dropReady && host.panelBody.parent===host.floating.liveContent){
                if(++host.dropFrames<2)floating.contentItem.Window.window.update();
                host.completeDrop();
            }
        }
    }
    Connections {
        target:nativeWindow.contentItem.Window.window
        function onFrameSwapped(){
            if(!host.nativeDetach || !host.dropping || !host.dropReady || !nativeWindow.placed)return;
            if(++host.dropFrames<2)nativeWindow.contentItem.Window.window.update();
            host.completeDrop();
        }
    }
    Connections {
        target:host.sourceContent.Window.window
        function onFrameSwapped(){
            if(!host.dropping || !host.dropReady || host.panelBody.parent!==host.floating.liveContent)return;
            // Clear the old popup before unmapping it, so popup fade-out cannot
            // retain an old image at the pickup position.
            if(++host.sourceFrames<2)host.sourceContent.Window.window.update();
            host.completeDrop();
        }
    }
    Connections {
        target:Quickshell
        function onScreensChanged(){
            if((host.detached || host.dragging) && !Quickshell.screens.includes(host.destination))host.dismissed();
        }
    }
    Connections {
        target:host
        function onPanelOpenChanged(){if(host.panelShell){if(host.panelOpen)host.panelShell.show(host);else host.panelShell.release(host);}}
        function onDetachedChanged(){if(host.detached)host.panelShell?.release(host);}
    }
    Variants {
        id:surfaces
        model:host.stableSurface && !host.panelShell?Quickshell.screens:[]
        StablePanelSurface {panelHost:host}
    }
    // Template base types recognize controls across all Qt Quick Controls styles.
    // Reject presses over real controls, including their icon/text children.
    // Clipping and mapFromItem keep scrolled content hit-testing accurate.
    function controlAt(x,y,item){
        for(const child of (item || panelBody).children){
            if(child===moveArea || !child.visible || child.opacity===0)continue;
            const point=child.mapFromItem(panelBody,x,y),inside=child.contains(point);
            if(child.clip && !inside)continue;
            if(inside && (child instanceof Controls.AbstractButton || child instanceof Controls.Slider
                || child instanceof Controls.RangeSlider || child instanceof Controls.ScrollBar
                || child instanceof Controls.ComboBox || child instanceof Controls.SpinBox
                || child instanceof TextInput || child instanceof TextEdit))return true;
            if(controlAt(x,y,child))return true;
        }
        return false;
    }
    MouseArea {
        id:moveArea;parent:host.panelBody;anchors.fill:parent;z:1000
        enabled:host.panelOpen && !host.dropping
        acceptedButtons:Qt.LeftButton;preventStealing:true
        property point start:Qt.point(0,0)
        onPressed:mouse=>{
            const superDrag=!!(mouse.modifiers & Qt.MetaModifier);
            if((!host.detached && !host.dragFromBody && !superDrag)
                || (!superDrag && host.controlAt(mouse.x,mouse.y))){mouse.accepted=false;return;}
            start=Qt.point(mouse.x,mouse.y);
        }
        onPositionChanged:mouse=>{
            if(!pressed)return;
            const point=Qt.point(mouse.x,mouse.y);
            if(!host.dragging && Math.hypot(point.x-start.x,point.y-start.y)>=8)host.beginDrag(start);
            host.moveDrag(point);
        }
        onReleased:mouse=>host.endDrag(Qt.point(mouse.x,mouse.y))
        onCanceled:if(host.dragging)host.cancelDrag()
        onWheel:wheel=>{wheel.accepted=false;}
    }
    component CardSurface: PanelWindow {
        id:surface
        property point cardPosition:Qt.point(0,0)
        readonly property bool activeCard:!host.nativeDetach && host.detached && surface===host.floating
        readonly property bool showingPreview:surface===host.previewWindow && !!host.dragImage
        property alias liveContent:liveContent
        property alias previewImage:previewImage
        visible:host.panelOpen && (activeCard || showingPreview || (host.dropping && surface===host.sourceSurface))
        anchors {top:true;left:true}
        implicitWidth:Math.min(host.implicitWidth,(screen?.width || host.implicitWidth+24)-24)
        implicitHeight:Math.min(host.implicitHeight,(screen?.height || host.implicitHeight+24)-24)
        margins.left:host.clamp(cardPosition.x,width,screen?.width || width+24)
        margins.top:host.clamp(cardPosition.y,height,screen?.height || height+24)
        color:'transparent';exclusionMode:ExclusionMode.Ignore
        WlrLayershell.namespace:'bartjik-panel'
        WlrLayershell.layer:WlrLayer.Overlay
        WlrLayershell.keyboardFocus:activeCard && host.grabFocus?WlrKeyboardFocus.OnDemand:WlrKeyboardFocus.None
        mask:Region {width:surface.activeCard?surface.width:0;height:surface.activeCard?surface.height:0}
        Item {id:liveContent;anchors.fill:parent}
        Image {
            id:previewImage;anchors.fill:parent;z:1
            visible:surface.showingPreview;source:surface.showingPreview?host.dragImage.url:''
        }
    }
    DetachedWindow {id:nativeWindow;host:host}
    CardSurface {id:firstSurface}
    CardSurface {id:secondSurface}
}
