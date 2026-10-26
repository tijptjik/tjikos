import QtQuick

Column {
    id:details
    required property var popup
    readonly property var theme:popup.theme
    readonly property var info:popup.panelData
    spacing:14
    Column {
        width:parent.width;spacing:4
        Text {text:details.info.balanceLabel || 'Credits remaining';font.pixelSize:12;color:details.theme.muted}
        Row {
            spacing:8
            Text {text:details.info.balance || '—';font.family:details.theme.font;font.pixelSize:32;color:details.theme.text;textFormat:Text.PlainText}
            Text {anchors.baseline:parent.children[0].baseline;text:'USD';font.pixelSize:12;color:details.theme.muted}
        }
        Text {visible:!!details.popup.row?.state?.stale;text:'Cached balance · waiting for an update';font.pixelSize:11;color:details.theme.muted}
    }
    Rectangle {width:parent.width;height:1;color:Qt.rgba(1,1,1,.09)}
    Column {
        width:parent.width;spacing:10
        Text {text:'DAILY TOKEN USAGE';font.pixelSize:10;font.letterSpacing:1;color:details.theme.muted}
        Text {width:parent.width;text:details.info.activityNote || 'Balance and usage are currently unavailable.';wrapMode:Text.WordWrap;textFormat:Text.PlainText;font.pixelSize:12;color:details.theme.muted}
        Column {
            width:parent.width;spacing:4
            Repeater {
                model:details.popup.fields.filter(f=>typeof f.tokens==='number')
                Item {
                    required property var modelData
                    width:parent.width;height:26
                    Text {anchors.verticalCenter:parent.verticalCenter;text:parent.modelData.label;font.family:details.theme.font;font.pixelSize:13;color:details.theme.muted;textFormat:Text.PlainText}
                    Text {anchors.right:parent.right;anchors.verticalCenter:parent.verticalCenter;text:parent.modelData.value;font.family:details.theme.font;font.pixelSize:14;color:details.theme.text;textFormat:Text.PlainText}
                }
            }
        }
    }
}
