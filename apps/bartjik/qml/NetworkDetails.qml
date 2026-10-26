import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell.Io

Item {
    id: root
    required property var popup
    readonly property var theme: popup.theme
    readonly property bool demo: popup.controller.demo
    readonly property bool active: popup.opened && popup.networkPanel
    readonly property bool needsKeyboard: active && selected!==null
    implicitHeight: body.implicitHeight
    property var lastAction: null
    onActiveChanged: {
        if(active)run({action:'status'});
        else {selected=null;password.text='';}
    }
    Component.onCompleted: {
        if(demo){
            hasState=true;
            state={enabled:true,defaultDns:['192.0.2.1'],devices:[
                {interface:'wlan-demo',kind:'wifi',state:'connected',connection:'Demo Wi-Fi',addresses:{ip:['192.0.2.20'],gateway:'192.0.2.1',dns:['192.0.2.1']}},
                {interface:'eth-demo',kind:'ethernet',state:'unavailable',connection:'',addresses:{}}
            ],networks:[
                {ssid:'Demo Wi-Fi',bssid:'02:00:00:00:00:01',interface:'wlan-demo',signal:94,security:'WPA2',active:true},
                {ssid:'Guest Wi-Fi',bssid:'02:00:00:00:00:02',interface:'wlan-demo',signal:78,security:'WPA2',active:false}
            ]};
            traffic={'wlan-demo':{rx:1048576,tx:524288,rxRate:65536,txRate:32768}};
        }
    }
    function inspect(){return {active:active,traffic:trafficObserver.running,busy:busy,devices:state.devices.length,networks:state.networks.length,selected:!!selected,lastAction:lastAction};}
    property bool busy: false
    property bool hasState: false
    property string operation: "status"
    property bool scanMessageReady: false
    readonly property bool showProgress: busy && (operation === "scan" ? scanMessageReady : operation !== "status" || !hasState)
    onBusyChanged: {
        if (!busy) { scanMessageDelay.stop(); scanMessageReady = false; }
    }
    property var state: ({ enabled: false, devices: [], networks: [] })
    property var traffic: ({})
    property var selected: null
    property string message: ""
    property string pending: ""

    readonly property var connectedDevices: state.devices.filter(device=>device.state==='connected')
    function run(request) {
        if (busy) return;
        if(demo){lastAction={action:request.action,interface:request.interface || ''};return;}
        operation = request.action;
        scanMessageReady = false;
        busy = true;
        if (operation === "scan") scanMessageDelay.restart();
        if (request.action !== "status") message = "";
        pending = JSON.stringify(request);
        backend.stdinEnabled = true;
        backend.running = true;
    }

    Timer {
        id: scanMessageDelay
        interval: 1000
        onTriggered: { if (root.busy && root.operation === "scan") root.scanMessageReady = true; }
    }

    function connectSelected() {
        if (!selected || busy) return;
        run({ action: "connect", interface: selected.interface, bssid: selected.bssid, password: password.text });
        password.text = "";
        selected = null;
    }

    Process {
        id: backend
        command: ["python3", Qt.resolvedUrl("network_controls.py").toString().replace("file://", "")]
        onStarted: {
            write(root.pending + "\n");
            root.pending = "";
            stdinEnabled = false;
        }
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const result = JSON.parse(text);
                    if (result.ok) {
                        root.state = result;
                        root.hasState = true;
                        root.message = "";
                    }
                    else root.message = result.error;
                } catch (_) {
                    root.message = "Could not read network status. Check that Python 3 and nmcli are installed.";
                }
            }
        }
        onExited: (code, status) => {
            root.pending = "";
            root.busy = false;
            if (code !== 0) root.message = "The networking helper stopped unexpectedly.";
        }
    }

    Timer {
        interval: 10000
        repeat: true
        running: root.active && !root.demo
        onTriggered: root.run({ action: "status" })
    }

    Process {
        id: trafficObserver
        command: ["python3", Qt.resolvedUrl("network_controls.py").toString().replace("file://", ""), "traffic"]
        running: root.active && !root.demo
        stdout: SplitParser {
            onRead: line => {
                try { root.traffic = JSON.parse(line); }
                catch (_) { root.traffic = ({}); }
            }
        }
    }


    ColumnLayout {
        id: body
        anchors.fill: parent
        spacing: 12

        Repeater {
            model: root.connectedDevices
            delegate: NetworkConnection {
                required property var modelData
                Layout.fillWidth: true
                Layout.preferredHeight: implicitHeight
                theme: root.theme
                device: modelData
                networks: root.state.networks
                traffic: root.traffic[modelData.interface] || ({})
                defaultDns: root.state.defaultDns || []
                busy: root.busy
                onDisconnectRequested: root.run({ action: "disconnect", interface: modelData.interface })
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 6
            RowLayout {
                Layout.topMargin: 4
                Layout.fillWidth: true
                Label {
                    text: root.state.enabled ? "Wi-Fi networks" : "Wi-Fi is off"
                    font.family: theme.font
                    font.pixelSize: 13
                    color: theme.text
                    Layout.fillWidth: true
                }
                PanelButton {
                    theme: root.theme
                    symbol: "refresh"
                    text: "Scan for networks"
                    enabled: !root.busy && root.state.enabled
                    onClicked: root.run({ action: "scan" })
                }
                PanelButton {
                    theme: root.theme
                    symbol: "power_settings_new"
                    text: root.state.enabled ? "Turn Wi-Fi off" : "Turn Wi-Fi on"
                    accented: root.state.enabled
                    enabled: !root.busy
                    onClicked: root.run({ action: "radio", enabled: !root.state.enabled })
                }
            }

            ScrollView {
                id: wifiScroll
                padding: 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.minimumHeight: 0
                Layout.preferredHeight: root.hasState ? wifiItems.previewHeight : 80
                Layout.maximumHeight: Layout.preferredHeight
                contentWidth: availableWidth
                contentHeight: wifiItems.implicitHeight
                clip: true
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ScrollBar.vertical: ScrollBar {
                    id: wifiBar
                    parent: wifiScroll
                    x: wifiScroll.width - width
                    y: 0
                    height: wifiScroll.height
                    width: 6
                    padding: 0
                    policy: ScrollBar.AsNeeded
                    contentItem: Rectangle {
                        implicitWidth: 6
                        radius: 3
                        color: wifiBar.pressed ? "#555555" : wifiBar.hovered ? "#484848" : "#383838"
                    }
                    background: Item {}
                }

                Column {
                    id: wifiItems
                    width: wifiScroll.availableWidth - (wifiBar.visible ? 12 : 0)
                    spacing: 6
                    // Keep at most three complete rows visible, including their spacing.
                    readonly property real previewHeight: {
                        const count = Math.min(networkRows.count, 3);
                        if (!count) return implicitHeight;
                        let height = (count - 1) * spacing;
                        for (let i = 0; i < count; ++i)
                            height += networkRows.itemAt(i)?.implicitHeight || 0;
                        return height;
                    }
                    opacity: root.showProgress || root.message !== "" ? 0 : 1
                    enabled: !root.showProgress && root.message === ""
                    Label {
                        width: parent.width
                        visible: !root.state.networks.some(network => !network.active)
                        text: !root.state.devices.some(device => device.kind === "wifi")
                            ? "No Wi-Fi adapter found."
                            : !root.state.enabled ? "Turn on Wi-Fi to find networks."
                            : root.state.networks.length ? "No other networks nearby" : "No networks found · try scanning"
                        color: theme.text
                        opacity: 0.65
                        font.family: theme.font
                        font.pixelSize: 12
                        wrapMode: Text.Wrap
                    }

                    Repeater {
                        id: networkRows
                        model: root.state.networks.filter(network => !network.active)
                        delegate: Button {
                            id: networkRow
                            required property var modelData
                            width: wifiItems.width
                            padding: 12
                            hoverEnabled: true
                            focusPolicy: Qt.NoFocus
                            enabled: !root.busy
                            Accessible.name: "Connect to " + modelData.ssid
                            background: Rectangle {
                                radius: 8
                                color: networkRow.hovered || root.selected?.bssid === networkRow.modelData.bssid ? theme.surface : "transparent"
                            }
                            contentItem: RowLayout {
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2
                                    Label {
                                        text: networkRow.modelData.ssid
                                        textFormat: Text.PlainText
                                        color: theme.text
                                        font.family: theme.font
                                        font.pixelSize: 13
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                    Label {
                                        text: networkRow.modelData.signal + "% · " + (networkRow.modelData.security || "Open") + " · " + networkRow.modelData.interface
                                        textFormat: Text.PlainText
                                        color: theme.text
                                        opacity: 0.65
                                        font.family: theme.font
                                        font.pixelSize: 11
                                        elide: Text.ElideRight
                                        Layout.fillWidth: true
                                    }
                                }
                                Label { text: "chevron_right"; font.family: "Material Symbols Rounded"; font.pixelSize: 18; color: theme.text }
                            }
                            onClicked: {
                                root.selected = modelData;
                                password.text = "";
                                password.forceActiveFocus();
                            }
                        }
                    }
                }
                Label {
                    parent: wifiScroll
                    anchors.fill: parent
                    anchors.margins: 10
                    visible: root.showProgress || root.message !== ""
                    text: root.message || (root.operation === "scan" ? "Scanning for networks…" : "Working…")
                    textFormat: Text.PlainText
                    verticalAlignment: Text.AlignVCenter
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    color: root.message ? (theme.error || "#eb6f92") : theme.muted
                    font.family: theme.font
                    font.pixelSize: 12
                }
            }
        }

        Rectangle {
            visible: root.selected !== null
            Layout.fillWidth: true
            implicitHeight: connectionForm.implicitHeight + 24
            Layout.minimumHeight: 0
            Layout.maximumHeight: implicitHeight
            Layout.fillHeight: true
            clip: true
            radius: 8
            color: theme.surface
            ColumnLayout {
                id: connectionForm
                x: 12
                y: 12
                width: parent.width - 24
                spacing: 8
                RowLayout {
                    Layout.fillWidth: true
                    Label {
                        text: root.selected?.ssid || ""
                        textFormat: Text.PlainText
                        color: theme.accent
                        font.family: theme.font
                        font.pixelSize: 13
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    PanelButton {
                        theme: root.theme
                        symbol: "close"
                        text: "Cancel connection"
                        onClicked: { root.selected = null; password.text = ""; }
                    }
                    PanelButton {
                        theme: root.theme
                        symbol: "check"
                        text: "Connect"
                        accented: true
                        enabled: !root.busy
                        onClicked: root.connectSelected()
                    }
                }
                TextField {
                    id: password
                    visible: root.selected !== null && !!root.selected.security && root.selected.security !== "--"
                    Layout.fillWidth: true
                    placeholderText: "Password · blank uses saved credentials"
                    echoMode: TextInput.Password
                    color: theme.text
                    placeholderTextColor: theme.muted
                    selectionColor: theme.accent
                    font.family: theme.font
                    font.pixelSize: 12
                    padding: 10
                    enabled: !root.busy
                    background: Rectangle { color: theme.background; radius: 8; border.color: password.activeFocus ? theme.accent : theme.border }
                    onAccepted: root.connectSelected()
                }
            }
        }

        Repeater {
            model: root.state.devices.filter(device => device.kind === "ethernet" && device.state !== "connected")
            delegate: Column {
                required property var modelData
                Layout.fillWidth: true
                Layout.maximumHeight: implicitHeight
                Layout.alignment: Qt.AlignBottom
                spacing: 10
                Rectangle { width: parent.width; height: 1; color: theme.surface }
                RowLayout {
                    width: parent.width
                    spacing: 8
                    Label { text: "lan"; font.family: "Material Symbols Rounded"; font.pixelSize: 20; color: theme.muted }
                    Label {
                        text: "Ethernet · " + modelData.interface + " · " + modelData.state
                        textFormat: Text.PlainText
                        font.family: theme.font
                        font.pixelSize: 11
                        color: theme.text
                        opacity: 0.65
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    PanelButton {
                        theme: root.theme
                        symbol: "link"
                        text: "Connect Ethernet"
                        enabled: !root.busy && !["unavailable", "unmanaged"].includes(modelData.state)
                        onClicked: root.run({ action: "ethernet", interface: modelData.interface })
                    }
                }
            }
        }
    }
}
