import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell.Services.Pipewire

Rectangle {
    id: group
    required property var theme
    required property string title
    required property var nodes
    property string detail: ""
    property bool embedded: false
    property bool microphone: false
    readonly property var readyNodes: nodes.filter(n => n.ready && n.audio)
    readonly property real volume: readyNodes.length ? readyNodes.reduce((sum, n) => sum + n.audio.volume, 0) / readyNodes.length : 0
    readonly property bool muted: readyNodes.length > 0 && readyNodes.every(n => n.audio.muted)
    property real peak: 0
    // Peak brightness uses a dB scale; the handle always retains the volume setting.
    readonly property real meterPosition: muted || peak <= 0.001 ? 0 : Math.max(0, Math.min(1, (20 * Math.log(peak) / Math.LN10 + 60) / 60))
    color: embedded ? "transparent" : theme.surface
    radius: 10
    implicitHeight: contents.implicitHeight + (embedded ? 0 : 24)
    ColumnLayout {
        id: contents
        opacity: group.muted ? 0.5 : 1
        Behavior on opacity { NumberAnimation { duration: 120 } }
        x: group.embedded ? 0 : 12; y: x
        width: parent.width - 2 * x
        spacing: 4
        RowLayout {
            Layout.fillWidth: true
            Label { text: group.embedded ? group.detail : group.title; textFormat: Text.PlainText; color: theme.text; font.family: theme.font; font.pixelSize: 13; Layout.fillWidth: true; elide: Text.ElideRight }
            PanelButton {
                theme: group.theme; symbol: group.microphone ? (group.muted ? "mic_off" : "mic") : (group.muted ? "volume_off" : "volume_up")
                text: (group.muted ? "Unmute " : "Mute ") + group.title
                accented: group.muted; enabled: group.readyNodes.length > 0
                onClicked: { const value = !group.muted; group.readyNodes.forEach(n => n.audio.muted = value); }
            }
        }
        Label { visible: !group.embedded && text !== ""; text: group.nodes.length ? group.detail : "No devices available"; color: theme.muted; font.family: theme.font; font.pixelSize: 11; Layout.fillWidth: true; elide: Text.ElideRight }
        Slider {
            id: slider
            Layout.fillWidth: true
            Layout.minimumHeight: 36
            implicitHeight: 36
            leftPadding: 0; rightPadding: 0
            topPadding: 0; bottomPadding: 0
            from: 0; to: 1; stepSize: 0.01
            enabled: group.readyNodes.length > 0
            value: group.volume
            Accessible.name: group.title + " volume"
            onMoved: { const level = value; group.readyNodes.forEach(n => n.audio.volume = level); }
            background: Item {
                x: slider.leftPadding; y: slider.topPadding + slider.availableHeight / 2 - height / 2
                width: slider.availableWidth; height: 16
                Row {
                    anchors.fill: parent
                    spacing: 3
                    Repeater {
                        model: 40
                        Rectangle {
                            required property int index
                            width: (slider.availableWidth - 39 * 3) / 40
                            height: 8 + (index % 5 === 4 ? 8 : 4)
                            anchors.verticalCenter: parent.verticalCenter
                            radius: 2
                            readonly property bool lit: (index + 0.5) / 40 <= group.meterPosition
                            color: lit ? theme.accent : (index + 0.5) / 40 <= slider.visualPosition ? Qt.alpha(theme.accent, 0.25) : theme.background
                            Behavior on color { ColorAnimation { duration: 80 } }
                        }
                    }
                }
            }
            handle: Rectangle {
                x: slider.leftPadding + slider.visualPosition * (slider.availableWidth - width)
                y: slider.topPadding + slider.availableHeight / 2 - height / 2
                implicitWidth: 16; implicitHeight: 26
                width: 16; height: 26; radius: 5
                color: slider.pressed ? theme.text : theme.accent
                border.width: 2; border.color: theme.background
                Rectangle { anchors.centerIn: parent; width: 2; height: 10; radius: 1; color: theme.background }
            }
        }
    }
}
