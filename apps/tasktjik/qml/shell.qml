import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root
    property var pending: []
    function request(value) {
        pending.push(value);
        drain();
    }
    function drain() {
        if (backend.running || !pending.length) return;
        backend.payload = JSON.stringify(pending.shift());
        backend.running = true;
    }
    Process {
        id: backend
        property string payload
        command: ['python3', '-B', Quickshell.env('TASKTJIK_HOME') + '/tasktjik.py', 'state']
        stdinEnabled: true
        onStarted: { write(payload); stdinEnabled = false; }
        stdout: StdioCollector {
            onStreamFinished: {
                try { view.stateData = JSON.parse(text); view.errorMessage = ''; }
                catch (e) { view.errorMessage = 'Could not load tasks. Check Tasktjik authority configuration.'; }
            }
        }
        stderr: StdioCollector { onStreamFinished: { if (text.trim()) view.errorMessage = text.trim(); } }
        onExited: { stdinEnabled = true; Qt.callLater(root.drain); }
    }
    FloatingWindow {
        title: 'Tasktjik'
        implicitWidth: 860; implicitHeight: 680
        color: '#26233a'
        TodoApp {
            id: view
            anchors.fill: parent; anchors.margins: 20
            focus: true
            view: 'all'
            theme: ({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
            onRequest: value => root.request(value)
            onExitApp: Qt.quit()
        }
    }
    Timer { interval: 10000; running: true; repeat: true; onTriggered: { if (!view.editor && !backend.running) root.request({action:'list'}); } }
    Component.onCompleted: root.request({action:'list'})
}
