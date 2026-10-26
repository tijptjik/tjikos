import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: traffic
    required property var theme
    property var counters: ({})
    spacing: 8

    function bytes(value) {
        if (value === undefined || value === null) return "—";
        const units = ["B", "KiB", "MiB", "GiB", "TiB"];
        let unit = 0;
        while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
        return value.toFixed(unit === 0 ? 0 : 1) + " " + units[unit];
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 12
        Repeater {
            model: [
                { icon: "swap_vert", label: "Transferred", value: traffic.counters.rx === undefined ? undefined : traffic.counters.rx + traffic.counters.tx, speed: false },
                { icon: "arrow_downward", label: "Download", value: traffic.counters.rxRate, speed: true },
                { icon: "arrow_upward", label: "Upload", value: traffic.counters.txRate, speed: true }
            ]
            delegate: RowLayout {
                required property var modelData
                Layout.fillWidth: true
                spacing: 6
                Label {
                    text: modelData.icon
                    font.family: "Material Symbols Rounded"
                    font.pixelSize: 20
                    color: traffic.theme.accent
                }
                ColumnLayout {
                    spacing: 2
                    Label {
                        text: modelData.label
                        font.family: traffic.theme.font
                        font.pixelSize: 11
                        color: traffic.theme.text
                        opacity: 0.65
                    }
                    Label {
                        text: traffic.bytes(modelData.value) + (modelData.speed && modelData.value != null ? "/s" : "")
                        font.family: traffic.theme.font
                        font.pixelSize: 14
                        font.weight: Font.DemiBold
                        color: traffic.theme.text
                    }
                }
            }
        }
    }
    Label {
        text: "↓ " + traffic.bytes(traffic.counters.rx) + "   ↑ " + traffic.bytes(traffic.counters.tx) + " · since adapter reset"
        font.family: traffic.theme.font
        font.pixelSize: 11
        color: traffic.theme.text
        opacity: 0.65
        wrapMode: Text.Wrap
        horizontalAlignment: Text.AlignHCenter
        Layout.fillWidth: true
    }
}
