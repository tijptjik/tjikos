import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: stats
    required property var theme
    required property var device
    property var defaultDns: []
    readonly property var values: [
        ["Local IP", (device.addresses?.ip || []).join(", ") || "No IPv4 address"],
        ["Gateway", device.addresses?.gateway || "None"],
        ["DHCP server", device.addresses?.dhcp || "Not reported / static"],
        [defaultDns.length ? "Default DNS" : "Link DNS", (defaultDns.length ? defaultDns : device.addresses?.dns || []).join(", ") || "None"]
    ]
    spacing: 4
    Repeater {
        model: stats.values
        delegate: RowLayout {
            required property var modelData
            Layout.fillWidth: true
            spacing: 12
            Label {
                text: modelData[0]
                font.family: stats.theme.font
                font.pixelSize: 13
                color: stats.theme.text
                opacity: 0.65
                Layout.preferredWidth: 96
            }
            TextEdit {
                text: modelData[1]
                textFormat: TextEdit.PlainText
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                font.family: stats.theme.font
                font.pixelSize: 13
                color: stats.theme.text
                selectionColor: stats.theme.accent
                Layout.fillWidth: true
            }
        }
    }
}
