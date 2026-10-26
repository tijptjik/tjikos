import QtQuick
import Quickshell
import Quickshell.Io

// One attached card across providers; detached cards retain their own surfaces.
Item {
    id:deck
    // Publish the actual masked card bounds for Hyprland's Super-drag routing.
    // The compositor checks the live layer PID before consulting this file.
    readonly property var hitSurface:surfaceFor(panelScreen)
    readonly property string hitRect:panelOpen && hitSurface
        ? [panelScreen.x+hitSurface.frame.x,panelScreen.y+hitSurface.frame.y,
           hitSurface.frame.width,hitSurface.frame.height].join(' ') : '0 0 0 0'
    FileView {
        id:hitFile
        path:Quickshell.env('XDG_RUNTIME_DIR')+'/bartjik-panel-'+Quickshell.processId+'.rect'
        blockWrites:true
    }
    onHitRectChanged:hitFile.setText(hitRect)
    Component.onCompleted:hitFile.setText(hitRect)
    Component.onDestruction:hitFile.setText('0 0 0 0')
    property var activePanel:null
    property var pendingPanel:null
    property int generation:0
    property var transitionImage:null
    property real transitionWidth:0
    property real transitionHeight:0
    property real contentProgress:1
    readonly property bool panelOpen:!!activePanel?.panelOpen && !activePanel.detached
    readonly property bool detached:false
    readonly property int positionDuration:transitionImage?140:(activePanel?.positionDuration ?? 140)
    readonly property bool dragging:!!activePanel?.dragging
    readonly property bool grabFocus:!!activePanel?.grabFocus
    readonly property var panelScreen:activePanel?.panelScreen || null
    readonly property var panelBody:activePanel?.panelBody || null
    readonly property var anchor:activePanel?.anchor || {window:null,rect:{x:0,y:0}}
    readonly property var theme:activePanel?.theme || {background:'#1f1d2e',border:'#555169'}
    implicitWidth:activePanel?.implicitWidth || 440
    implicitHeight:activePanel?.implicitHeight || 360
    function clamp(value,size,extent){return Math.max(12,Math.min(value,extent-size-12));}
    function surfaceFor(screen){return surfaces.instances.find(s=>s.screen===screen) || null;}
    function canCross(window){return panelOpen && !dragging && activePanel.anchor.window===window;}
    function holding(panel){return activePanel===panel && !!pendingPanel;}
    function show(panel){
        if(panel.detached || panel.dragging || !panel.panelOpen || pendingPanel===panel)return;
        if(activePanel?.dragging && activePanel!==panel){
            Qt.callLater(()=>{if(activePanel!==panel && !panel.detached && !panel.dragging)panel.dismissed();});
            return;
        }
        const serial=++generation;
        pendingPanel=null;
        if(activePanel===panel)return;
        const surface=surfaceFor(panel.panelScreen);
        if(canCross(panel.anchor.window) && surface){
            pendingPanel=panel;
            const w=surface.frame.width,h=surface.frame.height;
            surface.frame.grabToImage(image=>{
                if(serial!==generation || pendingPanel!==panel || !panel.panelOpen)return;
                if(activePanel?.dragging){pendingPanel=null;panel.dismissed();return;}
                commit(panel,image,w,h);
            });
        }else commit(panel,null,0,0);
    }
    function commit(panel,image,w,h){
        const previous=activePanel;
        fade.stop();transitionImage=image;transitionWidth=w;transitionHeight=h;
        contentProgress=image?0:1;pendingPanel=null;activePanel=panel;
        if(previous && previous!==panel && !previous.detached && !previous.dragging)previous.dismissed();
        if(image)fade.restart();
    }
    function release(panel){
        if(pendingPanel===panel){++generation;pendingPanel=null;}
        if(activePanel!==panel)return;
        ++generation;pendingPanel=null;activePanel=null;
        fade.stop();contentProgress=1;transitionImage=null;
    }
    NumberAnimation {id:fade;target:deck;property:'contentProgress';from:0;to:1;duration:140;onFinished:deck.transitionImage=null}
    Variants {
        id:surfaces;model:Quickshell.screens
        StablePanelSurface {
            panelHost:deck
            contentProgress:deck.contentProgress
            transitionImage:deck.transitionImage
            transitionWidth:deck.transitionWidth
            transitionHeight:deck.transitionHeight
        }
    }
}
