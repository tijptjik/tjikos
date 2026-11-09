import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: detail
    required property var notice
    required property var theme
    required property double now
    spacing: 10
    function cells() {
        const result = [];
        for (const field of detail.notice.data.fields || []) result.push(field.label, field.value);
        return result;
    }
    Text {
        Layout.fillWidth: true
        text: detail.notice.data.heading || ""
        textFormat: Text.PlainText
        color: detail.theme.text
        font { family: detail.theme.font; pixelSize: detail.theme.fontSize; bold: true }
        wrapMode: Text.Wrap
    }
    Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: detail.theme.border }
    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: 14
        rowSpacing: 5
        Repeater {
            model: detail.cells()
            Text {
                required property int index
                required property string modelData
                Layout.fillWidth: index % 2 === 1
                Layout.maximumWidth: index % 2 === 0 ? 100 : 100000
                Layout.alignment: Qt.AlignTop
                text: modelData
                textFormat: Text.PlainText
                color: index % 2 === 0 ? detail.theme.muted : detail.theme.text
                font { family: detail.theme.font; pixelSize: detail.theme.fontSize - 1; bold: index % 2 === 1 }
                wrapMode: Text.Wrap
            }
        }
    }
}
