import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {
    id: root
    property int remaining: Number(Quickshell.env("RUNTJIK_COUNTDOWN_SECONDS"))
    readonly property var rainbow: ["#ff53a6", "#eb6f92", "#f6c177", "#9ccf7a", "#9ccfd8", "#c4a7e7"]
    onRemainingChanged: numeral.requestPaint()
    Process {
        id: finish
        command: ["touch", "--", Quickshell.env("RUNTJIK_COUNTDOWN_SUCCESS")]
        onExited: Qt.quit()
    }
    Timer {
        interval: 1000; running: true; repeat: true
        onTriggered: { root.remaining--; if (root.remaining <= 0) { stop(); finish.running = true; } }
    }
    PanelWindow {
        visible: true
        screen: Quickshell.screens.find(s => s.name === Quickshell.env("RUNTJIK_COUNTDOWN_OUTPUT")) || Quickshell.screens[0]
        implicitWidth: 220; implicitHeight: 200
        color: "transparent"
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "runtjik-countdown"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
        Item {
            anchors.fill: parent
            focus: true
            Keys.onEscapePressed: Qt.quit()
            Canvas {
                id: numeral
                anchors.centerIn: parent
                width: 144; height: 144
                Accessible.role: Accessible.StaticText
                Accessible.name: "Recording in " + root.remaining
                onPaint: {
                    const ctx = getContext("2d");
                    ctx.reset();
                    const gradient = ctx.createLinearGradient(32, 28, 112, 116);
                    root.rainbow.forEach((colour, i) => gradient.addColorStop(i / (root.rainbow.length - 1), colour));
                    ctx.font = "bold 88px monospace";
                    ctx.textAlign = "center";
                    ctx.textBaseline = "middle";
                    ctx.lineWidth = 4;
                    ctx.strokeStyle = "#191724";
                    ctx.strokeText(String(root.remaining), width / 2, height / 2);
                    ctx.fillStyle = gradient;
                    ctx.fillText(String(root.remaining), width / 2, height / 2);
                }
            }
        }
    }
}
