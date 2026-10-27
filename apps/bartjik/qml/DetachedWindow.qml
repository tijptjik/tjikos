import QtQuick
import QtQuick.Window
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io

// One regular xdg-toplevel per detached card. Hyprland owns movement and workspace state.
FloatingWindow {
    id: panel
    required property var host
    readonly property var position: ({x:host.destination?.x+host.dropPosition.x,
        y:host.destination?.y+host.dropPosition.y,width:host.implicitWidth})
    readonly property string initialTitle: "bartjik-detached:" + Quickshell.processId + ":" + identity + ":" + host.generation
    property string identity: Date.now().toString(36)+Math.random().toString(36).slice(2)
    property bool placed:false
    property real releaseProgress:0
    property bool luaMode:Hyprland.usingLua
    property bool modeKnown:false
    property string address:""
    property int placementAttempts:0
    property alias liveContent:liveBody
    // Initial local position is retained for drag diagnostics only; Hyprland
    // owns geometry after placement and it must never be reapplied.
    readonly property var margins: ({left:host.dropPosition.x,top:host.dropPosition.y})
    title:placed ? host.detachedTitle + " — Bartjik" : initialTitle
    visible:host.nativeDetach && host.detached && host.panelOpen
    implicitWidth:host.implicitWidth
    implicitHeight:host.implicitHeight
    minimumSize:Qt.size(180,180)
    color:"transparent"
    onWindowConnected:contentItem.Window.window.flags=Qt.Window | Qt.FramelessWindowHint
    onBackingWindowVisibleChanged:{
        if(backingWindowVisible && !placed){if(modeKnown)locateTimer.start();else probe.running=true;}
        else if(!backingWindowVisible){locateTimer.stop();placed=false;address="";placementAttempts=0;}
    }
    onClosed:host.dismissed()
    onPlacedChanged:{
        release.stop();releaseProgress=0;
        if(placed){release.start();contentItem.Window.window?.update();}
    }
    NumberAnimation {
        id:release;target:panel;property:'releaseProgress';from:0;to:1;duration:420;easing.type:Easing.OutExpo
        onFinished:panel.host.completeDrop()
    }
    Item {
        id:liveBody;anchors.fill:parent;opacity:panel.placed?panel.releaseProgress:0
        scale:.985+.015*panel.releaseProgress;transformOrigin:Item.Center
    }

    function finishPlacement() {if(visible)placed=true;}
    // Some Quickshell builds report usingLua=false on Lua compositors.
    // A no-op probe avoids issuing legacy dispatch syntax to those versions.
    Process {
        id:probe
        command:["hyprctl","eval","return"]
        stdout:StdioCollector {onStreamFinished:panel.luaMode=text.trim()==="ok"}
        onExited:{panel.modeKnown=true;if(panel.visible)locateTimer.start();}
    }

    // Placement runs once, against an exact PID/title match. Subsequent moves,
    // workspace transfers and updates must never reapply the drop coordinates.
    Timer {
        id: locateTimer
        interval: 30
        onTriggered: {
            if (!panel.visible || panel.placed || locate.running) return;
            if (panel.placementAttempts++ < 50) locate.running = true;
            else {
                console.warn("bartjik: compositor did not confirm initial placement; leaving the window at its native position");
                panel.finishPlacement();
            }
        }
    }
    Process {
        id: locate
        command: ["hyprctl", "-j", "clients"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const client = JSON.parse(text).find(c => c.pid === Quickshell.processId && c.title === panel.initialTitle);
                    if (!client || !/^0x[0-9a-f]+$/i.test(client.address)) return;
                    panel.address = client.address;
                    const target = JSON.stringify("address:" + client.address);
                    const x = Math.round(panel.position.x), y = Math.round(panel.position.y);
                    const width = Math.round(panel.position.width), height = Math.ceil(panel.implicitHeight);
                    const monitor=Hyprland.monitorFor(panel.host.destination);
                    const special=monitor?.lastIpcObject?.specialWorkspace;
                    const workspace=special?.id ? special.name : monitor?.activeWorkspace?.name;
                    if (panel.luaMode) {
                        const moveWorkspace = workspace ? "hl.dispatch(hl.dsp.window.move({workspace=" + JSON.stringify(workspace)
                            + ",follow=false,window=w}));" : "";
                        const makeFloating = client.floating ? "" : "hl.dispatch(hl.dsp.window.float({window=w}));";
                        place.command = ["hyprctl", "eval", "local w=hl.get_window(" + target + "); if w then "
                            + makeFloating + moveWorkspace
                            + "hl.dispatch(hl.dsp.window.resize({x=" + width + ",y=" + height + ",relative=false,window=w}));"
                            + "hl.dispatch(hl.dsp.window.move({x=" + x + ",y=" + y + ",relative=false,window=w})); end"];
                    } else {
                        place.command = ["hyprctl", "--batch", "dispatch setfloating address:" + client.address
                            + (workspace ? "; dispatch movetoworkspacesilent " + workspace + ",address:" + client.address : "")
                            + "; dispatch resizewindowpixel exact " + width + " " + height + ",address:" + client.address
                            + "; dispatch movewindowpixel exact " + x + " " + y + ",address:" + client.address];
                    }
                    place.running = true;
                } catch (error) { console.warn("bartjik window placement:", String(error)); }
            }
        }
        onExited: if (!place.running && !panel.placed) locateTimer.restart()
    }
    Process {
        id: place
        stdout: StdioCollector { onStreamFinished: if (text.trim() && text.trim() !== "ok") console.warn("bartjik window placement:", text.trim()) }
        onExited: (code, status) => {
            if (code !== 0) console.warn("bartjik window placement failed:", code);
            panel.finishPlacement();
        }
    }
}
