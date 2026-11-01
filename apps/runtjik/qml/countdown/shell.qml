import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {
    id: root
    property int remaining: Number(Quickshell.env("RUNTJIK_COUNTDOWN_SECONDS"))
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
            Text {
                anchors.centerIn: parent
                text: root.remaining
                font.pixelSize: 80
                color: "white"
            }
        }
    }
}
