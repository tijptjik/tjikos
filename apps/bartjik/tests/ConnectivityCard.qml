import QtQuick
import QtQuick.Window
import Quickshell
import "../qml" as App
ShellRoot {
    QtObject {
        id:fixture
        property var theme: ({background:'#1f1d2e',surface:'#26233a',text:'#e5e5e5',muted:'#a5a0ba',border:'#555169',accent:'#ff53a6',font:'tijpset'})
        property var data: FIXTURE_ROW
        property var panelData: data.sections[0].row.panel
        property var row: data.sections[0].row
        property var fields: []
        property string provider:'mullvad'
        property var feedback: ({})
        property var selected: ({context:{module:'vpn'}})
        property bool detached:false
        property bool opened:false
        property real renderedHeight:0
        property real targetHeight:0
        function actionFor(module,name,selection){}
        function action(name,selection){}
        function dismissed(){}
    }
    Window {
        title:'Bartjik Panel Fixture';visible:true;width:420;height:88+contents.implicitHeight;color:'transparent'
        Rectangle {
            id:card;anchors.fill:parent;radius:12;color:fixture.theme.background;border.width:1;border.color:fixture.theme.border
            App.PanelHeader {x:16;y:16;width:parent.width-32;title:'Mullvad';imageSource:'icons/tray/mullvad.svg';theme:fixture.theme;popup:fixture}
            Rectangle {x:16;y:58;width:parent.width-32;height:1;color:fixture.theme.surface}
            Column {
                id:contents;x:16;y:72;width:parent.width-32;spacing:12
                App.StatusDetails {width:parent.width;popup:fixture}
                App.MullvadLocations {width:parent.width;popup:fixture}
                Item {
                    width:parent.width;height:56
                    App.PanelHeader {y:12;width:parent.width;theme:fixture.theme;popup:fixture;title:'Tailscale';imageSource:'icons/tailscale.svg';showHandle:false}
                    Rectangle {y:54;width:parent.width;height:1;color:fixture.theme.surface}
                }
                App.StatusDetails {width:parent.width;popup:fixture;provider:'tailscale';info:fixture.data.sections[1].row.panel;statusRow:fixture.data.sections[1].row;actionModule:'tailnet'}
            }
        }
        Timer {interval:1500;running:true;onTriggered:card.grabToImage(result=>{result.saveToFile('/tmp/bartjik-connectivity-reference.png');Qt.quit();})}
    }
}
