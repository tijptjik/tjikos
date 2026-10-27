import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: stage
    required property var controller
    property string mode: "intro"
    property string heading: "Notifications with flair"
    property string introHeading: "Notifications with flair"
    property string introCaption: "Live updates. Telegram integration. Replies in place."
    property string caption: "Live updates. Telegram integration. Replies in place."
    property int chapter: 0
    property int chapterCount: 3
    property real reveal: 1
    property string toolName: "Notjik"
    property string toolId: "notjik"
    property string repository: "github.com/tijptjik/tooltjik/tree/main/apps/notjik"
    property string iconRoot: ""
    screen: Quickshell.screens[0]
    visible: controller.ready
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Background
    WlrLayershell.namespace: "tjikOS-showcase"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    mask: Region { width: 0; height: 0 }
    color: "#191724"
    function scene(data) {
        if (data.mode === "intro") { introHeading = data.heading || introHeading; introCaption = data.caption || introCaption; }
        if (data.mode !== undefined) mode = data.mode;
        if (data.heading !== undefined) heading = data.heading;
        if (data.caption !== undefined) caption = data.caption;
        if (data.chapterCount !== undefined) chapterCount = data.chapterCount;
        if (data.chapter !== undefined) chapter = data.chapter;
        reveal = 0;
        revealLater.restart();
    }
    Timer { id: revealLater; interval: 50; onTriggered: stage.reveal = 1 }
    Behavior on reveal { NumberAnimation { duration: 500; easing.type: Easing.OutCubic } }
    Rectangle {
        x: 48; y: 44; width: 4; height: 22; radius: 2; color: "#ff53a6"
    }
    Text {
        x: 66; y: 44; text: "tjikOS"; color: "#e5e5e5"
        font { family: "tijpset"; pixelSize: 19; weight: Font.DemiBold }
    }
    Row {
        anchors { right: parent.right; rightMargin: 48; top: parent.top; topMargin: 40 }
        spacing: 10
        Image { width: 30; height: 30; source: stage.iconRoot + "/" + stage.toolId + ".svg"; sourceSize: Qt.size(60,60) }
        Text { anchors.verticalCenter: parent.verticalCenter; text: stage.toolName; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 18 } }
    }
    Column {
        x: 48; y: 99 + (1-stage.reveal)*12; spacing: 10
        opacity: stage.mode === "feature" ? stage.reveal : 0
        Behavior on opacity { NumberAnimation { duration: 300 } }
        Text { text: stage.heading; color: "#e5e5e5"; font { family: "tijpset"; pixelSize: 34; weight: Font.DemiBold } }
        Text { text: stage.caption; color: "#a5a0ba"; font { family: "tijpset"; pixelSize: 18 } }
    }
    Column {
        anchors.centerIn: parent
        anchors.verticalCenterOffset: -16 + (1-stage.reveal)*18
        spacing: 21
        opacity: stage.mode !== "feature" ? stage.reveal : 0
        scale: .98 + .02*stage.reveal
        Behavior on opacity { NumberAnimation { duration: 350 } }
        Image {
            anchors.horizontalCenter: parent.horizontalCenter
            width: stage.mode === "outro" ? 108 : 88; height: width
            source: stage.iconRoot + "/" + stage.toolId + ".svg"
            sourceSize: Qt.size(216,216)
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: stage.mode === "outro" ? stage.toolName : stage.introHeading
            color: "#e5e5e5"; font { family: "tijpset"; pixelSize: stage.mode === "outro" ? 52 : 42; weight: Font.DemiBold }
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: stage.mode === "outro" ? stage.repository : stage.introCaption
            color: stage.mode === "outro" ? "#9ccfd8" : "#a5a0ba"
            font { family: "tijpset"; pixelSize: 19 }
        }
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 12
            visible: stage.mode === "outro"
            Repeater {
                model: ["notjik", "bartjik", "runtjik", "tjikup", "mailtjik", "greetjik"]
                Image {
                    required property string modelData
                    width: 35; height: 35; opacity: modelData === stage.toolId ? 1 : .48
                    source: stage.iconRoot + "/" + modelData + ".svg"; sourceSize: Qt.size(70,70)
                }
            }
        }
    }
    Rectangle { x: 48; y: parent.height-55; width: parent.width-96; height: 1; color: "#393546" }
    Row {
        x: 48; y: parent.height-35; spacing: 9
        Repeater {
            model: stage.chapterCount
            Rectangle {
                required property int index
                width: stage.chapter === index+1 ? 48 : 18; height: 3; radius: 1.5
                color: stage.chapter >= index+1 ? "#ff53a6" : "#393546"
                Behavior on width { NumberAnimation { duration: 450; easing.type: Easing.InOutCubic } }
                Behavior on color { ColorAnimation { duration: 300 } }
            }
        }
    }
    Text {
        anchors { right: parent.right; rightMargin: 48; bottom: parent.bottom; bottomMargin: 22 }
        text: "ROSE PUNK  /  NATIVE TOOLS"; color: "#777185"
        font { family: "tijpset"; pixelSize: 12; letterSpacing: 1.1 }
    }
}
