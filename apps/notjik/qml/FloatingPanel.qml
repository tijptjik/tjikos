import QtQuick
import QtQuick.Window
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io

// One regular xdg-toplevel per notice. Hyprland owns movement and workspace state.
FloatingWindow {
    id: panel
    required property var controller
    required property string noticeId
    readonly property var position: controller.floatingPositions[noticeId] || ({width:420})
    readonly property var notice: controller.notices.find(n => n.id === noticeId)
        || ({id:noticeId,title:"",body:"",app:"",urgency:"normal",icon:"",image:"",actions:[],data:{},type:"standard"})
    readonly property string initialTitle: "notjik-detached:" + controller.session + ":" + encodeURIComponent(noticeId)
    property bool placed: false
    property int placementFrames: 0
    property string address: ""
    property int placementAttempts: 0
    title: placed ? (controller.locked ? "Notification" : notice.title) + " — Notjik" : initialTitle
    visible: controller.ready && controller.draggingId !== noticeId
    implicitWidth: position.width
    implicitHeight: card.implicitHeight
    // The compositor owns position and width; content owns height as media and
    // reply controls open or close. implicitHeight alone is only an initial hint.
    minimumSize: Qt.size(180, Math.ceil(card.implicitHeight))
    maximumSize: Qt.size(16777215, Math.ceil(card.implicitHeight))
    color: "transparent"
    mask: Region { item: card; radius: card.radius }
    onWindowConnected: contentItem.Window.window.flags = Qt.Window | Qt.FramelessWindowHint
    onBackingWindowVisibleChanged: if (backingWindowVisible && !placed) locateTimer.start()
    onClosed: controller.close(noticeId, false)

    function finishPlacement() {
        placed = true;
        placementFrames = 0;
        contentItem.Window.window?.update();
    }

    // Keep the drag preview until the placed, opaque card has rendered. IPC
    // completion only confirms geometry, not that a new frame reached the screen.
    Connections {
        target: panel.contentItem.Window.window
        function onFrameSwapped() {
            if (!panel.placed || !panel.backingWindowVisible || panel.controller.dragPreviewId !== panel.noticeId) return;
            if (++panel.placementFrames < 2) panel.contentItem.Window.window.update();
            else panel.controller.dragPreviewId = "";
        }
    }

    // Placement runs once, against an exact PID/title match. Subsequent moves,
    // workspace transfers and updates must never reapply the drop coordinates.
    Timer {
        id: locateTimer
        interval: 30
        onTriggered: {
            if (panel.placed || locate.running) return;
            if (panel.placementAttempts++ < 50) locate.running = true;
            else {
                console.warn("notjik: compositor did not confirm initial placement; leaving the window at its native position");
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
                    const width = Math.round(panel.position.width), height = Math.ceil(card.implicitHeight);
                    const workspace = panel.position.workspace;
                    if (Hyprland.usingLua) {
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
                } catch (error) { console.warn("notjik window placement:", String(error)); }
            }
        }
        onExited: if (!place.running && !panel.placed) locateTimer.restart()
    }
    Process {
        id: place
        stdout: StdioCollector { onStreamFinished: if (text.trim() && text.trim() !== "ok") console.warn("notjik window placement:", text.trim()) }
        onExited: (code, status) => {
            if (code !== 0) console.warn("notjik window placement failed:", code);
            panel.finishPlacement();
        }
    }
    NotificationCard {
        id: card
        anchors.fill: parent
        opacity: panel.placed && !panel.controller.locked ? 1 : 0
        enabled: panel.placed && !panel.controller.locked
        notice: panel.notice
        controller: panel.controller
        monitorName: panel.screen?.name || ""
        floating: true
        Component.onCompleted: restoreState(controller.floatingStates[panel.noticeId])
        selected: panel.contentItem.Window.active
        onDragStarted: panel.startSystemMove()
        Keys.onEscapePressed: controller.unfocus()
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Delete && !panel.controller.locked) {
                panel.controller.close(panel.noticeId, false);
                event.accepted = true;
            }
        }
    }
}
