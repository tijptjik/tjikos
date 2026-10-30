import Quickshell
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell.Io
import Quickshell.Services.Pipewire

Item {
    id: root
    required property var popup
    readonly property var theme: popup.theme
    readonly property bool demo: popup.controller.demo
    readonly property bool active: popup.opened && popup.audioPanel
    implicitHeight: body.implicitHeight
    property var lastAction: null
    property var demoNodes: [
        {ready:true,name:'demo-output',description:'USB Audio Speakers',isStream:false,isSink:true,properties:{},audio:{volume:.4,muted:false}},
        {ready:true,name:'demo-screen',description:'Navi 21/23 HDMI/DP Screen',isStream:false,isSink:true,properties:{},audio:{volume:.4,muted:false}},
        {ready:true,name:'alsa_input.usb-046d_Logitech_BRIO_demo',description:'BRIO microphone',isStream:false,isSink:false,properties:{},audio:{volume:.65,muted:false}},
        {ready:true,name:'demo-music',description:'Music',isStream:true,isSink:true,properties:{'application.name':'Music'},audio:{volume:.7,muted:false}}
    ]
    function routeOutput() {
        message='';
        if(demo){lastAction={action:'toggle-output'};return;}
        routing.running=true;
    }
    function inspect(){return {active:active,meters:meters.running,clients:clientGroups.length,output:activeOutput?.description || '',lastAction:lastAction};}
    property string message: ""
    readonly property var outputs: (demo ? demoNodes : Pipewire.nodes.values).filter(n => n.audio && !n.isStream && n.isSink && (n.description === "USB Audio Speakers" || n.description.startsWith("Navi 21/23 HDMI/DP")))
    readonly property var inputs: (demo ? demoNodes : Pipewire.nodes.values).filter(n => n.audio && !n.isStream && !n.isSink && n.name.startsWith("alsa_input.usb-046d_Logitech_BRIO_"))
    readonly property var clients: (demo ? demoNodes : Pipewire.nodes.values).filter(n => n.audio && n.isStream && n.isSink && !((n.properties["application.name"] || "") + " " + n.name + " " + n.description).toLowerCase().includes("speech-dispatcher-dummy"))
    readonly property var clientGroups: {
        const groups = [];
        for (const node of clients) {
            const title = node.properties["application.name"] || node.description || node.name;
            let group = groups.find(item => item.title === title);
            if (!group) { group = {title: title, nodes: []}; groups.push(group); }
            group.nodes.push(node);
        }
        return groups;
    }
    readonly property var usb: outputs.find(n => n.description === "USB Audio Speakers") || null
    readonly property var screenOutput: outputs.find(n => n.description.startsWith("Navi 21/23 HDMI/DP")) || null
    readonly property var activeOutput: demo ? demoNodes[0] : Pipewire.defaultAudioSink
    readonly property bool headphones: !!usb && activeOutput === usb
    PwObjectTracker { objects: root.active && !root.demo ? Pipewire.nodes.values.filter(n => !!n.audio) : [] }
    Process {
        id: routing
        onExited: (code,status)=>{if(code!==0 && !root.message)root.message="Could not switch the audio output."}
        command: [Quickshell.env("BARTJIK_HOME")+"/bin/bartjikctl", "helper", "audio", "toggle-output"]
        stdout: StdioCollector { onStreamFinished: root.message = text.trim() }
    }

    property var levels: ({})
    Process {
        id: meters
        command: [Quickshell.env("BARTJIK_HOME")+"/bin/bartjikctl", "helper", "audio-levels"]
        running: root.active && !root.demo
        stdout: SplitParser {
            onRead: line => {
                try { root.levels = JSON.parse(line); } catch (_) {}
            }
        }
        onExited: root.levels = ({})
    }


    ColumnLayout {
        id: body
        anchors.fill: parent
        spacing: 10



        Label { Layout.topMargin: 4; text: "Master"; color: theme.muted; font.family: theme.font; font.pixelSize: 11 }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: masterContents.implicitHeight + 24
            color: theme.surface
            radius: 10
            ColumnLayout {
                id: masterContents
                x: 12; y: 12
                width: parent.width - 24
                spacing: 4
                AudioGroup {
                    Layout.fillWidth: true
                    theme: root.theme; title: "Output"; nodes: root.activeOutput ? [root.activeOutput] : []
                    peak: root.levels.__output || 0
                    embedded: true
                    detail: root.activeOutput?.description || "No output device available"
                }
                Rectangle { visible: root.inputs.length > 0; Layout.fillWidth: true; height: 1; color: theme.background }
                AudioGroup {
                    visible: nodes.length > 0
                    Layout.fillWidth: true
                    theme: root.theme; title: "BRIO microphone"; nodes: root.inputs
                    microphone: true
                    peak: root.levels.__input || 0
                    embedded: true
                    detail: nodes.map(n => n.description || n.name).join(" · ")
                }
            }
        }
        Label { Layout.topMargin: 4; text: "Clients"; color: theme.muted; font.family: theme.font; font.pixelSize: 11 }
        ScrollView {
            id: clientsScroll
            padding: 0
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: Math.min(playback.implicitHeight, 260)
            contentWidth: availableWidth
            clip: true
            Column {
                id: playback
                width: clientsScroll.availableWidth
                spacing: 8
                Label {
                    visible: root.clients.length === 0
                    text: "No playback clients"
                    color: theme.muted; font.family: theme.font; font.pixelSize: 12
                }
                Repeater {
                    model: root.clientGroups
                    AudioGroup {
                        required property var modelData
                        width: playback.width
                        theme: root.theme
                        title: modelData.title
                        nodes: modelData.nodes
                        peak: root.levels[modelData.title] || 0
                    }
                }
            }
        }
        Label {
            visible: root.message !== ""
            text: root.message; textFormat: Text.PlainText
            color: theme.error || "#eb6f92"; font.family: theme.font; font.pixelSize: 12
            Layout.fillWidth: true; wrapMode: Text.Wrap
        }
        Button {
            id: headphonesButton
            Layout.topMargin: 4
            Layout.fillWidth: true
            implicitHeight: 48
            hoverEnabled: true
            Accessible.name: text
            contentItem: RowLayout {
                spacing: 10
                Item { Layout.fillWidth: true }
                Label { text: "headphones"; font.family: "Material Symbols Rounded"; font.pixelSize: 26; color: root.headphones ? theme.accent : theme.text }
                Label { text: "HEADPHONES"; font.family: theme.font; font.pixelSize: 13; font.weight: Font.DemiBold; color: root.headphones ? theme.accent : theme.text }
                Item { Layout.fillWidth: true }
            }
            text: root.headphones ? "Headphones · switch to Screen" : "Screen · switch to Headphones"
            enabled: !!root.usb && !!root.screenOutput && !routing.running
            onClicked: { root.routeOutput(); }
            background: Rectangle { radius: 8; color: root.headphones ? Qt.alpha(theme.accent, 0.22) : theme.surface; border.color: root.headphones ? theme.accent : "transparent" }
        }
    }
}
