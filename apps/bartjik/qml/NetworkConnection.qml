import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: connection
    required property var theme
    required property var device
    required property var networks
    required property var traffic
    property var defaultDns: []
    property bool busy: false
    readonly property var accessPoint: networks.find(network => network.active && network.interface === device.interface)
    signal disconnectRequested()
    implicitHeight: contents.implicitHeight + 24
    color: theme.surface
    radius: 10

    ColumnLayout {
        id: contents
        x: 12
        y: 12
        width: parent.width - 24
        spacing: 10
        RowLayout {
            Layout.fillWidth: true
            Label {
                text: connection.device.kind === "wifi" ? "wifi" : "lan"
                font.family: "Material Symbols Rounded"
                font.pixelSize: 22
                color: connection.theme.accent
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    text: connection.accessPoint?.ssid || connection.device.connection || connection.device.interface
                    textFormat: Text.PlainText
                    font.family: connection.theme.font
                    font.pixelSize: 14
                    font.weight: Font.DemiBold
                    color: connection.theme.text
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                Label {
                    text: "Connected · " + connection.device.interface + (connection.accessPoint ? " · " + connection.accessPoint.signal + "%" : "")
                    font.family: connection.theme.font
                    font.pixelSize: 11
                    color: connection.theme.text
                    opacity: 0.65
                }
            }
            PanelButton {
                theme: connection.theme
                symbol: "link_off"
                text: "Disconnect " + (connection.accessPoint?.ssid || connection.device.interface)
                enabled: !connection.busy
                onClicked: connection.disconnectRequested()
            }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: connection.theme.border; opacity: 0.45 }
        NetworkTraffic { theme: connection.theme; counters: connection.traffic; Layout.fillWidth: true }
        Rectangle { Layout.fillWidth: true; height: 1; color: connection.theme.border; opacity: 0.45 }
        NetworkAddresses { theme: connection.theme; device: connection.device; defaultDns: connection.defaultDns; Layout.fillWidth: true }
    }
}
