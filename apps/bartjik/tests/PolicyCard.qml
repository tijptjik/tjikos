import QtQuick
import QtQuick.Window
import Quickshell
import "../qml" as App
ShellRoot {
    QtObject {
        id:fixture
        property var theme: ({background:'#1f1d2e',surface:'#26233a',text:'#e5e5e5',muted:'#a5a0ba',border:'#555169',accent:'#ff53a6',font:'tijpset'})
        property QtObject controller: QtObject {property var state: ({})}
        property string provider:'FIXTURE_PROVIDER'
        property var panelData: FIXTURE_ROW
        property var feedback: ({busy:false})
        property bool idlePanel:provider==='caffeine'
        property bool nightlightPanel:provider==='nightlight'
        property bool monitorPanel:provider==='monitor-layout'
        property bool opened:true
        property bool detached:false
        property bool pinned:false
        property var panelScreen:null
        function action(a,v){}
        function dismissed(){}
    }
    Window {
        title:'Bartjik Panel Fixture';visible:true;width:fixture.monitorPanel?520:420;height:88+contents.item.implicitHeight;color:'transparent'
        Rectangle {
            id:card;anchors.fill:parent;radius:12;color:fixture.theme.background;border.width:1;border.color:fixture.theme.border
            App.PanelHeader {x:16;y:16;width:parent.width-32;title:fixture.panelData.title;symbol:fixture.idlePanel?'coffee':fixture.monitorPanel?'desktop_windows':'nightlight';theme:fixture.theme;popup:fixture}
            Rectangle {x:16;y:58;width:parent.width-32;height:1;color:fixture.theme.surface}
            Loader {id:contents;x:16;y:72;width:parent.width-32;sourceComponent:fixture.idlePanel?idle:fixture.nightlightPanel?night:monitors}
            Component {id:idle;App.IdleDetails {popup:fixture}}
            Component {id:night;App.NightlightDetails {popup:fixture}}
            Component {id:monitors;App.MonitorDetails {popup:fixture}}
        }
        Timer {interval:1500;running:true;onTriggered:card.grabToImage(result=>{result.saveToFile('/tmp/bartjik-FIXTURE_PROVIDER-reference.png');Qt.quit();})}
    }
}
