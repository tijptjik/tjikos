import QtQuick

Item {
    id:header
    required property var theme
    property string title:''
    property string imageSource:''
    property string symbol:''
    property bool showHandle:popup.detached
    required property var popup
    implicitHeight:32
    Image {x:0;anchors.verticalCenter:parent.verticalCenter;width:22;height:22;visible:!!header.imageSource;source:header.imageSource;fillMode:Image.PreserveAspectFit;sourceSize:Qt.size(44,44)}
    Text {anchors.verticalCenter:parent.verticalCenter;visible:!header.imageSource;text:header.symbol;font.family:'Material Symbols Rounded';font.pixelSize:22;color:header.theme.text}
    Text {x:32;anchors.verticalCenter:parent.verticalCenter;width:parent.width-(header.showHandle?76:32);text:header.title;font.family:header.theme.font;font.pixelSize:16;font.weight:Font.DemiBold;color:header.theme.text;textFormat:Text.PlainText;elide:Text.ElideRight}
    PanelHandle {visible:header.showHandle;anchors.right:parent.right;theme:header.theme;popup:header.popup}
}
