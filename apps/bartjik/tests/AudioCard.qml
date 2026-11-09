import QtQuick
import QtQuick.Window
import Quickshell
import "../qml" as App
ShellRoot {
    QtObject {
        id:fixture
        property var theme: ({background:'#1f1d2e',surface:'#26233a',text:'#e5e5e5',muted:'#a5a0ba',border:'#555169',accent:'#ff53a6',font:'tijpset'})
        property var controller: ({demo:true})
        property bool audioPanel:true
        property bool opened:false
        property bool detached:false
        function dismissed(){}
    }
    Window {
        title:'Bartjik Panel Fixture';visible:true;width:420;height:88+contents.implicitHeight;color:'transparent'
        Rectangle {
            id:card;anchors.fill:parent;radius:12;color:fixture.theme.background;border.width:1;border.color:fixture.theme.border
            App.PanelHeader {x:16;y:16;width:parent.width-32;title:'Audio';imageSource:'icons/proicons/volume.svg';theme:fixture.theme;popup:fixture}
            Rectangle {x:16;y:58;width:parent.width-32;height:1;color:fixture.theme.surface}
            App.AudioDetails {id:contents;x:16;y:72;width:parent.width-32;popup:fixture;levels:({'__output':0.08,'__input':0.08,'Music':0.08})}
        }
        Timer {interval:1500;running:true;onTriggered:card.grabToImage(result=>{result.saveToFile('/tmp/bartjik-audio-reference.png');Qt.quit();})}
    }
}
