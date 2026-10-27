import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Hyprland

// A top-layer surface with an input mask supplied by the launcher frame.
PanelWindow {
    id: window
    required property var targetScreen
    property bool pinned: false
    property bool ownsKeyboard: true
    property bool acquiringKeyboard: false
    function acquireKeyboard() {
        ownsKeyboard=true;acquiringKeyboard=true;keyboardHandoff.restart();
    }
    Timer {id:keyboardHandoff;interval:50;onTriggered:window.acquiringKeyboard=false;}
    onPinnedChanged: {boundsRevision++;observerDelay.restart();}
    property bool styled: false
    property bool observed: false
    property bool placed: backingWindowVisible && styled && observed
    property string address: ''
    property string placementError: ''
    property string clickWatchError: ''
    property real frameX: 0
    property real frameY: 0
    property real frameWidth: 720
    property real frameHeight: 600
    property int boundsRevision: 0
    property int armedRevision: 0
    property bool observerAgain: false
    property string hook: Quickshell.env('RUNTJIK_HOME')+'/qml/outside-click.lua'
    screen: targetScreen
    anchors {top:true;bottom:true;left:true;right:true}
    exclusionMode: ExclusionMode.Ignore
    color: 'transparent'
    WlrLayershell.namespace: 'runtjik'
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.keyboardFocus: !ownsKeyboard ? WlrKeyboardFocus.None : acquiringKeyboard ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.OnDemand
    function updateBounds(x,y,w,h) {
        frameX=x;frameY=y;frameWidth=w;frameHeight=h;boundsRevision++;
        if (backingWindowVisible) observerDelay.restart();
    }
    function arm() {
        if (!backingWindowVisible) return;
        if (observer.running) {observerAgain=true;return;}
        armedRevision=boundsRevision;
        observer.command=['hyprctl','eval','assert(loadfile('+JSON.stringify(hook)+'))()('
            +Quickshell.processId+','+Math.round(screen.x+frameX)+','+Math.round(screen.y+frameY)+','
            +Math.round(frameWidth)+','+Math.round(frameHeight)+','+pinned+')'];
        observer.running=true;
    }
    onBackingWindowVisibleChanged: {if (backingWindowVisible) observerDelay.restart();else observed=false;}
    Timer {id:observerDelay;interval:0;onTriggered:window.arm()}
    Connections {
        target: Hyprland
        function onRawEvent(event) {
            if (event.name==='configreloaded') {style.running=true;observerDelay.restart();}
        }
    }
    Process {
        id: style
        running: true
        command:['hyprctl','eval','hl.layer_rule({name="runtjik-plain",match={namespace="^runtjik$"},blur=false,blur_popups=false,dim_around=false,no_anim=true,order=100})']
        stdout: StdioCollector {onStreamFinished: {
            window.placementError=text.trim()==='ok'?'':text.trim();
        }}
        onExited: (code,status)=>{
            if(code!==0 && !window.placementError)window.placementError='Unable to configure the launcher layer';
            window.styled=true;
        }
    }
    Process {
        id: observer
        stdout: StdioCollector {onStreamFinished: {window.clickWatchError=text.trim()==='ok'?'':text.trim();if(window.armedRevision===window.boundsRevision)window.observed=true;}}
        onExited: (code,status)=>{
            if(code!==0){window.clickWatchError='Unable to observe outside clicks';window.observed=true;}
            if(window.observerAgain || window.armedRevision!==window.boundsRevision){
                window.observerAgain=false;observerDelay.restart();
            }
        }
    }
}
