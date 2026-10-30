import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Column {
    id:details
    required property var popup
    readonly property var theme:popup.theme
    readonly property var info:popup.panelData
    readonly property bool busy:popup.feedback.busy===true
    width:parent.width;spacing:16
    function duration(seconds) {
        if(!(seconds>0))return info.estimateUnavailable || 'Estimating…';
        const minutes=Math.max(1,Math.round(seconds/60)),hours=Math.floor(minutes/60);
        return 'About '+(hours?hours+' hr'+(minutes%60?' '+minutes%60+' min':''):minutes+' min');
    }
    Rectangle {
        width:parent.width;height:summary.implicitHeight+24;radius:10;color:details.theme.surface
        Column {
            id:summary;x:12;y:12;width:parent.width-24;spacing:12
            RowLayout {
                width:parent.width
                Text {Layout.fillWidth:true;text:details.info.status || 'Battery';font.family:details.theme.font;font.pixelSize:13;color:details.theme.text}
                Text {text:Math.round(details.info.percent || 0)+'%';font.family:details.theme.font;font.pixelSize:26;color:details.theme.text}
            }
            Rectangle {width:parent.width;height:1;color:details.theme.background}
            Column {
                width:parent.width;spacing:5
                Text {text:details.info.estimateLabel || '';font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted}
                Text {objectName:'battery-time-estimate';text:details.duration(details.info.seconds);font.family:details.theme.font;font.pixelSize:18;color:details.theme.text}
            }
        }
    }
    Column {
        width:parent.width;spacing:6
        Text {text:'Power mode';font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted;bottomPadding:4}
        Repeater {
            model:details.info.profiles || []
            delegate:Button {
                id:mode
                required property var modelData
                objectName:'battery-profile-'+modelData.id
                width:parent.width;height:48;hoverEnabled:true
                enabled:modelData.available && !details.busy && !details.popup.row?.state?.stale
                readonly property bool selected:modelData.active
                Accessible.checkable:true
                Accessible.checked:selected
                Accessible.name:modelData.label
                Accessible.role:Accessible.RadioButton
                opacity:enabled?1:.45
                background:Rectangle {radius:8;color:mode.hovered || mode.selected?details.theme.surface:'transparent'}
                contentItem:RowLayout {
                    spacing:12
                    Rectangle {
                        Layout.leftMargin:10;width:16;height:16;radius:8;color:'transparent'
                        border.width:1.5;border.color:mode.selected?details.theme.accent:details.theme.muted
                        Rectangle {anchors.centerIn:parent;width:8;height:8;radius:4;visible:mode.selected;color:details.theme.accent}
                    }
                    Text {Layout.fillWidth:true;text:mode.modelData.label;font.family:details.theme.font;font.pixelSize:13;color:details.theme.text}
                    Text {Layout.rightMargin:10;text:mode.selected?'Active':!mode.modelData.available?'Unavailable':'';font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted}
                }
                onClicked:if(!selected)details.popup.action('profile',modelData.id)
            }
        }
    }
    Text {width:parent.width;visible:!!text;text:details.info.profileMessage || '';wrapMode:Text.Wrap;font.family:details.theme.font;font.pixelSize:11;color:details.theme.muted}
}
