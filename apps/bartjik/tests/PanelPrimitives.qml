import QtQuick
import QtQuick.Window
import Quickshell
import "../qml" as App

ShellRoot {
    QtObject {
        id: fixture
        property var theme: ({background:'#1f1d2e',surface:'#26233a',text:'#e5e5e5',muted:'#a5a0ba',border:'#555169',accent:'#ff53a6',font:'tijpset'})
        property var fields: [
            {label:'Hong Kong',value:'18:42',detail:'Monday, 9 November'},
            {label:'London',value:'10:42',detail:'Monday, 9 November'},
            {label:'New York',value:'05:42',detail:'Monday, 9 November'}
        ]
        property var panelData: ({rows:fields})
        property var row: ({state:{}})
        property var feedback: ({})
        property var selected: ({context:{module:'fixture'}})
        property string provider: 'datetime'
        property bool detached: false
        property bool opened: true
        property real renderedHeight: 268
        property real targetHeight: 268
        function actionFor() {}
        function dismissed() {}
    }
    Window {
        id: preview
        title: 'Bartjik Panel Fixture'
        visible: true; width: 420; height: 268; color: 'transparent'
        Rectangle {
            id: card
            anchors.fill: parent; radius: 12; color: fixture.theme.background
            border.width: 1; border.color: fixture.theme.border
            App.PanelHeader { x:16; y:16; width:parent.width-32; title:'World clock'; symbol:'schedule'; theme:fixture.theme; popup:fixture }
            Rectangle { x:16; y:58; width:parent.width-32; height:1; color:fixture.theme.surface }
            App.StatusDetails { x:16; y:72; width:parent.width-32; popup:fixture }
        }
        Timer {
            interval:1500; running:true
            onTriggered:card.grabToImage(result=>{result.saveToFile('/tmp/bartjik-panel-reference.png'); Qt.quit();})
        }
    }
}
