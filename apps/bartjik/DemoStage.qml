import QtQuick
import Quickshell
import Quickshell.Wayland
PanelWindow {
    id: stage
    required property var controller
    screen: Quickshell.screens.find(s=>s.name===Quickshell.env("BARTJIK_DEMO_MONITOR"))
    anchors {top:true;bottom:true;left:true;right:true}
    color: "#15131f"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Background
    mask: Region {width:0;height:0}
    Text {x:48;y:20;text:"Waybar <font color='#e5e5e5'>vs</font> Bartjik";textFormat:Text.StyledText;color:"#ff53a6";font {family:"tijpset";pixelSize:16;bold:true}}
    Text {anchors.right:parent.right;anchors.rightMargin:48;y:20;text:controller.stageCounter;color:"#a5a0ba";font {family:"tijpset";pixelSize:16}}
    Column {anchors.horizontalCenter:parent.horizontalCenter;y:55;spacing:12
        Text {anchors.horizontalCenter:parent.horizontalCenter;text:controller.stageTitle;color:"#e5e5e5";font {family:"tijpset";pixelSize:25;bold:true}}
        Text {anchors.horizontalCenter:parent.horizontalCenter;text:controller.stageCaption;color:"#a5a0ba";font {family:"tijpset";pixelSize:14}}
    }
    Text {x:270-width/2;y:140;visible:!controller.stageFinale;text:"WAYBAR";color:"#a5a0ba";font {family:"tijpset";pixelSize:16;bold:true}}
    Text {x:(controller.stageFinale?500:730)-width/2;y:140;text:"BARTJIK";color:"#ff53a6";font {family:"tijpset";pixelSize:16;bold:true}}
    Repeater {model:controller.stageIntro?controller.stageMontage:[]
        Image {required property var modelData;required property int index
            source:modelData.path;x:modelData.x-width/2;y:212+Math.floor(index/2)*49-height/2
            width:implicitWidth;height:implicitHeight
            opacity:0;scale:0.8
            SequentialAnimation on opacity {PauseAnimation {duration:100*index} NumberAnimation {to:1;duration:450}}
            NumberAnimation on scale {to:1;duration:700;easing.type:Easing.OutCubic}
        }
    }
    Text {anchors.horizontalCenter:parent.horizontalCenter;anchors.bottom:parent.bottom;anchors.bottomMargin:4;text:"Design by <font color='#ff53a6'>TIJPTJIK</font> · Implementation by <font color='#ff53a6'>ASTRA</font>";textFormat:Text.StyledText;color:"#777185";font {family:"tijpset";pixelSize:13}}
    PanelWindow {screen:stage.screen;anchors {top:true;bottom:true;left:true;right:true} color:"transparent";exclusionMode:ExclusionMode.Ignore;WlrLayershell.layer:WlrLayer.Overlay;mask:Region {width:0;height:0}
        Rectangle {x:0;y:168;width:1000;height:480;color:"#15131f";opacity:controller.stageCurtain?1:0;Behavior on opacity {NumberAnimation {duration:180}}}
        Rectangle {width:10;height:10;radius:5;color:"#ffffff";border.color:"#ff53a6";border.width:2;x:controller.stagePointer.x-5;y:controller.stagePointer.y-5;visible:x>=0}
    }
}
