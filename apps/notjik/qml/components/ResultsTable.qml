import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: results
    required property var notice
    required property var theme
    required property double now
    spacing: 10

    Repeater {
        model: results.notice.data.results || []
        delegate: ColumnLayout {
            id: resultGroup
            required property var modelData
            Layout.fillWidth: true
            spacing: 5
            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                Text {
                    Layout.fillWidth: true
                    text: resultGroup.modelData.label || "Result"
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    color: results.theme.text
                    font { family: results.theme.font; pixelSize: results.theme.fontSize; bold: true }
                }
                Text {
                    Layout.maximumWidth: results.width * 0.42
                    text: resultGroup.modelData.value || ""
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    horizontalAlignment: Text.AlignRight
                    color: resultGroup.modelData.tone === "critical" ? results.theme.critical : results.theme.accent
                    font { family: results.theme.font; pixelSize: results.theme.fontSize - 1; bold: true }
                }
            }
            Repeater {
                model: resultGroup.modelData.ingredients || []
                delegate: RowLayout {
                    id: ingredient
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.leftMargin: 12
                    spacing: 10
                    Text {
                        text: "└"
                        color: results.theme.muted
                        font { family: results.theme.font; pixelSize: results.theme.fontSize - 2 }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: ingredient.modelData.label || ""
                        textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        color: results.theme.muted
                        font { family: results.theme.font; pixelSize: results.theme.fontSize - 1 }
                    }
                    Text {
                        Layout.maximumWidth: results.width * 0.42
                        text: ingredient.modelData.value || ""
                        textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        horizontalAlignment: Text.AlignRight
                        color: results.theme.muted
                        font { family: results.theme.font; pixelSize: results.theme.fontSize - 2 }
                    }
                }
            }
        }
    }
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: 1
        color: results.theme.muted
        opacity: 0.2
    }
    RowLayout {
        Layout.fillWidth: true
        Text {
            Layout.fillWidth: true
            text: results.notice.data.state || "Running"
            textFormat: Text.PlainText
            color: results.notice.data.tone === "critical" ? results.theme.critical : results.notice.data.tone === "success" ? results.theme.success : results.theme.accent
            font { family: results.theme.font; pixelSize: results.theme.fontSize - 1; bold: true }
        }
        Text {
            text: results.notice.data.duration || ""
            textFormat: Text.PlainText
            color: results.theme.muted
            font { family: results.theme.font; pixelSize: results.theme.fontSize - 2 }
        }
    }
}
