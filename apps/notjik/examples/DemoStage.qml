import QtQuick
import Quickshell
import Quickshell.Wayland
PanelWindow {
    id: stage
    property real comparisonReveal: controller.cycleIntro ? 0 : 1
    Behavior on comparisonReveal { NumberAnimation { duration: 450; easing.type: Easing.InOutQuad } }
    required property var controller
    screen: Quickshell.screens.find(s => s.name === controller.config.placement.primaryMonitor) || Quickshell.screens[0]
    visible: controller.ready
    implicitWidth: 1000
    implicitHeight: 680
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "notjik-demo-stage"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    mask: Region { width: 0; height: 0 }
    color: "#15131f"
    Text { x: 48; y: 20; textFormat: Text.StyledText; text: "Dunst <font color='#ffffff'>vs</font> Notjik"; color: "#ff53a6"; font { family: "tijpset"; pixelSize: 16; bold: true } }
    Text { anchors.right: parent.right; anchors.rightMargin: 48; y: 20; text: stage.controller.cycleCounter; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 16 } }
    Column {
        opacity: stage.controller.cycleIntro || stage.controller.cycleOutro ? 1 : stage.comparisonReveal
        y: stage.controller.cycleOutro ? parent.height/2 - 35 : 55; width: parent.width; spacing: 12
        Text { anchors.horizontalCenter: parent.horizontalCenter; text: stage.controller.cycleTitle; color: "#e5e5e5"; font { family: "tijpset"; pixelSize: 25; bold: true } }
        Text { anchors.horizontalCenter: parent.horizontalCenter; text: stage.controller.cycleCaption; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 14 } }
    }
    Text { opacity: stage.comparisonReveal; visible: !stage.controller.cycleOutro && opacity > 0; x: parent.width/2 - 230 - width/2; y: 140; text: "DUNST"; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 18; bold: true } }
    Text { opacity: stage.comparisonReveal; visible: !stage.controller.cycleOutro && opacity > 0; x: parent.width/2 + 230 - width/2; y: 140; text: "NOTJIK"; color: "#ff53a6"; font { family: "tijpset"; pixelSize: 18; bold: true } }
    Item {
        id: montage
        x: 48; y: 145; width: 904; height: 470
        opacity: stage.controller.cycleIntro ? 1 : 0
        scale: stage.controller.cycleIntro ? 1 : 0.96
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 800; easing.type: Easing.InOutQuad } }
        Behavior on scale { NumberAnimation { duration: 800; easing.type: Easing.InOutQuad } }
        Row {
            spacing: 24
            Repeater {
                model: 4
                Column {
                    required property int index
                    width: 208; spacing: 15
                    Repeater {
                        model: stage.controller.cycleIntroCards.filter((item, i) => i % 4 === index)
                        Item {
                            required property var modelData
                            property real reveal: 0
                            opacity: reveal
                            scale: 0.82 + 0.18 * reveal
                            rotation: (1-reveal) * (modelData.demoOrder % 2 ? 4 : -4)
                            transform: Translate { y: (1-reveal) * 38 }
                            Behavior on reveal { NumberAnimation { duration: 480; easing.type: Easing.OutCubic } }
                            Timer { interval: 50 + modelData.demoOrder * 65; running: true; onTriggered: parent.reveal = 1 }
                            width: 208; height: preview.implicitHeight * 0.495 + 22
                            Text { text: modelData.demoLabel; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 12 } }
                            NotificationCard {
                                id: preview
                                y: 22; width: 420; height: implicitHeight
                                scale: 0.495; transformOrigin: Item.TopLeft
                                notice: modelData
                                controller: stage.controller
                                monitorName: stage.screen.name
                                enabled: false
                            }
                        }
                    }
                }
            }
        }
    }
    Text { anchors.horizontalCenter: parent.horizontalCenter; anchors.bottom: parent.bottom; anchors.bottomMargin: 18;
        textFormat: Text.StyledText;
        text: "Design by <font color='#ff53a6'>TIJPTJIK</font> · Implementation by <font color='#ff53a6'>ASTRA</font>";
        color: "#777185"; font { family: "tijpset"; pixelSize: 13 } }
}
