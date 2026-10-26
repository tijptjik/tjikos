import QtQuick
import QtQuick.Layouts

RowLayout {
    id: component
    required property var notice
    required property var theme
    required property double now
    spacing: 8
    function escaped(value) {
        return String(value).replace(/&/g, "&amp;").replace(/</g, "&lt;")
            .replace(/>/g, "&gt;").replace(/\"/g, "&quot;");
    }
    function rainbowText(value) {
        const colors = ["#f6c177", "#eb6f92", "#c4a7e7", "#9ccfd8", "#ebbcba"];
        return Array.from(String(value)).map((character, index) =>
            character === " " ? " " : "<font color=\"" + colors[index % colors.length]
                + "\">" + escaped(character) + "</font>").join("");
    }
    Rectangle {
        implicitWidth: 6
        implicitHeight: 6
        radius: 3
        color: component.notice.data.tone === "critical" ? component.theme.critical : component.notice.data.tone === "success" ? component.theme.success : component.theme.accent
    }
    Text {
        Layout.maximumWidth: component.width * 0.55
        readonly property string state: component.notice.data.state || "Status"
        readonly property bool rainbow: component.notice.data["rainbow-text"] === true
        text: rainbow ? component.rainbowText(state) : state
        // StyledText keeps the same compact line box as a plain status label.
        textFormat: rainbow ? Text.StyledText : Text.PlainText
        elide: Text.ElideRight
        color: component.notice.data.tone === "critical" ? component.theme.critical : component.notice.data.tone === "success" ? component.theme.success : component.theme.text
        font { family: component.theme.font; pixelSize: component.theme.fontSize - 1; bold: true }
    }
    Text {
        Layout.fillWidth: true
        text: component.notice.data.detail || ""
        textFormat: Text.PlainText
        elide: Text.ElideRight
        color: component.notice.urgency === "critical" ? component.theme.text : component.theme.muted
        font { family: component.theme.font; pixelSize: component.theme.fontSize - 2 }
        horizontalAlignment: Text.AlignRight
    }
}
